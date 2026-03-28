use std::sync::{Arc, Mutex, RwLock};

use cplp_scene::renderer::{self, SceneRenderer};

use crate::types::CplpResult;

/// グローバルシーン状態 — Arc で参照カウント管理
///
/// Arc パターンにより、render スレッドが参照を保持している間は
/// detach が SceneState を解放できない（UAF が構造的に不可能）。
static SCENE: RwLock<Option<Arc<SceneState>>> = RwLock::new(None);

/// 変更されるフィールド（Mutex で保護）
struct SceneInner {
    renderer: SceneRenderer,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
}

struct SceneState {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    inner: Mutex<SceneInner>,
}

// SAFETY: SceneState の可変フィールドは Mutex<SceneInner> で保護済み。
// device, queue, surface は wgpu 内部で Sync（Arc ベース）。
unsafe impl Send for SceneState {}
unsafe impl Sync for SceneState {}

/// Arc<SceneState> のクローンを取得（render/resize から使用）
fn scene_ref() -> Option<Arc<SceneState>> {
    SCENE.read().ok()?.clone()
}

/// CAMetalLayer をアタッチして 3D シーン描画を開始
///
/// # Safety
/// `metal_layer` は有効な `CAMetalLayer` ポインタであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_scene_attach(
    metal_layer: *mut std::ffi::c_void,
    width: u32,
    height: u32,
) -> CplpResult {
    if metal_layer.is_null() {
        return crate::error::set_error(
            CplpResult::InvalidArgument,
            "cplp_scene_attach: metal_layer が null",
        );
    }

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..Default::default()
    });

    let surface = unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(metal_layer))
    };
    let surface = match surface {
        Ok(s) => s,
        Err(e) => {
            return crate::error::set_error(
                CplpResult::InternalError,
                format!("Surface 作成失敗: {e}"),
            );
        }
    };

    let result = pollster::block_on(async {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        tracing::info!("Scene GPU adapter: {}", adapter.get_info().name);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;

        let config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| anyhow::anyhow!("Surface not compatible"))?;
        surface.configure(&device, &config);

        // CplpRuntime の AudioGraph を参照してレンダラーに渡す
        let graph = crate::runtime().and_then(|rt| {
            rt.graph.lock().ok().map(|g| g.clone())
        });
        let renderer = SceneRenderer::with_graph(
            &device,
            config.format,
            width,
            height,
            graph.as_ref(),
        );
        let depth_view = renderer::create_depth_texture(&device, width, height);

        Ok::<_, anyhow::Error>(SceneState {
            device,
            queue,
            surface,
            inner: Mutex::new(SceneInner { renderer, config, depth_view }),
        })
    });

    match result {
        Ok(state) => {
            let arc = Arc::new(state);
            if let Ok(mut guard) = SCENE.write() {
                *guard = Some(arc);
            }
            tracing::info!("cplp_scene_attach: 完了 ({}x{})", width, height);
            CplpResult::Ok
        }
        Err(e) => crate::error::set_error(
            CplpResult::InternalError,
            format!("Scene 初期化失敗: {e}"),
        ),
    }
}

/// シーンをデタッチ
///
/// RwLock 内の Arc を None にする。既存の render/resize が Arc を保持していれば
/// 最後の Arc がドロップされるまで SceneState は生存する（UAF 不可能）。
#[unsafe(no_mangle)]
pub extern "C" fn cplp_scene_detach() -> CplpResult {
    if let Ok(mut guard) = SCENE.write() {
        *guard = None;
    }
    tracing::info!("cplp_scene_detach: 完了");
    CplpResult::Ok
}

/// リサイズ
#[unsafe(no_mangle)]
pub extern "C" fn cplp_scene_resize(width: u32, height: u32) -> CplpResult {
    let Some(state) = scene_ref() else {
        return CplpResult::NotInitialized;
    };

    if width == 0 || height == 0 {
        return CplpResult::Ok;
    }

    let Ok(mut inner) = state.inner.lock() else {
        return CplpResult::InternalError;
    };
    inner.config.width = width;
    inner.config.height = height;
    state.surface.configure(&state.device, &inner.config);
    inner.depth_view = renderer::create_depth_texture(&state.device, width, height);
    inner.renderer.resize(width, height);

    CplpResult::Ok
}

/// 1 フレーム描画（DisplayLink から呼ばれる）
#[unsafe(no_mangle)]
pub extern "C" fn cplp_scene_render() -> CplpResult {
    let Some(state) = scene_ref() else {
        return CplpResult::NotInitialized;
    };

    let Ok(inner) = state.inner.lock() else {
        return CplpResult::InternalError;
    };

    let frame = match state.surface.get_current_texture() {
        Ok(f) => f,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            state.surface.configure(&state.device, &inner.config);
            return CplpResult::Ok;
        }
        Err(e) => {
            return crate::error::set_error(
                CplpResult::InternalError,
                format!("Surface texture error: {e}"),
            );
        }
    };

    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    inner
        .renderer
        .render(&state.device, &state.queue, &view, &inner.depth_view);
    frame.present();

    CplpResult::Ok
}

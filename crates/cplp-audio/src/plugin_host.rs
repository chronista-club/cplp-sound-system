use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui,
    Window as ClapWindow,
};
use clack_host::events::event_types::*;
use clack_host::events::Match;
use clack_host::prelude::*;
use clack_host::process::StartedPluginAudioProcessor;
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window as WinitWindow, WindowId};

// ─── プラグインスキャン ──────────────────────────────────────

/// スキャンで見つかったプラグイン情報
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub bundle_path: PathBuf,
}

/// インストール済み CLAP プラグインをスキャン
///
/// REQ-AUDIO-001: CLAP プラグインのホスティング
pub fn scan_plugins() -> Vec<PluginInfo> {
    let mut plugins = Vec::new();

    for bundle_path in clack_finder::ClapFinder::from_standard_paths() {
        match scan_bundle(&bundle_path) {
            Ok(mut found) => plugins.append(&mut found),
            Err(e) => {
                warn!("Failed to scan {:?}: {e}", bundle_path);
            }
        }
    }

    info!("Found {} CLAP plugins", plugins.len());
    plugins
}

fn scan_bundle(path: &Path) -> Result<Vec<PluginInfo>> {
    let bundle = unsafe { PluginBundle::load(path)? };

    let factory = bundle
        .get_plugin_factory()
        .context("No plugin factory")?;

    let mut plugins = Vec::new();
    for descriptor in factory.plugin_descriptors() {
        let id = cstr_to_string(descriptor.id());
        let name = cstr_to_string(descriptor.name());
        let vendor = cstr_to_string(descriptor.vendor());
        let version = cstr_to_string(descriptor.version());

        plugins.push(PluginInfo {
            id,
            name,
            vendor,
            version,
            bundle_path: path.to_path_buf(),
        });
    }

    Ok(plugins)
}

fn cstr_to_string(s: Option<&CStr>) -> String {
    s.and_then(|s| s.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

// ─── CLAP ホスト実装 ──────────────────────────────────────

/// ホスト→メインスレッド間のメッセージ
enum HostMessage {
    /// プラグインがメインスレッドコールバックを要求
    RequestCallback,
    /// プラグインGUIが閉じられた
    GuiClosed,
}

/// cplp-sound-system の CLAP ホスト
struct CplpHost;

impl HostHandlers for CplpHost {
    type Shared<'a> = CplpHostShared;
    type MainThread<'a> = CplpHostMainThread;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder.register::<HostGui>();
    }
}

struct CplpHostShared {
    sender: mpsc::Sender<HostMessage>,
}

impl<'a> SharedHandler<'a> for CplpHostShared {
    fn initializing(&self, _instance: InitializingPluginHandle<'a>) {}

    fn request_restart(&self) {
        warn!("Plugin requested restart (not supported)");
    }

    fn request_process(&self) {
        // cpal が常に process を呼ぶので無視
    }

    fn request_callback(&self) {
        let _ = self.sender.send(HostMessage::RequestCallback);
    }
}

impl HostGuiImpl for CplpHostShared {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, _new_size: GuiSize) -> Result<(), HostError> {
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn closed(&self, _was_destroyed: bool) {
        let _ = self.sender.send(HostMessage::GuiClosed);
    }
}

struct CplpHostMainThread;

impl<'a> MainThreadHandler<'a> for CplpHostMainThread {
    fn initialized(&mut self, _instance: InitializedPluginHandle<'a>) {}
}

fn host_info() -> HostInfo {
    HostInfo::new(
        "cplp-sound-system",
        "cplp",
        "https://github.com/mako-357/cplp-sound-system",
        env!("CARGO_PKG_VERSION"),
    )
    .unwrap()
}

// ─── MIDIイベント ──────────────────────────────────────

/// MIDIイベント（ringbuf で転送される固定サイズ型）
#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    /// 0x90 = NoteOn, 0x80 = NoteOff
    pub status: u8,
    pub key: u8,
    pub velocity: u8,
}

/// lock-free でMIDIイベントをオーディオスレッドに送信するコントローラ
///
/// ringbuf ベースで複数イベント/バッファサイクルに対応（和音・高速連打OK）
pub struct NoteController {
    producer: ringbuf::HeapProd<MidiEvent>,
}

/// オーディオスレッド側のイベント受信端
struct NoteReceiver {
    consumer: ringbuf::HeapCons<MidiEvent>,
}

fn note_channel(capacity: usize) -> (NoteController, NoteReceiver) {
    let rb = HeapRb::<MidiEvent>::new(capacity);
    let (prod, cons) = rb.split();
    (
        NoteController { producer: prod },
        NoteReceiver { consumer: cons },
    )
}

impl NoteController {
    /// ノートオンを送信
    pub fn note_on(&mut self, key: u8, velocity: u8) {
        let _ = self.producer.try_push(MidiEvent {
            status: 0x90,
            key,
            velocity,
        });
    }

    /// ノートオフを送信
    pub fn note_off(&mut self, key: u8) {
        let _ = self.producer.try_push(MidiEvent {
            status: 0x80,
            key,
            velocity: 0,
        });
    }
}

impl NoteReceiver {
    /// 保留中の全イベントを EventBuffer に書き込む
    fn drain_to_event_buffer(&mut self, event_buf: &mut EventBuffer) {
        while let Some(evt) = self.consumer.try_pop() {
            match evt.status & 0xF0 {
                0x90 if evt.velocity > 0 => {
                    let velocity = evt.velocity as f64 / 127.0;
                    event_buf.push(&NoteOnEvent::new(
                        0,
                        Pckn::new(0u16, 0u16, evt.key as u16, Match::All),
                        velocity,
                    ));
                }
                0x90 => {
                    // velocity 0 の NoteOn は NoteOff として扱う
                    event_buf.push(&NoteOffEvent::new(
                        0,
                        Pckn::new(0u16, 0u16, evt.key as u16, Match::All),
                        0.0,
                    ));
                }
                0x80 => {
                    event_buf.push(&NoteOffEvent::new(
                        0,
                        Pckn::new(0u16, 0u16, evt.key as u16, Match::All),
                        0.0,
                    ));
                }
                _ => {}
            }
        }
    }
}

// ─── オーディオ処理 ──────────────────────────────────────

/// CLAPプラグインのオーディオプロセッサ（オーディオスレッドで使用）
///
/// REQ-AUDIO-001: CLAP プラグインのホスティング
pub struct PluginAudioProcessor {
    audio_processor: StartedPluginAudioProcessor<CplpHost>,
    /// 非インターリーブ出力バッファ
    output_buf: Vec<f32>,
    /// 非インターリーブ入力バッファ
    input_buf: Vec<f32>,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    channels: usize,
    steady_counter: u64,
    /// ノートイベント受信端（lock-free ringbuf）
    note_recv: NoteReceiver,
    /// CLAP イベントバッファ（再利用）
    event_buf: EventBuffer,
}

// StartedPluginAudioProcessor は Send なので PluginAudioProcessor も Send
unsafe impl Send for PluginAudioProcessor {}

impl PluginAudioProcessor {
    /// シンセ用: 入力なしでオーディオを生成（インターリーブ f32 出力）
    ///
    /// NoteController 経由のノートイベントを自動的にプラグインに送信する。
    pub fn process(&mut self, output: &mut [f32]) {
        let frame_count = output.len() / self.channels;
        let total_samples = frame_count * self.channels;

        self.ensure_buf_size(total_samples);
        self.output_buf[..total_samples].fill(0.0);
        self.input_buf[..total_samples].fill(0.0);

        // ringbuf から全イベントを drain
        self.event_buf.clear();
        self.note_recv.drain_to_event_buffer(&mut self.event_buf);

        self.do_process(total_samples, frame_count, output);
    }

    /// エフェクト用: インターリーブ入力を受け取り、加工して出力
    pub fn process_effect(&mut self, input: &[f32], output: &mut [f32]) {
        let frame_count = output.len() / self.channels;
        let total_samples = frame_count * self.channels;

        self.ensure_buf_size(total_samples);
        self.output_buf[..total_samples].fill(0.0);

        // インターリーブ → 非インターリーブ変換（入力）
        interleaved_to_deinterleaved(
            input,
            &mut self.input_buf[..total_samples],
            self.channels,
            frame_count,
        );

        self.event_buf.clear();
        self.note_recv.drain_to_event_buffer(&mut self.event_buf);

        self.do_process(total_samples, frame_count, output);
    }

    fn ensure_buf_size(&mut self, total_samples: usize) {
        if self.output_buf.len() < total_samples {
            self.output_buf.resize(total_samples, 0.0);
            self.input_buf.resize(total_samples, 0.0);
        }
    }

    fn do_process(
        &mut self,
        total_samples: usize,
        frame_count: usize,
        output: &mut [f32],
    ) {
        let channels = self.channels;

        let ins = self.input_ports.with_input_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                self.input_buf[..total_samples]
                    .chunks_exact_mut(frame_count)
                    .map(|buffer| InputChannel {
                        buffer,
                        is_constant: false,
                    }),
            ),
        }]);

        let mut outs = self.output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(
                self.output_buf[..total_samples]
                    .chunks_exact_mut(frame_count)
                    .map(|buf| &mut *buf),
            ),
        }]);

        let input_events = self.event_buf.as_input();

        match self.audio_processor.process(
            &ins,
            &mut outs,
            &input_events,
            &mut OutputEvents::void(),
            Some(self.steady_counter),
            None,
        ) {
            Ok(_) => {
                deinterleave_to_interleaved(
                    &self.output_buf[..total_samples],
                    output,
                    channels,
                    frame_count,
                );
            }
            Err(e) => {
                error!("Plugin process error: {e}");
                output.fill(0.0);
            }
        }

        self.steady_counter += frame_count as u64;
    }
}

/// 非インターリーブ [L0,L1,...,R0,R1,...] → インターリーブ [L0,R0,L1,R1,...] 変換
fn deinterleave_to_interleaved(
    deinterleaved: &[f32],
    interleaved: &mut [f32],
    channels: usize,
    frame_count: usize,
) {
    for frame in 0..frame_count {
        for ch in 0..channels {
            interleaved[frame * channels + ch] = deinterleaved[ch * frame_count + frame];
        }
    }
}

/// インターリーブ [L0,R0,L1,R1,...] → 非インターリーブ [L0,L1,...,R0,R1,...] 変換
fn interleaved_to_deinterleaved(
    interleaved: &[f32],
    deinterleaved: &mut [f32],
    channels: usize,
    frame_count: usize,
) {
    for frame in 0..frame_count {
        for ch in 0..channels {
            deinterleaved[ch * frame_count + frame] = interleaved[frame * channels + ch];
        }
    }
}

// ─── プラグインハンドル ──────────────────────────────────────

/// プラグインインスタンスのハンドル（メインスレッドで保持）
///
/// GUI 操作やメインスレッドコールバックの処理に使用する。
/// Drop 時にプラグインが自動的にクリーンアップされる。
pub struct PluginHandle {
    instance: PluginInstance<CplpHost>,
    receiver: mpsc::Receiver<HostMessage>,
}

impl PluginHandle {
    /// プラグインGUIを表示してイベントループを実行（メインスレッドをブロック）
    ///
    /// winit のウィンドウにプラグインGUIを埋め込み、ウィンドウが閉じられるまでブロックする。
    /// オーディオ処理は cpal スレッドで並行動作する。
    pub fn run_gui(&mut self) -> Result<()> {
        let event_loop = EventLoop::new()
            .map_err(|e| anyhow::anyhow!("EventLoop 作成に失敗: {e}"))?;

        let mut app = PluginGuiApp {
            plugin_handle: self,
            window: None,
            gui_created: false,
        };

        event_loop
            .run_app(&mut app)
            .map_err(|e| anyhow::anyhow!("GUI イベントループエラー: {e}"))?;

        Ok(())
    }

    /// メインスレッドのイベントを処理する（非ブロッキング）
    ///
    /// GUIが閉じられた場合は false を返す。
    pub fn process_gui_events(&mut self) -> bool {
        while let Ok(msg) = self.receiver.try_recv() {
            match msg {
                HostMessage::RequestCallback => {
                    self.instance.call_on_main_thread_callback();
                }
                HostMessage::GuiClosed => {
                    info!("Plugin GUI closed");
                    return false;
                }
            }
        }
        true
    }
}

// ─── winit GUI アプリケーション ──────────────────────────────

/// winit EventLoop と CLAP プラグイン GUI を統合するアプリケーション
struct PluginGuiApp<'a> {
    plugin_handle: &'a mut PluginHandle,
    window: Option<WinitWindow>,
    gui_created: bool,
}

impl ApplicationHandler for PluginGuiApp<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        if let Err(e) = self.setup_gui(event_loop) {
            error!("GUI セットアップ失敗: {e}");
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // プラグインからのコールバックを処理
        if !self.plugin_handle.process_gui_events() {
            self.cleanup_gui();
            event_loop.exit();
            return;
        }

        // ~60fps でポーリング（プラグインコールバック処理用）
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.cleanup_gui();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(window) = &self.window {
                    let mut ph = self.plugin_handle.instance.plugin_handle();
                    if let Some(gui) = ph.get_extension::<PluginGui>() {
                        // macOS Cocoa は logical pixels を使用
                        let logical = size.to_logical::<u32>(window.scale_factor());
                        let gui_size = GuiSize {
                            width: logical.width,
                            height: logical.height,
                        };
                        if let Some(adjusted) = gui.adjust_size(&mut ph, gui_size) {
                            let _ = gui.set_size(&mut ph, adjusted);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl PluginGuiApp<'_> {
    /// GUI のセットアップ: プラグインGUI作成 → ウィンドウ作成 → 埋め込み → 表示
    fn setup_gui(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let mut ph = self.plugin_handle.instance.plugin_handle();

        let gui = ph
            .get_extension::<PluginGui>()
            .context("プラグインが GUI 拡張をサポートしていません")?;

        let api = GuiApiType::default_for_current_platform()
            .context("このプラットフォームでは GUI がサポートされていません")?;

        // サポート状況を確認
        let floating = gui.is_api_supported(
            &mut ph,
            GuiConfiguration { api_type: api, is_floating: true },
        );
        let embedded = gui.is_api_supported(
            &mut ph,
            GuiConfiguration { api_type: api, is_floating: false },
        );
        info!("GUI support: floating={floating}, embedded={embedded}");

        let config = if embedded {
            GuiConfiguration { api_type: api, is_floating: false }
        } else if floating {
            GuiConfiguration { api_type: api, is_floating: true }
        } else {
            anyhow::bail!("プラグインが GUI をサポートしていません");
        };

        // プラグインGUIリソースを作成
        gui.create(&mut ph, config)
            .map_err(|e| anyhow::anyhow!("GUI 作成に失敗: {e:?}"))?;
        self.gui_created = true;

        // 初期サイズを取得
        let size = gui
            .get_size(&mut ph)
            .unwrap_or(GuiSize { width: 800, height: 600 });
        let resizable = gui.can_resize(&mut ph);

        info!("GUI size: {}x{}, resizable={resizable}", size.width, size.height);

        // winit ウィンドウを作成
        let attrs = WinitWindow::default_attributes()
            .with_title("cplp-sound-system")
            .with_inner_size(LogicalSize::new(size.width, size.height))
            .with_resizable(resizable);

        let window = event_loop
            .create_window(attrs)
            .map_err(|e| anyhow::anyhow!("ウィンドウ作成に失敗: {e}"))?;

        if !config.is_floating {
            // Embedded モード: winit ウィンドウの NSView にプラグインGUIを埋め込む
            let clap_window = ClapWindow::from_window(&window)
                .context("CLAP ウィンドウハンドルの取得に失敗")?;

            unsafe {
                gui.set_parent(&mut ph, clap_window)
                    .map_err(|e| anyhow::anyhow!("set_parent に失敗: {e:?}"))?;
            }
        } else {
            // Floating モード: タイトルだけ設定
            let title = CString::new("cplp-sound-system").unwrap();
            gui.suggest_title(&mut ph, &title);
        }

        gui.show(&mut ph)
            .map_err(|e| anyhow::anyhow!("GUI 表示に失敗: {e:?}"))?;

        let mode = if config.is_floating { "floating" } else { "embedded" };
        info!("Plugin GUI opened ({mode} mode)");
        println!("プラグイン GUI を表示中... (ウィンドウを閉じると停止)");

        self.window = Some(window);
        Ok(())
    }

    fn cleanup_gui(&mut self) {
        if self.gui_created {
            let mut ph = self.plugin_handle.instance.plugin_handle();
            if let Some(gui) = ph.get_extension::<PluginGui>() {
                gui.destroy(&mut ph);
            }
            self.gui_created = false;
            info!("Plugin GUI destroyed");
        }
    }
}

impl Drop for PluginGuiApp<'_> {
    fn drop(&mut self) {
        self.cleanup_gui();
    }
}

// ─── プラグインローダー ──────────────────────────────────────

/// CLAPプラグインをロードしてオーディオプロセッサを生成
///
/// # 戻り値
/// - `PluginAudioProcessor` — オーディオスレッドで `process()` / `process_effect()` を呼ぶ
/// - `NoteController` — ノートイベントを送信するコントローラ
/// - `PluginHandle` — メインスレッドで保持（drop でクリーンアップ）
pub fn load_plugin(
    plugin_info: &PluginInfo,
    sample_rate: f64,
    min_frames: u32,
    max_frames: u32,
    channels: usize,
) -> Result<(PluginAudioProcessor, NoteController, PluginHandle)> {
    let host = host_info();
    let plugin_id = CString::new(plugin_info.id.as_str())
        .context("Invalid plugin ID")?;

    let bundle = unsafe {
        PluginBundle::load(&plugin_info.bundle_path)
            .context("Failed to load plugin bundle")?
    };

    let (sender, receiver) = mpsc::channel();

    let mut instance = PluginInstance::<CplpHost>::new(
        |_| CplpHostShared {
            sender: sender.clone(),
        },
        |_shared| CplpHostMainThread,
        &bundle,
        &plugin_id,
        &host,
    )
    .context("Failed to instantiate plugin")?;

    info!("Plugin instantiated: {}", plugin_info.name);

    let audio_config = PluginAudioConfiguration {
        sample_rate,
        min_frames_count: min_frames,
        max_frames_count: max_frames,
    };

    let stopped = instance
        .activate(|_, _| (), audio_config)
        .context("Failed to activate plugin")?;

    let started = stopped
        .start_processing()
        .map_err(|e| anyhow::anyhow!("Failed to start processing: {:?}", e))?;

    info!("Plugin activated: {}Hz, {}-{} frames", sample_rate, min_frames, max_frames);

    let buf_size = max_frames as usize * channels;
    // 256 イベントキュー: 和音・高速連打に十分な容量
    let (note_ctrl, note_recv) = note_channel(256);

    let processor = PluginAudioProcessor {
        audio_processor: started,
        output_buf: vec![0.0; buf_size],
        input_buf: vec![0.0; buf_size],
        input_ports: AudioPorts::with_capacity(channels, 1),
        output_ports: AudioPorts::with_capacity(channels, 1),
        channels,
        steady_counter: 0,
        note_recv,
        event_buf: EventBuffer::with_capacity(64),
    };

    let handle = PluginHandle {
        instance,
        receiver,
    };

    Ok((processor, note_ctrl, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deinterleave_stereo() {
        let deinterleaved = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut interleaved = [0.0f32; 6];
        deinterleave_to_interleaved(&deinterleaved, &mut interleaved, 2, 3);
        assert_eq!(interleaved, [1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn deinterleave_mono() {
        let deinterleaved = [1.0, 2.0, 3.0];
        let mut interleaved = [0.0f32; 3];
        deinterleave_to_interleaved(&deinterleaved, &mut interleaved, 1, 3);
        assert_eq!(interleaved, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn interleave_roundtrip() {
        let original_interleaved = [1.0, 4.0, 2.0, 5.0, 3.0, 6.0]; // L,R,L,R,L,R
        let mut deinterleaved = [0.0f32; 6];
        let mut result = [0.0f32; 6];

        interleaved_to_deinterleaved(&original_interleaved, &mut deinterleaved, 2, 3);
        deinterleave_to_interleaved(&deinterleaved, &mut result, 2, 3);

        assert_eq!(result, original_interleaved);
    }

    #[test]
    fn note_channel_multiple_events() {
        let (mut ctrl, mut recv) = note_channel(32);

        // 和音: 3ノート同時
        ctrl.note_on(60, 100);
        ctrl.note_on(64, 80);
        ctrl.note_on(67, 90);

        let mut event_buf = EventBuffer::with_capacity(16);
        recv.drain_to_event_buffer(&mut event_buf);

        assert_eq!(event_buf.as_input().len(), 3);
    }

    #[test]
    fn note_channel_velocity_zero_is_note_off() {
        let (mut ctrl, mut recv) = note_channel(32);

        ctrl.note_on(60, 0); // velocity 0 = NoteOff

        let mut event_buf = EventBuffer::with_capacity(16);
        recv.drain_to_event_buffer(&mut event_buf);

        // NoteOff として処理される
        assert_eq!(event_buf.as_input().len(), 1);
    }
}

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::text::TextEntry;
use crate::renderer::Renderer;
use crate::state::{AudioMeters, SessionSnapshot};
use crate::ui::button::Button;
use crate::ui::event::{from_window_event, EventResponse, UiEvent};
use crate::ui::list::List;
use crate::ui::slider::Slider;
use crate::ui::text_input::TextInput;
use crate::ui::widget::Widget;
use crate::visuals::connection::ConnectionIndicator;
use crate::visuals::meters::LevelMeter;
use crate::visuals::spectrum::Spectrum;
use crate::visuals::waveform::Waveform;
use crate::HudContext;

// ── 画面遷移アクション ──────────────────────────────

pub enum ScreenAction {
    None,
    GoToConnecting,
    GoToLive,
    GoToSetup,
}

// ── Screen 列挙型 ────────────────────────────────────

pub enum Screen {
    Setup(Box<SetupScreen>),
    Connecting(ConnectingScreen),
    Live(Box<LiveScreen>),
}

// ── レイアウト定数 ───────────────────────────────────

const PADDING: f32 = 20.0;
const HALF_W: f32 = 300.0;
const CONTENT_Y: f32 = 70.0;

// ── SetupScreen ──────────────────────────────────────

pub struct SetupScreen {
    plugin_list: List,
    midi_list: List,
    session_input: TextInput,
    create_btn: Button,
    join_btn: Button,
}

impl Default for SetupScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl SetupScreen {
    pub fn new() -> Self {
        let mut plugin_list = List::new(4);
        plugin_list.set_items(vec![
            "Diva".into(),
            "Surge XT".into(),
            "Vital".into(),
            "ZynAddSubFX".into(),
        ]);

        let mut midi_list = List::new(2);
        midi_list.set_items(vec![
            "MIDI Keyboard 1".into(),
            "USB MIDI".into(),
        ]);

        Self {
            plugin_list,
            midi_list,
            session_input: TextInput::new("Session ID..."),
            create_btn: Button::new("Create Session"),
            join_btn: Button::new("Join Session"),
        }
    }

    /// 各ウィジェットの配置 Rect を返すヘルパー
    fn layout() -> SetupLayout {
        let list_w = HALF_W - PADDING * 2.0;
        let right_x = HALF_W + PADDING;
        let midi_y = CONTENT_Y + 160.0;

        SetupLayout {
            plugin_list: Rect { x: PADDING, y: CONTENT_Y + 24.0, w: list_w, h: 120.0 },
            midi_list: Rect { x: PADDING, y: midi_y + 24.0, w: list_w, h: 60.0 },
            session_input: Rect { x: right_x, y: CONTENT_Y + 24.0, w: list_w, h: 32.0 },
            create_btn: Rect { x: right_x, y: CONTENT_Y + 68.0, w: list_w, h: 36.0 },
            join_btn: Rect { x: right_x, y: CONTENT_Y + 116.0, w: list_w, h: 36.0 },
            midi_label_y: midi_y,
            right_x,
        }
    }

    pub fn draw(&self, renderer: &mut Renderer) {
        let l = Self::layout();

        // タイトル
        renderer.text(TextEntry {
            text: "cplp".into(),
            x: PADDING,
            y: PADDING,
            size: 32.0,
            color: [0.2, 0.6, 0.9, 1.0],
        });

        // ── 左半分: Plugins ──
        renderer.text(TextEntry {
            text: "Plugins".into(),
            x: PADDING,
            y: CONTENT_Y,
            size: 16.0,
            color: [0.7, 0.7, 0.7, 1.0],
        });
        self.plugin_list.draw(renderer, l.plugin_list);

        // ── 左半分: MIDI Devices ──
        renderer.text(TextEntry {
            text: "MIDI Devices".into(),
            x: PADDING,
            y: l.midi_label_y,
            size: 16.0,
            color: [0.7, 0.7, 0.7, 1.0],
        });
        self.midi_list.draw(renderer, l.midi_list);

        // ── 右半分: Session ──
        renderer.text(TextEntry {
            text: "Session".into(),
            x: l.right_x,
            y: CONTENT_Y,
            size: 16.0,
            color: [0.7, 0.7, 0.7, 1.0],
        });
        self.session_input.draw(renderer, l.session_input);
        self.create_btn.draw(renderer, l.create_btn);
        self.join_btn.draw(renderer, l.join_btn);
    }

    pub fn event(&mut self, event: &UiEvent) -> ScreenAction {
        let l = Self::layout();

        // 全ウィジェットにイベント委譲
        self.plugin_list.event(event, l.plugin_list);
        self.midi_list.event(event, l.midi_list);
        self.session_input.event(event, l.session_input);

        let create_resp = self.create_btn.event(event, l.create_btn);
        let join_resp = self.join_btn.event(event, l.join_btn);

        // MouseUp で Consumed = ボタンクリック完了 → 画面遷移
        if matches!(event, UiEvent::MouseUp(_, _))
            && (create_resp == EventResponse::Consumed || join_resp == EventResponse::Consumed)
        {
            return ScreenAction::GoToConnecting;
        }

        ScreenAction::None
    }
}

/// SetupScreen のレイアウト情報
struct SetupLayout {
    plugin_list: Rect,
    midi_list: Rect,
    session_input: Rect,
    create_btn: Rect,
    join_btn: Rect,
    midi_label_y: f32,
    right_x: f32,
}

// ── ConnectingScreen ─────────────────────────────────

pub struct ConnectingScreen {
    session_id: String,
    cancel_btn: Button,
    elapsed_frames: u64,
}

impl ConnectingScreen {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            cancel_btn: Button::new("Cancel"),
            elapsed_frames: 0,
        }
    }

    pub fn update(&mut self) {
        self.elapsed_frames += 1;
    }

    pub fn should_transition(&self) -> bool {
        self.elapsed_frames >= 180
    }

    fn cancel_btn_rect() -> Rect {
        Rect { x: 260.0, y: 280.0, w: 120.0, h: 36.0 }
    }

    pub fn draw(&self, renderer: &mut Renderer) {
        // "Connecting" + 点滅ドット
        let dots = if self.elapsed_frames % 60 < 30 { "..." } else { "" };
        renderer.text(TextEntry {
            text: format!("Connecting{}", dots),
            x: 240.0,
            y: 200.0,
            size: 24.0,
            color: [0.2, 0.6, 0.9, 1.0],
        });

        // セッション ID
        renderer.text(TextEntry {
            text: format!("Session: {}", self.session_id),
            x: 240.0,
            y: 240.0,
            size: 14.0,
            color: [0.6, 0.6, 0.6, 1.0],
        });

        // Cancel ボタン
        self.cancel_btn.draw(renderer, Self::cancel_btn_rect());
    }

    pub fn event(&mut self, event: &UiEvent) -> ScreenAction {
        let resp = self.cancel_btn.event(event, Self::cancel_btn_rect());

        if matches!(event, UiEvent::MouseUp(_, _)) && resp == EventResponse::Consumed {
            return ScreenAction::GoToSetup;
        }

        ScreenAction::None
    }
}

// ── LiveScreen ───────────────────────────────────────

pub struct LiveScreen {
    connection: ConnectionIndicator,
    local_meter: LevelMeter,
    remote_meter: LevelMeter,
    local_waveform: Waveform,
    remote_waveform: Waveform,
    local_spectrum: Spectrum,
    mix_slider: Slider,
}

impl Default for LiveScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveScreen {
    pub fn new() -> Self {
        Self {
            connection: ConnectionIndicator::new(),
            local_meter: LevelMeter::new("You"),
            remote_meter: LevelMeter::new("Peer"),
            local_waveform: Waveform::new(
                "You",
                Color { r: 0.0, g: 0.9, b: 0.9, a: 1.0 }, // シアン
            ),
            remote_waveform: Waveform::new(
                "Peer",
                Color { r: 0.9, g: 0.2, b: 0.9, a: 1.0 }, // マゼンタ
            ),
            local_spectrum: Spectrum::new(
                Color { r: 0.0, g: 0.8, b: 0.9, a: 1.0 }, // シアン系
            ),
            mix_slider: Slider::new("Mix Balance"),
        }
    }

    /// ウィンドウサイズに応じたレスポンシブレイアウト
    fn layout(w: f32, h: f32) -> LiveLayout {
        let pad = 10.0;
        let half_w = (w - pad * 3.0) / 2.0;
        let section_y = 50.0;
        let right_x = half_w + pad * 2.0;
        // ウェーブフォーム高さ: 残りスペースから接続バー・メーター・スペクトラム・スライダーを引く
        let waveform_h = (h - section_y - 24.0 - 28.0 - 100.0 - pad * 4.0 - 36.0).max(60.0);
        let spectrum_y = section_y + 24.0 + waveform_h + pad;
        let spectrum_h = 80.0f32.min((h - spectrum_y - 28.0 - pad * 2.0 - 20.0).max(40.0));
        let bottom_y = spectrum_y + spectrum_h + pad;

        LiveLayout {
            connection: Rect { x: pad, y: pad, w: w - pad * 2.0, h: 30.0 },
            local_meter: Rect { x: pad, y: section_y, w: half_w, h: 20.0 },
            local_waveform: Rect { x: pad, y: section_y + 24.0, w: half_w, h: waveform_h },
            remote_meter: Rect { x: right_x, y: section_y, w: half_w, h: 20.0 },
            remote_waveform: Rect { x: right_x, y: section_y + 24.0, w: half_w, h: waveform_h },
            local_spectrum: Rect { x: pad, y: spectrum_y, w: w - pad * 2.0, h: spectrum_h },
            mix_slider: Rect { x: pad, y: bottom_y, w: w - pad * 2.0, h: 28.0 },
            pad,
            bottom_y,
        }
    }

    /// デモモード: ダミーデータで更新
    pub fn update_demo(&mut self, frame_count: u64) {
        let local_level = (frame_count as f32 * 0.03).sin().abs() * 0.8 + 0.1;
        let remote_level = (frame_count as f32 * 0.025 + 1.0).sin().abs() * 0.7 + 0.15;
        self.local_meter.update(local_level);
        self.remote_meter.update(remote_level);

        let local_samples: Vec<f32> = (0..1024)
            .map(|i| (i as f32 * 0.05 + frame_count as f32 * 0.02).sin() * 0.6)
            .collect();
        let remote_samples: Vec<f32> = (0..256)
            .map(|i| (i as f32 * 0.07 + frame_count as f32 * 0.015).sin() * 0.5)
            .collect();
        self.local_waveform.update(&local_samples);
        self.remote_waveform.update(&remote_samples);
        self.local_spectrum.update(&local_samples);

        self.connection.update(&SessionSnapshot {
            peer_name: "Player B".into(),
            connected: true,
            latency_ms: 8.5,
            jitter_ms: 1.2,
            ..Default::default()
        });
    }

    /// リアルデータモード: AudioMeters + SessionSnapshot + PCM から更新
    pub fn update_live(
        &mut self,
        meters: &AudioMeters,
        snapshot: &SessionSnapshot,
        local_pcm: &[f32],
    ) {
        let local_level = meters.local_level.load(Relaxed);
        let remote_level = meters.remote_level.load(Relaxed);
        self.local_meter.update(local_level);
        self.remote_meter.update(remote_level);

        self.local_waveform.update(local_pcm);
        self.local_spectrum.update(local_pcm);

        // リモート PCM は未接続（次フェーズ: ネットワーク受信 PCM）
        let remote_samples: Vec<f32> = (0..256)
            .map(|i| (i as f32 * 0.07).sin() * remote_level)
            .collect();
        self.remote_waveform.update(&remote_samples);

        self.connection.update(snapshot);
    }

    pub fn draw(&self, renderer: &mut Renderer, w: f32, h: f32) {
        let l = Self::layout(w, h);

        // 上部: 接続情報バー
        self.connection.draw(renderer, l.connection);

        // 中央左: "You" + メーター + ウェーブフォーム
        self.local_meter.draw(renderer, l.local_meter);
        self.local_waveform.draw(renderer, l.local_waveform);

        // 中央右: "Peer" + メーター + ウェーブフォーム
        self.remote_meter.draw(renderer, l.remote_meter);
        self.remote_waveform.draw(renderer, l.remote_waveform);

        // スペクトラム
        self.local_spectrum.draw(renderer, l.local_spectrum);

        // 下部: ミックススライダー + ステータステキスト
        self.mix_slider.draw(renderer, l.mix_slider);
        renderer.text(TextEntry {
            text: "Session Active".into(),
            x: l.pad,
            y: l.bottom_y + 36.0,
            size: 14.0,
            color: [0.2, 0.9, 0.4, 1.0],
        });
    }

    pub fn event(&mut self, event: &UiEvent, w: f32, h: f32) -> ScreenAction {
        let l = Self::layout(w, h);
        self.mix_slider.event(event, l.mix_slider);
        ScreenAction::None
    }
}

/// LiveScreen のレイアウト情報
struct LiveLayout {
    connection: Rect,
    local_meter: Rect,
    local_waveform: Rect,
    remote_meter: Rect,
    remote_waveform: Rect,
    local_spectrum: Rect,
    mix_slider: Rect,
    pad: f32,
    bottom_y: f32,
}

// ── App 構造体 ───────────────────────────────────────

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    screen: Screen,
    cursor_pos: Vec2,
    frame_count: u64,
    ctx: Option<HudContext>,
}

impl App {
    fn new(ctx: Option<HudContext>) -> Self {
        Self {
            window: None,
            renderer: None,
            screen: Screen::Setup(Box::default()),
            cursor_pos: Vec2 { x: 0.0, y: 0.0 },
            frame_count: 0,
            ctx,
        }
    }

    /// 画面遷移を適用
    fn apply_action(&mut self, action: ScreenAction) {
        match action {
            ScreenAction::GoToConnecting => {
                self.screen = Screen::Connecting(ConnectingScreen::new("DEMO-SESSION"));
            }
            ScreenAction::GoToLive => {
                self.screen = Screen::Live(Box::default());
            }
            ScreenAction::GoToSetup => {
                self.screen = Screen::Setup(Box::default());
            }
            ScreenAction::None => {}
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("cplp")
            .with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("failed to create window"));
        let renderer =
            Renderer::new(window.clone()).expect("failed to initialize renderer");
        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                }
            }
            WindowEvent::RedrawRequested => {
                if self.renderer.is_none() {
                    return;
                }
                let renderer = self.renderer.as_mut().unwrap();
                let gpu_size = renderer.gpu().size;
                let (w, h) = (gpu_size.width as f32, gpu_size.height as f32);

                // グロー設定: Live 画面のみ有効
                renderer.set_glow_enabled(matches!(self.screen, Screen::Live(_)));

                // 画面ごとの更新・描画
                let action = match &mut self.screen {
                    Screen::Setup(s) => {
                        s.draw(renderer);
                        ScreenAction::None // イベント側で遷移を処理
                    }
                    Screen::Connecting(s) => {
                        s.update();
                        s.draw(renderer);
                        if s.should_transition() {
                            ScreenAction::GoToLive
                        } else {
                            ScreenAction::None
                        }
                    }
                    Screen::Live(s) => {
                        if let Some(ctx) = &mut self.ctx {
                            let snap = ctx.session.read().clone();
                            let pcm = ctx.local_pcm.read().clone();
                            s.update_live(&ctx.meters, &snap, &pcm.samples);
                        } else {
                            s.update_demo(self.frame_count);
                        }
                        s.draw(renderer, w, h);
                        ScreenAction::None
                    }
                };

                self.frame_count += 1;
                self.renderer.as_mut().unwrap().render_frame();

                // render_frame 後に画面遷移（次フレームから反映）
                self.apply_action(action);
            }
            other => {
                if let Some(ui_event) = from_window_event(&other) {
                    // CursorMoved は cursor_pos も更新
                    if let UiEvent::MouseMove(pos) = &ui_event {
                        self.cursor_pos = *pos;
                    }

                    // MouseDown/MouseUp の座標を cursor_pos で補完
                    let ui_event = match ui_event {
                        UiEvent::MouseDown(_, btn) => UiEvent::MouseDown(self.cursor_pos, btn),
                        UiEvent::MouseUp(_, btn) => UiEvent::MouseUp(self.cursor_pos, btn),
                        e => e,
                    };

                    let (w, h) = self.renderer.as_ref()
                        .map(|r| {
                            let s = r.gpu().size;
                            (s.width as f32, s.height as f32)
                        })
                        .unwrap_or((640.0, 480.0));

                    let action = match &mut self.screen {
                        Screen::Setup(s) => s.event(&ui_event),
                        Screen::Connecting(s) => s.event(&ui_event),
                        Screen::Live(s) => s.event(&ui_event, w, h),
                    };

                    self.apply_action(action);
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
            // ~60fps でリドローを要求（CPU 負荷軽減）
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(16),
            ));
        }
    }
}

/// デモモードで HUD を起動（外部データなし）
pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App::new(None);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// リアルデータモードで HUD を起動
pub fn run_with_context(ctx: HudContext) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App::new(Some(ctx));
    event_loop.run_app(&mut app)?;
    Ok(())
}

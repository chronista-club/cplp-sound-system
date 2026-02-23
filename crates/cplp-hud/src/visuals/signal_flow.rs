use cplp_core::{AudioEdge, AudioGraphState, AudioNode, AudioNodeKind, NodeActivity};

use crate::renderer::Renderer;
use crate::renderer::primitives::{Color, Rect, Vec2};
use crate::renderer::text::TextEntry;
use crate::state::{AudioMeters, SessionSnapshot};
use crate::ui::theme;

use std::sync::atomic::Ordering::Relaxed;

/// ノード描画用の矩形位置
struct NodeRect {
    rect: Rect,
}

/// SignalFlowGraph — 信号フローをノード＋エッジで描画するウィジェット
pub struct SignalFlowGraph {
    graph: AudioGraphState,
    node_rects: Vec<NodeRect>,
}

impl Default for SignalFlowGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalFlowGraph {
    pub fn new() -> Self {
        Self {
            graph: AudioGraphState::default(),
            node_rects: Vec::new(),
        }
    }

    /// FluxSnapshot + AudioMeters + SessionSnapshot → AudioGraphState を構築
    pub fn build_graph(
        &mut self,
        flux: &cplp_flux::FluxSnapshot,
        meters: Option<&AudioMeters>,
        session: &SessionSnapshot,
    ) {
        let local_level = meters.map_or(0.3, |m| m.local_level.load(Relaxed));
        let remote_level = meters.map_or(0.0, |m| m.remote_level.load(Relaxed));

        let module_activity = |state: cplp_flux::ModuleState| -> NodeActivity {
            match state {
                cplp_flux::ModuleState::Off => NodeActivity::Inactive,
                cplp_flux::ModuleState::Ready | cplp_flux::ModuleState::Playing => {
                    NodeActivity::Active
                }
            }
        };

        let synth_active = flux.synth_state == cplp_flux::ModuleState::Playing;
        let beat_active = flux.beat_machine_state == cplp_flux::ModuleState::Playing;
        let looper_active = matches!(
            flux.looper_state,
            cplp_flux::LooperState::Recording | cplp_flux::LooperState::Playing
        );

        let plugin_name = flux.active_plugin.clone().unwrap_or_default();

        // ── ノード構築 ──
        // インデックスを固定で管理（レイアウトと対応）
        //  0: MIDI In
        //  1: Synth
        //  2: BeatMachine
        //  3: Looper
        //  4: Mixer
        //  5: AudioOutput
        //  6: NetworkRecv
        //  7: NetworkSend
        let nodes = vec![
            AudioNode {
                kind: AudioNodeKind::MidiInput,
                activity: if synth_active || beat_active {
                    NodeActivity::Active
                } else {
                    NodeActivity::Inactive
                },
                level: if synth_active { local_level } else { 0.0 },
            },
            AudioNode {
                kind: AudioNodeKind::Synth { plugin_name },
                activity: module_activity(flux.synth_state),
                level: if synth_active { local_level } else { 0.0 },
            },
            AudioNode {
                kind: AudioNodeKind::BeatMachine,
                activity: module_activity(flux.beat_machine_state),
                level: if beat_active { local_level * 0.7 } else { 0.0 },
            },
            AudioNode {
                kind: AudioNodeKind::Looper,
                activity: if looper_active {
                    NodeActivity::Active
                } else {
                    NodeActivity::Inactive
                },
                level: if looper_active {
                    local_level * 0.5
                } else {
                    0.0
                },
            },
            AudioNode {
                kind: AudioNodeKind::Mixer,
                activity: NodeActivity::Active,
                level: local_level,
            },
            AudioNode {
                kind: AudioNodeKind::AudioOutput,
                activity: NodeActivity::Active,
                level: local_level,
            },
            AudioNode {
                kind: AudioNodeKind::NetworkRecv,
                activity: if session.connected {
                    NodeActivity::Active
                } else {
                    NodeActivity::Inactive
                },
                level: remote_level,
            },
            AudioNode {
                kind: AudioNodeKind::NetworkSend,
                activity: if session.connected {
                    NodeActivity::Active
                } else {
                    NodeActivity::Inactive
                },
                level: local_level,
            },
        ];

        // ── エッジ構築 ──
        let mut edges = vec![
            // MIDI In → Synth
            AudioEdge {
                from: 0,
                to: 1,
                level: if synth_active { local_level } else { 0.0 },
            },
            // Synth → Mixer
            AudioEdge {
                from: 1,
                to: 4,
                level: if synth_active { local_level } else { 0.0 },
            },
            // BeatMachine → Mixer
            AudioEdge {
                from: 2,
                to: 4,
                level: if beat_active { local_level * 0.7 } else { 0.0 },
            },
            // Looper → Mixer
            AudioEdge {
                from: 3,
                to: 4,
                level: if looper_active {
                    local_level * 0.5
                } else {
                    0.0
                },
            },
            // Mixer → AudioOutput
            AudioEdge {
                from: 4,
                to: 5,
                level: local_level,
            },
            // Mixer → NetworkSend
            AudioEdge {
                from: 4,
                to: 7,
                level: if session.connected {
                    local_level
                } else {
                    0.0
                },
            },
        ];

        // NetworkRecv → Mixer（接続中のみ）
        if session.connected {
            edges.push(AudioEdge {
                from: 6,
                to: 4,
                level: remote_level,
            });
        }

        self.graph = AudioGraphState { nodes, edges };
    }

    /// ノード座標をレイアウトする。
    ///
    /// 3 列構成: [入力] → [処理] → [出力]
    /// ユーザーが調整可能なパラメータ: node_w, node_h, col_gap, row_gap, pad
    pub fn layout_nodes(&mut self, w: f32, h: f32) {
        // TODO: ここがユーザー実装箇所です（後述の Learning Mode セクション参照）
        // 以下はデフォルト実装です。
        layout_nodes_default(&self.graph, w, h, &mut self.node_rects);
    }

    /// ノードとエッジを描画
    pub fn draw(&self, renderer: &mut Renderer) {
        if self.graph.nodes.is_empty() || self.node_rects.is_empty() {
            return;
        }

        // ── エッジ描画 ──
        for edge in &self.graph.edges {
            if edge.from >= self.node_rects.len() || edge.to >= self.node_rects.len() {
                continue;
            }
            let from_r = &self.node_rects[edge.from].rect;
            let to_r = &self.node_rects[edge.to].rect;

            let from_pt = Vec2 {
                x: from_r.x + from_r.w,
                y: from_r.y + from_r.h / 2.0,
            };
            let to_pt = Vec2 {
                x: to_r.x,
                y: to_r.y + to_r.h / 2.0,
            };

            // 信号レベルに応じてシアン系の明るさを変化
            let brightness = 0.2 + edge.level * 0.8;
            let edge_color = Color {
                r: 0.0,
                g: brightness * 0.8,
                b: brightness,
                a: 0.8,
            };

            // 直線が横に遠い場合はベジエ的にポリラインで中継
            let mid_x = (from_pt.x + to_pt.x) / 2.0;
            renderer.polyline(
                &[
                    from_pt,
                    Vec2 {
                        x: mid_x,
                        y: from_pt.y,
                    },
                    Vec2 {
                        x: mid_x,
                        y: to_pt.y,
                    },
                    to_pt,
                ],
                edge_color,
            );
        }

        // ── ノード描画 ──
        for (i, node) in self.graph.nodes.iter().enumerate() {
            if i >= self.node_rects.len() {
                break;
            }
            let nr = &self.node_rects[i];
            let r = nr.rect;

            // ノード背景
            let bg = Color {
                r: 0.12,
                g: 0.12,
                b: 0.18,
                a: 0.9,
            };
            renderer.rect(r, bg);

            // ボーダー（状態に応じた色）
            let border_color = match node.activity {
                NodeActivity::Active => Color {
                    r: 0.2,
                    g: 0.8,
                    b: 0.4,
                    a: 0.9,
                },
                NodeActivity::Inactive => Color {
                    r: 0.4,
                    g: 0.4,
                    b: 0.4,
                    a: 0.6,
                },
                NodeActivity::Error => Color {
                    r: 0.9,
                    g: 0.2,
                    b: 0.2,
                    a: 0.9,
                },
            };
            let bw = 2.0; // border width
            // Top
            renderer.rect(
                Rect {
                    x: r.x,
                    y: r.y,
                    w: r.w,
                    h: bw,
                },
                border_color,
            );
            // Bottom
            renderer.rect(
                Rect {
                    x: r.x,
                    y: r.y + r.h - bw,
                    w: r.w,
                    h: bw,
                },
                border_color,
            );
            // Left
            renderer.rect(
                Rect {
                    x: r.x,
                    y: r.y,
                    w: bw,
                    h: r.h,
                },
                border_color,
            );
            // Right
            renderer.rect(
                Rect {
                    x: r.x + r.w - bw,
                    y: r.y,
                    w: bw,
                    h: r.h,
                },
                border_color,
            );

            // ステータスドット（左上）
            let dot_size = 8.0;
            let dot_color = match node.activity {
                NodeActivity::Active => Color {
                    r: 0.2,
                    g: 0.9,
                    b: 0.4,
                    a: 1.0,
                },
                NodeActivity::Inactive => Color {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                    a: 1.0,
                },
                NodeActivity::Error => Color {
                    r: 0.9,
                    g: 0.2,
                    b: 0.2,
                    a: 1.0,
                },
            };
            renderer.rect(
                Rect {
                    x: r.x + 6.0,
                    y: r.y + 6.0,
                    w: dot_size,
                    h: dot_size,
                },
                dot_color,
            );

            // レベルバー（下部）
            if node.level > 0.0 {
                let bar_h = 4.0;
                let bar_w = (r.w - 8.0) * node.level.clamp(0.0, 1.0);
                renderer.rect(
                    Rect {
                        x: r.x + 4.0,
                        y: r.y + r.h - bar_h - 4.0,
                        w: bar_w,
                        h: bar_h,
                    },
                    Color {
                        r: 0.0,
                        g: 0.7,
                        b: 0.9,
                        a: 0.8,
                    },
                );
            }

            // ラベル
            let label = node.kind.label();
            renderer.text(TextEntry {
                text: label,
                x: r.x + 18.0,
                y: r.y + r.h / 2.0 - theme::TEXT_SM / 2.0,
                size: theme::TEXT_SM,
                color: theme::TEXT_COLOR,
            });
        }
    }
}

/// デフォルトのノードレイアウト — 3 列固定配置
///
/// ```text
/// [Col 0: 入力]      [Col 1: 処理]      [Col 2: 出力]
///  MIDI In (0)         Synth (1)          Mixer (4)
///                      Beat (2)           Output (5)
///  Net Recv (6)        Looper (3)         Net Send (7)
/// ```
fn layout_nodes_default(graph: &AudioGraphState, w: f32, h: f32, out: &mut Vec<NodeRect>) {
    out.clear();
    if graph.nodes.is_empty() {
        return;
    }

    let s = theme::SCALE;
    let pad = 20.0 * s;
    let node_w = 100.0 * s;
    let node_h = 40.0 * s;
    let col_gap = ((w - pad * 2.0 - node_w * 3.0) / 2.0).max(20.0 * s);

    let col_x = [pad, pad + node_w + col_gap, pad + (node_w + col_gap) * 2.0];

    // 各ノードの列と行位置を手動で割り当て
    // (col, row_within_col) — row は列ごとのノード順
    let positions: [(usize, usize); 8] = [
        (0, 0), // 0: MIDI In
        (1, 0), // 1: Synth
        (1, 1), // 2: BeatMachine
        (1, 2), // 3: Looper
        (2, 0), // 4: Mixer
        (2, 1), // 5: AudioOutput
        (0, 2), // 6: NetworkRecv
        (2, 2), // 7: NetworkSend
    ];

    let title_offset = 50.0 * s;
    let usable_h = h - title_offset - pad;
    let rows_per_col = 3usize;
    let row_gap = ((usable_h - node_h * rows_per_col as f32) / (rows_per_col as f32 + 1.0))
        .max(10.0 * s);

    for (i, &(col, row)) in positions.iter().enumerate() {
        if i >= graph.nodes.len() {
            break;
        }
        let x = col_x[col];
        let y = title_offset + row_gap + (node_h + row_gap) * row as f32;
        out.push(NodeRect {
            rect: Rect {
                x,
                y,
                w: node_w,
                h: node_h,
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_flow_graph_default() {
        let g = SignalFlowGraph::new();
        assert!(g.graph.nodes.is_empty());
        assert!(g.graph.edges.is_empty());
    }

    #[test]
    fn build_graph_creates_nodes_and_edges() {
        let mut g = SignalFlowGraph::new();
        let flux = cplp_flux::FluxSnapshot {
            synth_state: cplp_flux::ModuleState::Playing,
            beat_machine_state: cplp_flux::ModuleState::Off,
            looper_state: cplp_flux::LooperState::Empty,
            active_plugin: Some("Diva".into()),
            bpm: 120.0,
        };
        let session = SessionSnapshot {
            connected: true,
            peer_name: "Peer".into(),
            ..Default::default()
        };

        g.build_graph(&flux, None, &session);

        assert_eq!(g.graph.nodes.len(), 8);
        assert!(!g.graph.edges.is_empty());

        // Synth ノードは Active であること
        assert_eq!(g.graph.nodes[1].activity, NodeActivity::Active);
        // BeatMachine は Inactive
        assert_eq!(g.graph.nodes[2].activity, NodeActivity::Inactive);
        // NetworkRecv は接続中なので Active
        assert_eq!(g.graph.nodes[6].activity, NodeActivity::Active);
    }

    #[test]
    fn layout_nodes_produces_correct_count() {
        let mut g = SignalFlowGraph::new();
        let flux = cplp_flux::FluxSnapshot::default();
        let session = SessionSnapshot::default();
        g.build_graph(&flux, None, &session);
        g.layout_nodes(1280.0, 960.0);

        assert_eq!(g.node_rects.len(), 8);
    }

    #[test]
    fn audio_node_kind_label() {
        assert_eq!(AudioNodeKind::MidiInput.label(), "MIDI In");
        assert_eq!(
            AudioNodeKind::Synth {
                plugin_name: "Diva".into()
            }
            .label(),
            "Diva"
        );
        assert_eq!(
            AudioNodeKind::Synth {
                plugin_name: String::new()
            }
            .label(),
            "Synth"
        );
        assert_eq!(AudioNodeKind::Mixer.label(), "Mixer");
    }
}

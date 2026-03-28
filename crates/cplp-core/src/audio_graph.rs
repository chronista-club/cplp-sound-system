//! AudioGraph — オーディオ/MIDIルーティングの SSOT
//!
//! ノード（モジュール）とエッジ（接続）でオーディオ信号とMIDIの流れを定義する。
//! Scene はこのデータ構造を可視化するだけ。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// 一意なノード識別子
pub type NodeId = u32;

/// 一意なエッジ識別子
pub type EdgeId = u32;

/// AudioGraph — 全体のルーティングを管理
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioGraph {
    nodes: HashMap<NodeId, Node>,
    edges: HashMap<EdgeId, Edge>,
    /// 重複接続の O(1) チェック用（edges から派生、シリアライズ対象外）
    #[serde(skip, default)]
    edge_set: HashSet<(NodeId, NodeId, EdgeTypeKey)>,
    next_node_id: NodeId,
    next_edge_id: EdgeId,
}

/// EdgeType の HashSet 用キー（Copy + Hash 対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum EdgeTypeKey {
    Midi,
    Audio,
}

impl From<EdgeType> for EdgeTypeKey {
    fn from(t: EdgeType) -> Self {
        match t {
            EdgeType::Midi => EdgeTypeKey::Midi,
            EdgeType::Audio => EdgeTypeKey::Audio,
        }
    }
}

/// ノード（モジュール）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub node_type: NodeType,
    /// Scene 上の位置 (x, y) — レイアウト用
    pub position: (f32, f32),
}

/// ノードの種類
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeType {
    /// MIDI 入力デバイス (Keystage 等)
    MidiInput,
    /// CLAP インストゥルメント (MIDI → Audio)
    ClapInstrument {
        plugin_id: String,
    },
    /// CLAP エフェクト (Audio → Audio)
    ClapEffect {
        plugin_id: String,
    },
    /// 自作 AudioModule (Looper, BeatMachine, Synth)
    AudioModule {
        module_type: AudioModuleType,
    },
    /// ミキサー (Audio x N → Audio)
    Mixer,
    /// オーディオ出力 (cpal)
    Output,
}

/// 自作モジュールの種類
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioModuleType {
    Synthesizer,
    Looper,
    BeatMachine,
}

/// エッジ（接続）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub from_node: NodeId,
    pub to_node: NodeId,
    pub edge_type: EdgeType,
}

/// エッジの種類
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EdgeType {
    /// MIDI 接続 (NoteOn/Off, CC, PitchBend)
    Midi,
    /// オーディオ接続 (f32 サンプルストリーム)
    Audio,
}

impl AudioGraph {
    /// 空の AudioGraph を作成
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            edge_set: HashSet::new(),
            next_node_id: 1,
            next_edge_id: 1,
        }
    }

    // ─── ノード操作 ────────────────────────────────────

    /// ノードを追加し、割り当てられた NodeId を返す
    pub fn add_node(&mut self, name: impl Into<String>, node_type: NodeType) -> NodeId {
        let id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.insert(
            id,
            Node {
                id,
                name: name.into(),
                node_type,
                position: (0.0, 0.0),
            },
        );
        id
    }

    /// ノードを削除（関連するエッジも削除）
    pub fn remove_node(&mut self, id: NodeId) -> bool {
        if self.nodes.remove(&id).is_some() {
            // edge_set からも削除
            self.edges.retain(|_, e| {
                if e.from_node == id || e.to_node == id {
                    self.edge_set.remove(&(
                        e.from_node,
                        e.to_node,
                        EdgeTypeKey::from(e.edge_type),
                    ));
                    false
                } else {
                    true
                }
            });
            true
        } else {
            false
        }
    }

    /// ノードを取得
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// ノードを可変参照で取得
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    /// 全ノードのイテレータ
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    /// ノード数
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// ノードの位置を設定
    pub fn set_node_position(&mut self, id: NodeId, x: f32, y: f32) -> bool {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.position = (x, y);
            true
        } else {
            false
        }
    }

    // ─── エッジ操作 ────────────────────────────────────

    /// エッジ（接続）を追加
    ///
    /// 両端のノードが存在しない場合は None を返す。
    pub fn connect(
        &mut self,
        from: NodeId,
        to: NodeId,
        edge_type: EdgeType,
    ) -> Option<EdgeId> {
        // 自己ループを拒否
        if from == to {
            return None;
        }
        // 両端のノードが存在するか確認
        if !self.nodes.contains_key(&from) || !self.nodes.contains_key(&to) {
            return None;
        }
        // 同じ接続が既にあれば何もしない (O(1))
        let key = (from, to, EdgeTypeKey::from(edge_type));
        if !self.edge_set.insert(key) {
            return None;
        }

        let id = self.next_edge_id;
        self.next_edge_id += 1;
        self.edges.insert(
            id,
            Edge {
                id,
                from_node: from,
                to_node: to,
                edge_type,
            },
        );
        Some(id)
    }

    /// エッジを削除
    pub fn disconnect(&mut self, edge_id: EdgeId) -> bool {
        if let Some(edge) = self.edges.remove(&edge_id) {
            self.edge_set
                .remove(&(edge.from_node, edge.to_node, EdgeTypeKey::from(edge.edge_type)));
            true
        } else {
            false
        }
    }

    /// 全エッジのイテレータ
    pub fn edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.values()
    }

    /// エッジ数
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 指定ノードの入力エッジを取得
    pub fn incoming_edges(&self, node_id: NodeId) -> Vec<&Edge> {
        self.edges
            .values()
            .filter(|e| e.to_node == node_id)
            .collect()
    }

    /// 指定ノードの出力エッジを取得
    pub fn outgoing_edges(&self, node_id: NodeId) -> Vec<&Edge> {
        self.edges
            .values()
            .filter(|e| e.from_node == node_id)
            .collect()
    }

    // ─── デフォルト構成 ────────────────────────────────

    /// Keystage + 簡易シンセ + Mixer + Output のデフォルト構成を作成
    pub fn default_setup() -> Self {
        let mut graph = Self::new();

        let keystage = graph.add_node("Keystage", NodeType::MidiInput);
        let synth = graph.add_node(
            "Synth",
            NodeType::AudioModule {
                module_type: AudioModuleType::Synthesizer,
            },
        );
        let mixer = graph.add_node("Mixer", NodeType::Mixer);
        let output = graph.add_node("Output", NodeType::Output);

        // レイアウト
        graph.set_node_position(keystage, 100.0, 300.0);
        graph.set_node_position(synth, 400.0, 300.0);
        graph.set_node_position(mixer, 700.0, 300.0);
        graph.set_node_position(output, 1000.0, 300.0);

        // 接続
        graph.connect(keystage, synth, EdgeType::Midi);
        graph.connect(synth, mixer, EdgeType::Audio);
        graph.connect(mixer, output, EdgeType::Audio);

        graph
    }

    /// edges から edge_set を再構築（デシリアライズ後に呼ぶ）
    pub fn rebuild_edge_set(&mut self) {
        self.edge_set = self
            .edges
            .values()
            .map(|e| (e.from_node, e.to_node, EdgeTypeKey::from(e.edge_type)))
            .collect();
    }
}

impl Default for AudioGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_nodes() {
        let mut graph = AudioGraph::new();
        let id = graph.add_node("Test", NodeType::MidiInput);
        assert_eq!(graph.node_count(), 1);
        assert!(graph.remove_node(id));
        assert_eq!(graph.node_count(), 0);
    }

    #[test]
    fn connect_and_disconnect() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node("A", NodeType::MidiInput);
        let b = graph.add_node("B", NodeType::Mixer);

        let edge = graph.connect(a, b, EdgeType::Midi);
        assert!(edge.is_some());
        assert_eq!(graph.edge_count(), 1);

        // 重複接続は None
        assert!(graph.connect(a, b, EdgeType::Midi).is_none());

        assert!(graph.disconnect(edge.unwrap()));
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn remove_node_removes_edges() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node("A", NodeType::MidiInput);
        let b = graph.add_node("B", NodeType::Mixer);
        graph.connect(a, b, EdgeType::Audio);
        assert_eq!(graph.edge_count(), 1);

        graph.remove_node(a);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn connect_nonexistent_node_returns_none() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node("A", NodeType::MidiInput);
        assert!(graph.connect(a, 999, EdgeType::Midi).is_none());
    }

    #[test]
    fn default_setup_has_correct_structure() {
        let graph = AudioGraph::default_setup();
        assert_eq!(graph.node_count(), 4); // Keystage, Synth, Mixer, Output
        assert_eq!(graph.edge_count(), 3); // MIDI + Audio x 2
    }

    #[test]
    fn incoming_outgoing_edges() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node("A", NodeType::MidiInput);
        let b = graph.add_node("B", NodeType::Mixer);
        let c = graph.add_node("C", NodeType::Output);
        graph.connect(a, b, EdgeType::Midi);
        graph.connect(b, c, EdgeType::Audio);

        assert_eq!(graph.incoming_edges(b).len(), 1);
        assert_eq!(graph.outgoing_edges(b).len(), 1);
        assert_eq!(graph.incoming_edges(a).len(), 0);
        assert_eq!(graph.outgoing_edges(c).len(), 0);
    }

    #[test]
    fn self_loop_rejected() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node("A", NodeType::Mixer);
        assert!(graph.connect(a, a, EdgeType::Audio).is_none());
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn serde_roundtrip_preserves_edge_set() {
        let graph = AudioGraph::default_setup();
        assert_eq!(graph.edge_count(), 3);

        let json = serde_json::to_string(&graph).unwrap();
        let mut restored: AudioGraph = serde_json::from_str(&json).unwrap();

        // edge_set は #[serde(skip)] なのでデシリアライズ後は空
        assert!(restored.edge_set.is_empty());

        // rebuild で復元
        restored.rebuild_edge_set();
        assert_eq!(restored.edge_set.len(), 3);

        // 既存の接続は重複として拒否される (Keystage=1 → Synth=2)
        assert!(restored.connect(1, 2, EdgeType::Midi).is_none());
        assert_eq!(restored.edge_count(), 3); // 変化なし
    }

    #[test]
    fn disconnect_then_reconnect() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node("A", NodeType::MidiInput);
        let b = graph.add_node("B", NodeType::Mixer);

        let edge = graph.connect(a, b, EdgeType::Midi).unwrap();
        graph.disconnect(edge);
        // edge_set もクリアされているので再接続できる
        assert!(graph.connect(a, b, EdgeType::Midi).is_some());
    }
}

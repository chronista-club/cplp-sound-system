//! SceneGraph – プラットフォーム非依存のシーンデータ構造
//!
//! GPU への依存を一切持たない。Clone・Debug を実装し、
//! スナップショットテストや状態保存が容易。

use std::collections::HashMap;

/// ノードを一意に識別する ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// 3D トランスフォーム（位置・回転・スケール）。
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    /// ワールド空間での位置 [x, y, z]
    pub position: [f32; 3],
    /// 回転クォータニオン [x, y, z, w]（正規化済み）
    pub rotation: [f32; 4],
    /// スケール [x, y, z]
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl Transform {
    pub fn at(position: [f32; 3]) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }
}

/// シーンに配置できるノードの種別。
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// ラックユニット（シンセ・エフェクタなど）。
    RackUnit {
        /// 機材名（表示用）
        name: String,
        /// アクティブ状態（音が出ているか）
        active: bool,
        /// 占有ラックユニット数（高さの基準）
        rack_units: u32,
    },
    /// ケーブル（2 ノード間の接続）。
    Cable {
        from: NodeId,
        to: NodeId,
    },
    /// 背景プレーン。
    Background,
}

/// SceneGraph の 1 ノード。
#[derive(Debug, Clone)]
pub struct SceneNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub transform: Transform,
}

/// シーン全体を保持するグラフ。
///
/// GPU オブジェクトへの参照を持たない。
/// `lookup` により NodeId から O(1) でノードを取得できる。
#[derive(Debug, Clone)]
pub struct SceneGraph {
    nodes: Vec<SceneNode>,
    /// NodeId → nodes 配列インデックス のマップ（O(1) 検索用）
    lookup: HashMap<NodeId, usize>,
    next_id: u32,
}

impl Default for SceneGraph {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            lookup: HashMap::new(),
            next_id: 0,
        }
    }
}

impl SceneGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// ノードを追加し、割り当てた `NodeId` を返す。
    pub fn add_node(&mut self, kind: NodeKind, transform: Transform) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        let idx = self.nodes.len();
        self.nodes.push(SceneNode { id, kind, transform });
        self.lookup.insert(id, idx);
        id
    }

    /// 指定 ID のノードを O(1) で削除する。存在しない場合は何もしない。
    ///
    /// 内部的に `swap_remove` を使うため、削除後にノードの順序が変わる可能性がある。
    pub fn remove_node(&mut self, id: NodeId) {
        if let Some(&idx) = self.lookup.get(&id) {
            self.nodes.swap_remove(idx);
            self.lookup.remove(&id);
            // swap_remove で末尾ノードが idx に移動した場合、そのインデックスを更新する
            if idx < self.nodes.len() {
                let moved_id = self.nodes[idx].id;
                self.lookup.insert(moved_id, idx);
            }
        }
    }

    /// 指定 ID のノードへの参照を O(1) で返す。
    pub fn get(&self, id: NodeId) -> Option<&SceneNode> {
        self.lookup.get(&id).map(|&i| &self.nodes[i])
    }

    /// 指定 ID のノードへの可変参照を O(1) で返す。
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut SceneNode> {
        self.lookup.get(&id).map(|&i| &mut self.nodes[i])
    }

    /// 全ノードのスライスを返す。
    pub fn nodes(&self) -> &[SceneNode] {
        &self.nodes
    }

    /// 全ノードをクリアする。
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.lookup.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_node() {
        let mut graph = SceneGraph::new();
        let id = graph.add_node(
            NodeKind::RackUnit {
                name: "Surge XT".into(),
                active: true,
                rack_units: 2,
            },
            Transform::at([0.0, 0.0, -1.0]),
        );
        assert_eq!(graph.nodes().len(), 1);
        graph.remove_node(id);
        assert!(graph.nodes().is_empty());
        assert!(graph.get(id).is_none());
    }

    #[test]
    fn get_returns_correct_node() {
        let mut graph = SceneGraph::new();
        let id = graph.add_node(NodeKind::Background, Transform::at([1.0, 2.0, 3.0]));
        let node = graph.get(id).unwrap();
        assert_eq!(node.transform.position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn remove_updates_lookup_for_swapped_node() {
        let mut graph = SceneGraph::new();
        let a = graph.add_node(NodeKind::Background, Transform::default());
        let b = graph.add_node(NodeKind::Background, Transform::default());
        let c = graph.add_node(NodeKind::Background, Transform::default());

        // a を削除すると c が index 0 に移動する
        graph.remove_node(a);
        assert!(graph.get(a).is_none());
        assert!(graph.get(b).is_some());
        assert!(graph.get(c).is_some());
    }

    #[test]
    fn cable_references_nodes() {
        let mut graph = SceneGraph::new();
        let a = graph.add_node(NodeKind::Background, Transform::default());
        let b = graph.add_node(NodeKind::Background, Transform::default());
        let cable_id = graph.add_node(
            NodeKind::Cable { from: a, to: b },
            Transform::default(),
        );
        let node = graph.get(cable_id).unwrap();
        assert!(matches!(node.kind, NodeKind::Cable { .. }));
    }
}

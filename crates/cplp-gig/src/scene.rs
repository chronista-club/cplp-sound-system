//! SceneGraph – プラットフォーム非依存のシーンデータ構造
//!
//! GPU への依存を一切持たない。Clone・Debug を実装し、
//! スナップショットテストや状態保存が容易。

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
/// バックエンドへの送信前に `build_draw_calls` でジオメトリ情報へ変換する。
#[derive(Debug, Clone, Default)]
pub struct SceneGraph {
    nodes: Vec<SceneNode>,
    next_id: u32,
}

impl SceneGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// ノードを追加し、割り当てた `NodeId` を返す。
    pub fn add_node(&mut self, kind: NodeKind, transform: Transform) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(SceneNode { id, kind, transform });
        id
    }

    /// 指定 ID のノードを削除する。存在しない場合は何もしない。
    pub fn remove_node(&mut self, id: NodeId) {
        self.nodes.retain(|n| n.id != id);
    }

    /// 指定 ID のノードへの可変参照を返す。
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut SceneNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// 全ノードのスライスを返す。
    pub fn nodes(&self) -> &[SceneNode] {
        &self.nodes
    }

    /// 全ノードをクリアする。
    pub fn clear(&mut self) {
        self.nodes.clear();
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
        let node = graph.nodes().iter().find(|n| n.id == cable_id).unwrap();
        assert!(matches!(node.kind, NodeKind::Cable { .. }));
    }
}

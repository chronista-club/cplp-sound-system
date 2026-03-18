//! SceneGraph — プラットフォーム非依存のシーンデータ構造
//!
//! ノード階層、トランスフォーム、アニメーション、メッシュデータを保持する。
//! wgpu / RealityKit / その他バックエンドに依存しない。

/// シーングラフ全体
pub struct SceneGraph {
    /// ルートノード群
    pub nodes: Vec<SceneNode>,
    /// ユーロラック固有パラメータ
    pub rack_config: RackConfig,
}

/// ユーロラックラック構成
pub struct RackConfig {
    /// ラック幅（HP 単位）
    pub total_hp: u32,
    /// ラック行数
    pub rows: u32,
    /// フレーム色 [r, g, b]
    pub frame_color: [f32; 3],
}

impl Default for RackConfig {
    fn default() -> Self {
        Self {
            total_hp: 84,
            rows: 2,
            frame_color: [0.42, 0.42, 0.46],
        }
    }
}

/// シーングラフのノード
pub struct SceneNode {
    /// ノード名（デバッグ・識別用）
    pub name: String,
    /// トランスフォーム
    pub transform: Transform,
    /// メッシュデータ（None = グループノード）
    pub mesh: Option<MeshData>,
    /// アニメーションパラメータ
    pub animation: Option<AnimationParams>,
    /// 子ノード
    pub children: Vec<SceneNode>,
}

/// 3D トランスフォーム
#[derive(Clone, Debug)]
pub struct Transform {
    /// 位置 [x, y, z]
    pub position: [f32; 3],
    /// 回転（オイラー角 [rx, ry, rz] ラジアン）
    pub rotation: [f32; 3],
    /// スケール [sx, sy, sz]
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl Transform {
    /// 位置のみ指定（回転・スケールはデフォルト）
    pub fn from_position(position: [f32; 3]) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }

    /// TRS（Translation-Rotation-Scale）からモデル行列を計算
    ///
    /// 現在は回転なし（uniform scale + translation）のみ最適化。
    /// 回転が必要になったらクォータニオン対応を追加する。
    pub fn to_model_matrix(&self) -> [[f32; 4]; 4] {
        let [sx, sy, sz] = self.scale;
        let [tx, ty, tz] = self.position;
        [
            [sx, 0.0, 0.0, 0.0],
            [0.0, sy, 0.0, 0.0],
            [0.0, 0.0, sz, 0.0],
            [tx, ty, tz, 1.0],
        ]
    }

    /// ノーマル行列を計算（transpose(inverse(upper-left 3x3))）
    ///
    /// uniform scale の場合は (1/scale) * I に簡略化。
    pub fn to_normal_matrix(&self) -> [[f32; 4]; 4] {
        let [sx, sy, sz] = self.scale;
        let inv_sx = if sx != 0.0 { 1.0 / sx } else { 0.0 };
        let inv_sy = if sy != 0.0 { 1.0 / sy } else { 0.0 };
        let inv_sz = if sz != 0.0 { 1.0 / sz } else { 0.0 };
        [
            [inv_sx, 0.0, 0.0, 0.0],
            [0.0, inv_sy, 0.0, 0.0],
            [0.0, 0.0, inv_sz, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }
}

/// アニメーションパラメータ（プラットフォーム非依存）
#[derive(Clone, Debug)]
pub struct AnimationParams {
    /// 上下の浮遊振幅
    pub bob_amplitude: f32,
    /// 浮遊速度（rad/sec）
    pub bob_speed: f32,
    /// 呼吸スケール振幅（1.0 ± scale_amplitude）
    pub breathe_amplitude: f32,
    /// 呼吸速度（rad/sec）
    pub breathe_speed: f32,
    /// 位相オフセット（オブジェクト毎にずらす）
    pub phase_offset: f32,
}

impl AnimationParams {
    /// 時刻 t でのトランスフォーム差分を計算
    ///
    /// 返り値: (dy: f32, scale: f32)
    pub fn evaluate(&self, time: f32) -> (f32, f32) {
        let t = time * self.bob_speed + self.phase_offset;
        let dy = t.sin() * self.bob_amplitude;
        let bt = time * self.breathe_speed + self.phase_offset * 0.7;
        let scale = 1.0 + bt.sin() * self.breathe_amplitude;
        (dy, scale)
    }
}

/// プラットフォーム非依存のメッシュデータ
///
/// CPU 側の頂点・インデックスデータを保持。
/// GPU バッファへのアップロードは RenderBackend が行う。
pub struct MeshData {
    /// 頂点データ（position + color + normal = 9 floats per vertex）
    pub vertices: Vec<MeshVertex>,
    /// インデックスデータ
    pub indices: Vec<u32>,
}

/// プラットフォーム非依存の頂点データ
///
/// `bytemuck::Pod` を実装しているため、GPU バッファへの直接コピーが可能。
/// wgpu / Metal どちらのバックエンドでも同じメモリレイアウトで使える。
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub normal: [f32; 3],
}

// ── SceneGraph ビルダー ─────────────────────────────

impl SceneGraph {
    /// 空のシーングラフを作成
    pub fn new(rack_config: RackConfig) -> Self {
        Self {
            nodes: Vec::new(),
            rack_config,
        }
    }

    /// ノードを追加
    pub fn add_node(&mut self, node: SceneNode) {
        self.nodes.push(node);
    }

    /// 全ノードのフラットイテレータ（深さ優先）
    pub fn iter_nodes(&self) -> SceneNodeIter<'_> {
        SceneNodeIter {
            stack: self.nodes.iter().collect(),
        }
    }

    /// ノード数（再帰的にカウント）
    pub fn node_count(&self) -> usize {
        self.iter_nodes().count()
    }
}

/// 深さ優先のノードイテレータ
pub struct SceneNodeIter<'a> {
    stack: Vec<&'a SceneNode>,
}

impl<'a> Iterator for SceneNodeIter<'a> {
    type Item = &'a SceneNode;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        // 子ノードを逆順でスタックに積む（左から処理するため）
        for child in node.children.iter().rev() {
            self.stack.push(child);
        }
        Some(node)
    }
}

impl SceneNode {
    /// メッシュ付きリーフノードを作成
    pub fn leaf(name: impl Into<String>, position: [f32; 3], mesh: MeshData) -> Self {
        Self {
            name: name.into(),
            transform: Transform::from_position(position),
            mesh: Some(mesh),
            animation: None,
            children: Vec::new(),
        }
    }

    /// アニメーション付きリーフノードを作成
    pub fn animated_leaf(
        name: impl Into<String>,
        position: [f32; 3],
        mesh: MeshData,
        animation: AnimationParams,
    ) -> Self {
        Self {
            name: name.into(),
            transform: Transform::from_position(position),
            mesh: Some(mesh),
            animation: Some(animation),
            children: Vec::new(),
        }
    }

    /// グループノード（メッシュなし）を作成
    pub fn group(name: impl Into<String>, position: [f32; 3], children: Vec<SceneNode>) -> Self {
        Self {
            name: name.into(),
            transform: Transform::from_position(position),
            mesh: None,
            animation: None,
            children,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_default_is_identity() {
        let t = Transform::default();
        let m = t.to_model_matrix();
        assert_eq!(m[0][0], 1.0);
        assert_eq!(m[1][1], 1.0);
        assert_eq!(m[2][2], 1.0);
        assert_eq!(m[3][3], 1.0);
        assert_eq!(m[3][0], 0.0);
    }

    #[test]
    fn transform_position() {
        let t = Transform::from_position([1.0, 2.0, 3.0]);
        let m = t.to_model_matrix();
        assert_eq!(m[3][0], 1.0);
        assert_eq!(m[3][1], 2.0);
        assert_eq!(m[3][2], 3.0);
    }

    #[test]
    fn animation_evaluate() {
        let anim = AnimationParams {
            bob_amplitude: 0.1,
            bob_speed: 1.0,
            breathe_amplitude: 0.05,
            breathe_speed: 1.0,
            phase_offset: 0.0,
        };
        let (dy, scale) = anim.evaluate(0.0);
        assert!((dy - 0.0).abs() < 0.001);
        assert!((scale - 1.0).abs() < 0.001);
    }

    #[test]
    fn scene_graph_node_count() {
        let mut sg = SceneGraph::new(RackConfig::default());
        let mesh = MeshData {
            vertices: vec![],
            indices: vec![],
        };
        let child = SceneNode::leaf("child", [0.0, 0.0, 0.0], mesh);
        let mesh2 = MeshData {
            vertices: vec![],
            indices: vec![],
        };
        let parent = SceneNode::group("parent", [0.0, 0.0, 0.0], vec![child]);
        sg.add_node(parent);
        sg.add_node(SceneNode::leaf("solo", [1.0, 0.0, 0.0], mesh2));
        assert_eq!(sg.node_count(), 3);
    }
}

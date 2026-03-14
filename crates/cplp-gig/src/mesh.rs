//! メッシュ生成ロジック – プラットフォーム非依存
//!
//! SceneNode の種別から `MeshData`（頂点・インデックス列）を生成する。
//! GPU バッファへのアップロードは行わない。純粋関数で実装されており、
//! GPU デバイスなしで単体テストが可能。

use crate::scene::{NodeId, NodeKind, SceneGraph, SceneNode, Transform};

/// 1 頂点のデータ。
///
/// `bytemuck::Pod` / `Zeroable` は `wgpu` feature が有効なときのみ derive する。
/// これにより GPU バッファへの zero-copy 書き込みが可能になる。
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "wgpu", derive(bytemuck::Pod, bytemuck::Zeroable))]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// 1 メッシュのジオメトリデータ。GPU への依存なし。
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// 新しい空の MeshData を返す。
    pub fn new() -> Self {
        Self::default()
    }

    /// 頂点数を返す。
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// インデックス数（= 三角形数 × 3）を返す。
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }
}

// ── 生成関数 ──────────────────────────────────────────────────────────────────

/// SceneNode から対応する MeshData を生成する。
///
/// `Cable` ノードは `graph` から接続先ノードの位置を解決するため、
/// `SceneGraph` への参照が必要。他のノード種別は `graph` を使用しない。
/// 未対応の NodeKind には空の MeshData を返す。
pub fn build_mesh(node: &SceneNode, graph: &SceneGraph) -> MeshData {
    match &node.kind {
        NodeKind::RackUnit { active, rack_units, .. } => {
            rack_unit_mesh(&node.transform, *active, *rack_units)
        }
        NodeKind::Cable { from, to } => {
            let from_pos = resolve_position(*from, graph);
            let to_pos = resolve_position(*to, graph);
            cable_mesh(from_pos, to_pos)
        }
        NodeKind::Background => background_mesh(),
    }
}

/// NodeId からノードの位置を解決する。ノードが存在しない場合は原点を返す。
fn resolve_position(id: NodeId, graph: &SceneGraph) -> [f32; 3] {
    graph.get(id).map(|n| n.transform.position).unwrap_or([0.0, 0.0, 0.0])
}

/// ラックユニット（直方体）のメッシュを生成する。
///
/// 高さは `rack_units × 0.044 m`（19 インチラック 1U = 44.45 mm）。
fn rack_unit_mesh(transform: &Transform, active: bool, rack_units: u32) -> MeshData {
    let w = 0.48; // 19 インチラック幅
    let h = 0.04445 * rack_units as f32;
    let d = 0.30; // 奥行き 300 mm

    let color = if active {
        [0.2, 0.9, 0.4, 1.0] // アクティブ: 緑
    } else {
        [0.3, 0.3, 0.35, 1.0] // 非アクティブ: グレー
    };

    let [px, py, pz] = transform.position;
    let hw = w / 2.0;
    let hh = h / 2.0;
    let hd = d / 2.0;

    #[rustfmt::skip]
    let face_data: &[([f32; 3], [f32; 3])] = &[
        ([0.0,  0.0,  1.0], [0.0, 0.0,  hd]), // 前
        ([0.0,  0.0, -1.0], [0.0, 0.0, -hd]), // 後
        ([-1.0, 0.0,  0.0], [-hw, 0.0, 0.0]), // 左
        ([1.0,  0.0,  0.0], [ hw, 0.0, 0.0]), // 右
        ([0.0,  1.0,  0.0], [0.0,  hh, 0.0]), // 上
        ([0.0, -1.0,  0.0], [0.0, -hh, 0.0]), // 下
    ];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    for (face_idx, (normal, center)) in face_data.iter().enumerate() {
        let (right, up) = tangent_frame(normal);
        let (right_e, up_e) = face_half_extents(normal, hw, hh, hd);

        let corners = [
            [-right_e, -up_e],
            [ right_e, -up_e],
            [ right_e,  up_e],
            [-right_e,  up_e],
        ];
        let uvs = [[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

        let base = (face_idx * 4) as u32;
        for ([ru, uu], uv) in corners.iter().zip(uvs.iter()) {
            vertices.push(Vertex {
                position: [
                    px + center[0] + right[0] * ru + up[0] * uu,
                    py + center[1] + right[1] * ru + up[1] * uu,
                    pz + center[2] + right[2] * ru + up[2] * uu,
                ],
                normal: *normal,
                uv: *uv,
                color,
            });
        }
        indices.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
    }

    MeshData { vertices, indices }
}

/// ケーブル（細い4辺柱）のメッシュを2点間で生成する。
///
/// `from_pos` から `to_pos` へ延びるケーブルを表す。
/// 2点が同一の場合（縮退ケーブル）は空のメッシュを返す。
fn cable_mesh(from_pos: [f32; 3], to_pos: [f32; 3]) -> MeshData {
    let dx = to_pos[0] - from_pos[0];
    let dy = to_pos[1] - from_pos[1];
    let dz = to_pos[2] - from_pos[2];
    let len = (dx * dx + dy * dy + dz * dz).sqrt();
    if len < 1e-6 {
        return MeshData::new(); // 縮退ケーブル: メッシュなし
    }

    let r = 0.005f32; // 半径 5 mm
    let color = [0.8, 0.5, 0.1, 1.0];

    let dir = [dx / len, dy / len, dz / len];
    let (right, up) = tangent_frame(&dir);

    // ケーブル方向に垂直な 4 方向のオフセット
    let offsets: [[f32; 2]; 4] = [[r, 0.0], [0.0, r], [-r, 0.0], [0.0, -r]];

    let mut vertices = Vec::with_capacity(8);
    let mut indices = Vec::with_capacity(24);

    // from 側の 4 頂点
    for &[or, ou] in &offsets {
        let ox = right[0] * or + up[0] * ou;
        let oy = right[1] * or + up[1] * ou;
        let oz = right[2] * or + up[2] * ou;
        vertices.push(Vertex {
            position: [from_pos[0] + ox, from_pos[1] + oy, from_pos[2] + oz],
            normal: normalize([ox, oy, oz]),
            uv: [0.0, 0.0],
            color,
        });
    }
    // to 側の 4 頂点
    for &[or, ou] in &offsets {
        let ox = right[0] * or + up[0] * ou;
        let oy = right[1] * or + up[1] * ou;
        let oz = right[2] * or + up[2] * ou;
        vertices.push(Vertex {
            position: [to_pos[0] + ox, to_pos[1] + oy, to_pos[2] + oz],
            normal: normalize([ox, oy, oz]),
            uv: [0.0, 1.0],
            color,
        });
    }

    for i in 0u32..4 {
        let next = (i + 1) % 4;
        let (t0, t1, b0, b1) = (i, next, i + 4, next + 4);
        indices.extend_from_slice(&[t0, b0, b1, t0, b1, t1]);
    }

    MeshData { vertices, indices }
}

/// 背景プレーン（XZ 平面上の単純な四角形）を生成する。
fn background_mesh() -> MeshData {
    let size = 5.0f32;
    let color = [0.05, 0.05, 0.08, 1.0];
    let normal = [0.0f32, 1.0, 0.0];

    let vertices = vec![
        Vertex { position: [-size, 0.0, -size], normal, uv: [0.0, 0.0], color },
        Vertex { position: [ size, 0.0, -size], normal, uv: [1.0, 0.0], color },
        Vertex { position: [ size, 0.0,  size], normal, uv: [1.0, 1.0], color },
        Vertex { position: [-size, 0.0,  size], normal, uv: [0.0, 1.0], color },
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];

    MeshData { vertices, indices }
}

// ── ヘルパー ──────────────────────────────────────────────────────────────────

/// 法線から接線フレーム (right, up) を導出する（グラム=シュミット）。
fn tangent_frame(normal: &[f32; 3]) -> ([f32; 3], [f32; 3]) {
    let up_hint = if normal[1].abs() < 0.9 { [0.0f32, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let right = normalize(cross(normal, &up_hint));
    let up = cross(&right, normal);
    (right, up)
}

/// 各面のハーフエクステント (right_e, up_e) を返す。
fn face_half_extents(normal: &[f32; 3], hw: f32, hh: f32, hd: f32) -> (f32, f32) {
    if normal[2].abs() > 0.5 { (hw, hh) }      // 前後面
    else if normal[0].abs() > 0.5 { (hd, hh) } // 左右面
    else { (hw, hd) }                           // 上下面
}

fn cross(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-6 { v } else { [v[0] / len, v[1] / len, v[2] / len] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{NodeKind, NodeId, SceneGraph, SceneNode, Transform};

    fn make_node(kind: NodeKind) -> SceneNode {
        SceneNode { id: NodeId(0), kind, transform: Transform::default() }
    }

    #[test]
    fn rack_unit_mesh_vertex_count() {
        let graph = SceneGraph::new();
        let node = make_node(NodeKind::RackUnit {
            name: "Test".into(),
            active: false,
            rack_units: 1,
        });
        let mesh = build_mesh(&node, &graph);
        assert_eq!(mesh.vertex_count(), 24); // 6 面 × 4 頂点
        assert_eq!(mesh.index_count(), 36);  // 6 面 × 2 三角形 × 3 インデックス
    }

    #[test]
    fn background_mesh_valid() {
        let graph = SceneGraph::new();
        let node = make_node(NodeKind::Background);
        let mesh = build_mesh(&node, &graph);
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.index_count(), 6);
    }

    #[test]
    fn active_rack_unit_has_green_color() {
        let graph = SceneGraph::new();
        let node = make_node(NodeKind::RackUnit {
            name: "Synth".into(),
            active: true,
            rack_units: 2,
        });
        let mesh = build_mesh(&node, &graph);
        assert!(mesh.vertices[0].color[1] > 0.8);
    }

    #[test]
    fn cable_mesh_connects_from_to_positions() {
        let mut graph = SceneGraph::new();
        let a = graph.add_node(NodeKind::Background, Transform::at([0.0, 0.0, 0.0]));
        let b = graph.add_node(NodeKind::Background, Transform::at([1.0, 0.0, 0.0]));
        let cable_node = SceneNode {
            id: NodeId(99),
            kind: NodeKind::Cable { from: a, to: b },
            transform: Transform::default(),
        };
        let mesh = build_mesh(&cable_node, &graph);
        assert_eq!(mesh.vertex_count(), 8);
        assert_eq!(mesh.index_count(), 24);
        // from (x=0) から to (x=1) までスパンしているか確認
        let min_x = mesh.vertices.iter().map(|v| v.position[0]).fold(f32::INFINITY, f32::min);
        let max_x = mesh.vertices.iter().map(|v| v.position[0]).fold(f32::NEG_INFINITY, f32::max);
        assert!(max_x - min_x > 0.9);
    }

    #[test]
    fn cable_mesh_degenerate_returns_empty() {
        let mut graph = SceneGraph::new();
        let a = graph.add_node(NodeKind::Background, Transform::at([1.0, 0.0, 0.0]));
        let b = graph.add_node(NodeKind::Background, Transform::at([1.0, 0.0, 0.0])); // 同座標
        let cable_node = SceneNode {
            id: NodeId(99),
            kind: NodeKind::Cable { from: a, to: b },
            transform: Transform::default(),
        };
        let mesh = build_mesh(&cable_node, &graph);
        assert_eq!(mesh.vertex_count(), 0);
    }
}

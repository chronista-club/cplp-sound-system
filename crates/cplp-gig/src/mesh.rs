//! メッシュ生成ロジック – プラットフォーム非依存
//!
//! SceneNode の種別から `MeshData`（頂点・インデックス列）を生成する。
//! GPU バッファへのアップロードは行わない。純粋関数で実装されており、
//! GPU デバイスなしで単体テストが可能。

use crate::scene::{NodeKind, SceneNode, Transform};

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
/// 未対応の NodeKind には空の MeshData を返す。
pub fn build_mesh(node: &SceneNode) -> MeshData {
    match &node.kind {
        NodeKind::RackUnit { active, rack_units, .. } => {
            rack_unit_mesh(&node.transform, *active, *rack_units)
        }
        NodeKind::Cable { .. } => cable_mesh(&node.transform),
        NodeKind::Background => background_mesh(),
    }
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

    // 前面・後面・左右・上下の 6 面（各 4 頂点 × 6 面 = 24 頂点）
    let hw = w / 2.0;
    let hh = h / 2.0;
    let hd = d / 2.0;

    #[rustfmt::skip]
    let face_data: &[([f32; 3], [f32; 3])] = &[
        // (法線, 面の中心オフセット) – 各面の 4 頂点を展開する基底として使用
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
        // 面に平行な 2 軸を法線から導出（簡易版）
        let (right, up) = tangent_frame(normal);

        let half_extents = face_half_extents(normal, hw, hh, hd);
        let (right_e, up_e) = half_extents;

        let corners = [
            [-right_e, -up_e],
            [ right_e, -up_e],
            [ right_e,  up_e],
            [-right_e,  up_e],
        ];
        let uvs = [[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

        let base = (face_idx * 4) as u32;
        for (i, ([ru, uu], uv)) in corners.iter().zip(uvs.iter()).enumerate() {
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
            let _ = i; // suppress unused warning
        }
        indices.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
    }

    MeshData { vertices, indices }
}

/// ケーブル（細い円柱）のメッシュを生成する（簡易版: 4 辺柱）。
fn cable_mesh(transform: &Transform) -> MeshData {
    let r = 0.005f32; // 半径 5 mm
    let [px, py, pz] = transform.position;
    let color = [0.8, 0.5, 0.1, 1.0];

    // 単純な 4 角柱（上端 → 下端）
    let top_y = py + 0.5;
    let bot_y = py - 0.5;
    let offsets = [[r, 0.0], [0.0, r], [-r, 0.0], [0.0, -r]];

    let mut vertices = Vec::with_capacity(8);
    let mut indices = Vec::with_capacity(24);

    for &[ox, oz] in &offsets {
        vertices.push(Vertex {
            position: [px + ox, top_y, pz + oz],
            normal: [ox / r, 0.0, oz / r],
            uv: [0.0, 0.0],
            color,
        });
    }
    for &[ox, oz] in &offsets {
        vertices.push(Vertex {
            position: [px + ox, bot_y, pz + oz],
            normal: [ox / r, 0.0, oz / r],
            uv: [0.0, 1.0],
            color,
        });
    }

    for i in 0u32..4 {
        let next = (i + 1) % 4;
        let t0 = i;
        let t1 = next;
        let b0 = i + 4;
        let b1 = next + 4;
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
    let right = cross(normal, &up_hint);
    let right = normalize(right);
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
    use crate::scene::{NodeKind, SceneNode, NodeId, Transform};

    fn make_node(kind: NodeKind) -> SceneNode {
        SceneNode { id: NodeId(0), kind, transform: Transform::default() }
    }

    #[test]
    fn rack_unit_mesh_vertex_count() {
        let node = make_node(NodeKind::RackUnit {
            name: "Test".into(),
            active: false,
            rack_units: 1,
        });
        let mesh = build_mesh(&node);
        // 6 面 × 4 頂点
        assert_eq!(mesh.vertex_count(), 24);
        // 6 面 × 2 三角形 × 3 インデックス
        assert_eq!(mesh.index_count(), 36);
    }

    #[test]
    fn background_mesh_valid() {
        let node = make_node(NodeKind::Background);
        let mesh = build_mesh(&node);
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.index_count(), 6);
    }

    #[test]
    fn active_rack_unit_has_green_color() {
        let node = make_node(NodeKind::RackUnit {
            name: "Synth".into(),
            active: true,
            rack_units: 2,
        });
        let mesh = build_mesh(&node);
        // 最初の頂点が「アクティブ緑」
        assert!(mesh.vertices[0].color[1] > 0.8);
    }
}

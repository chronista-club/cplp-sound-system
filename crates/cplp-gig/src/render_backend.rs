//! RenderBackend trait – プラットフォーム非依存の描画インターフェース
//!
//! wgpu・Metal・RealityKit など、具体的な GPU バックエンドを抽象化する。
//! visionOS 向けの実装はこのトレイトを実装するだけで cplp-gig の他のコードを
//! 変更せずに動作させることができる。

use crate::mesh::MeshData;

/// GPU 上にアップロード済みのメッシュを参照する不透明なハンドル。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshHandle(pub(crate) u32);

/// 1 フレームで発行する描画命令。
#[derive(Debug, Clone)]
pub struct DrawCall {
    /// 描画するメッシュへのハンドル
    pub mesh: MeshHandle,
    /// モデル行列（列優先 4×4）
    ///
    /// `transform[col][row]` の順で要素を格納する列優先レイアウト。
    /// WGSL の `mat4x4<f32>` と同じメモリ配置であり、バイト列にキャストして
    /// そのままシェーダーへ転送できる。
    ///
    /// 例: 平行移動 (tx, ty, tz) のみの行列:
    /// ```text
    /// transform[0] = [1, 0, 0, 0]  // 列 0
    /// transform[1] = [0, 1, 0, 0]  // 列 1
    /// transform[2] = [0, 0, 1, 0]  // 列 2
    /// transform[3] = [tx, ty, tz, 1] // 列 3（平行移動列）
    /// ```
    pub transform: [[f32; 4]; 4],
}

impl DrawCall {
    /// 単位行列のトランスフォームで DrawCall を作成する。
    pub fn identity(mesh: MeshHandle) -> Self {
        Self {
            mesh,
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

/// 描画バックエンドの抽象トレイト。
///
/// バックエンドの実装者は次の責務を持つ:
/// - `upload_mesh`: CPU 側の `MeshData` を GPU メモリへ転送し `MeshHandle` を返す
/// - `free_mesh`: アップロード済みメッシュを解放する
/// - `submit_frame`: 1 フレーム分の `DrawCall` リストを GPU に送信し描画する
/// - `resize`: ウィンドウ/ビューポートのサイズ変更に対応する
pub trait RenderBackend {
    /// `MeshData` を GPU へアップロードし、ハンドルを返す。
    fn upload_mesh(&mut self, mesh: &MeshData) -> MeshHandle;

    /// アップロード済みメッシュを GPU メモリから解放する。
    fn free_mesh(&mut self, handle: MeshHandle);

    /// 1 フレーム分の描画コマンドを GPU に送信する。
    ///
    /// `draw_calls` は上から順に描画される（後のものが手前に表示）。
    fn submit_frame(&mut self, draw_calls: &[DrawCall]);

    /// ビューポートサイズを更新する。
    fn resize(&mut self, width: u32, height: u32);
}

/// SceneGraph 全体の描画コマンドを構築するヘルパー。
///
/// `upload_mesh` で事前にアップロードされたハンドルと SceneNode のリストを受け取り、
/// `DrawCall` のリストを生成する。
/// `SceneGraph::get` による O(1) 検索を使用するため、全体計算量は O(M)。
pub fn build_draw_calls(
    node_handles: &[(crate::scene::NodeId, MeshHandle)],
    graph: &crate::scene::SceneGraph,
) -> Vec<DrawCall> {
    node_handles
        .iter()
        .filter_map(|(node_id, mesh_handle)| {
            graph.get(*node_id).map(|node| {
                let t = &node.transform;
                // 列優先モデル行列（スケール + 平行移動）。
                // transform[col][row] の順。列 3 が平行移動列。
                // 将来: 四元数からの回転行列を乗算する。
                let transform = [
                    [t.scale[0], 0.0, 0.0, 0.0], // 列 0
                    [0.0, t.scale[1], 0.0, 0.0],  // 列 1
                    [0.0, 0.0, t.scale[2], 0.0],  // 列 2
                    [t.position[0], t.position[1], t.position[2], 1.0], // 列 3
                ];
                DrawCall { mesh: *mesh_handle, transform }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{NodeKind, SceneGraph, Transform};

    #[test]
    fn build_draw_calls_maps_node_to_mesh() {
        let mut graph = SceneGraph::new();
        let id = graph.add_node(NodeKind::Background, Transform::at([1.0, 2.0, 3.0]));

        let handle = MeshHandle(42);
        let calls = build_draw_calls(&[(id, handle)], &graph);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].mesh, handle);
        // 列 3 が平行移動列: transform[3] = [tx, ty, tz, 1]
        assert_eq!(calls[0].transform[3], [1.0, 2.0, 3.0, 1.0]);
    }

    #[test]
    fn build_draw_calls_skips_missing_nodes() {
        let graph = SceneGraph::new();
        let handle = MeshHandle(0);
        let calls = build_draw_calls(&[(crate::scene::NodeId(99), handle)], &graph);
        assert!(calls.is_empty());
    }
}

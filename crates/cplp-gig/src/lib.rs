//! cplp-gig – Gig Scene（3D ラック/ケーブル UI）
//!
//! visionOS 対応を見据え、シーングラフ・メッシュ生成・GPU パイプラインを
//! 明確に分離したアーキテクチャを採用している。
//!
//! # モジュール構成
//!
//! | モジュール | 依存 | 責務 |
//! |------------|------|------|
//! | `scene` | なし | SceneGraph データ構造 |
//! | `mesh` | なし（`wgpu` feat で `bytemuck` のみ追加） | メッシュ生成ロジック |
//! | `render_backend` | なし | `RenderBackend` trait・`DrawCall` |
//! | `wgpu_backend` | `wgpu` feat | wgpu 実装 |

pub mod mesh;
pub mod render_backend;
pub mod scene;

#[cfg(feature = "wgpu")]
pub mod wgpu_backend;

pub use mesh::{MeshData, Vertex, build_mesh};
pub use render_backend::{DrawCall, MeshHandle, RenderBackend, build_draw_calls};
pub use scene::{NodeId, NodeKind, SceneGraph, SceneNode, Transform};

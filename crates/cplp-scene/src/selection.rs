//! オブジェクト選択 & トランスフォームギズモ基盤
//!
//! クリック位置からレイを飛ばし、AABB（軸並行バウンディングボックス）で
//! ヒットテストを行う。選択されたオブジェクトにはハイライト表示用の
//! フラグを付与する。
//!
//! トランスフォームギズモはデータ構造とインターフェースのみ定義。

use crate::camera::{self, Camera};

// ── レイキャスト ────────────────────────────────

/// ワールド空間のレイ
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
}

impl Ray {
    /// スクリーン NDC 座標（-1..1）からワールド空間のレイを生成
    pub fn from_ndc(ndc: [f32; 2], camera: &Camera) -> Self {
        let view = camera.view();
        let proj = camera.projection();
        let vp = camera::mat4_mul(proj, view);

        // view-projection の逆行列
        let inv_vp = camera::mat4_inverse(vp).unwrap_or([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);

        // near plane と far plane 上の点を逆変換
        let near_pt = unproject([ndc[0], ndc[1], 0.0], inv_vp);
        let far_pt = unproject([ndc[0], ndc[1], 1.0], inv_vp);

        let dir = camera::normalize(camera::sub(far_pt, near_pt));

        Self {
            origin: near_pt,
            direction: dir,
        }
    }

    /// AABB とのヒットテスト（Slab 法）
    ///
    /// ヒットした場合は交差距離 t を返す。
    pub fn intersect_aabb(&self, aabb: &Aabb) -> Option<f32> {
        let mut t_min = f32::NEG_INFINITY;
        let mut t_max = f32::INFINITY;

        for i in 0..3 {
            if self.direction[i].abs() < 1e-8 {
                // レイが軸に平行 → 範囲外なら miss
                if self.origin[i] < aabb.min[i] || self.origin[i] > aabb.max[i] {
                    return None;
                }
            } else {
                let inv_d = 1.0 / self.direction[i];
                let mut t1 = (aabb.min[i] - self.origin[i]) * inv_d;
                let mut t2 = (aabb.max[i] - self.origin[i]) * inv_d;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                t_min = t_min.max(t1);
                t_max = t_max.min(t2);
                if t_min > t_max {
                    return None;
                }
            }
        }

        if t_max < 0.0 {
            None // AABB がレイの後方
        } else {
            Some(t_min.max(0.0))
        }
    }
}

/// NDC + depth をワールド座標に逆変換
fn unproject(ndc: [f32; 3], inv_vp: [[f32; 4]; 4]) -> [f32; 3] {
    let v = [ndc[0], ndc[1], ndc[2], 1.0];
    let mut result = [0.0f32; 4];
    for j in 0..4 {
        result[j] = inv_vp[0][j] * v[0]
            + inv_vp[1][j] * v[1]
            + inv_vp[2][j] * v[2]
            + inv_vp[3][j] * v[3];
    }
    let w = result[3];
    if w.abs() < 1e-10 {
        return [0.0; 3];
    }
    [result[0] / w, result[1] / w, result[2] / w]
}

// ── AABB ───────────────────────────────────────

/// 軸並行バウンディングボックス
#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    /// 中心と半径（half-extent）から生成
    pub fn from_center_half(center: [f32; 3], half: [f32; 3]) -> Self {
        Self {
            min: [center[0] - half[0], center[1] - half[1], center[2] - half[2]],
            max: [center[0] + half[0], center[1] + half[1], center[2] + half[2]],
        }
    }

    /// 中心を取得
    pub fn center(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
            (self.min[2] + self.max[2]) / 2.0,
        ]
    }
}

// ── 選択状態 ───────────────────────────────────

/// 選択可能なオブジェクトの情報
pub struct Selectable {
    /// オブジェクト名（デバッグ用）
    pub name: String,
    /// ワールド空間の AABB
    pub aabb: Aabb,
    /// MeshPipeline 内のオブジェクトインデックス
    pub object_index: usize,
}

/// 選択状態マネージャー
pub struct SelectionState {
    /// 選択可能オブジェクトの一覧
    pub selectables: Vec<Selectable>,
    /// 現在選択中のオブジェクトインデックス（selectables 内）
    pub selected: Option<usize>,
    /// ハイライト色の加算値（RGB）
    pub highlight_add: [f32; 3],
}

impl SelectionState {
    pub fn new() -> Self {
        Self {
            selectables: Vec::new(),
            selected: None,
            highlight_add: [0.15, 0.15, 0.25],
        }
    }

    /// 選択可能オブジェクトを登録
    pub fn add_selectable(&mut self, name: impl Into<String>, aabb: Aabb, object_index: usize) {
        self.selectables.push(Selectable {
            name: name.into(),
            aabb,
            object_index,
        });
    }

    /// レイによるピック — 最も手前のオブジェクトを選択
    pub fn pick(&mut self, ray: &Ray) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;

        for (i, sel) in self.selectables.iter().enumerate() {
            if let Some(t) = ray.intersect_aabb(&sel.aabb) {
                if best.map_or(true, |(_, bt)| t < bt) {
                    best = Some((i, t));
                }
            }
        }

        self.selected = best.map(|(i, _)| i);

        if let Some(idx) = self.selected {
            tracing::debug!(
                "Selected: '{}' (object_index={})",
                self.selectables[idx].name,
                self.selectables[idx].object_index,
            );
        } else {
            tracing::debug!("Selection cleared");
        }

        self.selected
    }

    /// 選択を解除
    pub fn clear(&mut self) {
        self.selected = None;
    }

    /// 現在選択中のオブジェクトの MeshPipeline インデックスを返す
    pub fn selected_object_index(&self) -> Option<usize> {
        self.selected
            .and_then(|i| self.selectables.get(i))
            .map(|s| s.object_index)
    }
}

impl Default for SelectionState {
    fn default() -> Self {
        Self::new()
    }
}

// ── トランスフォームギズモ（基盤）──────────────────

/// トランスフォーム操作の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformMode {
    /// 移動（Translate）— G キー
    Translate,
    /// 回転（Rotate）— R キー
    Rotate,
    /// スケール（Scale）— S キー
    Scale,
}

/// トランスフォーム軸の制約
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformAxis {
    /// 制約なし（スクリーン空間）
    Free,
    /// X 軸のみ
    X,
    /// Y 軸のみ
    Y,
    /// Z 軸のみ
    Z,
}

/// ギズモの状態
///
/// 現時点ではデータ構造とインターフェースのみ。
/// 描画・インタラクションは今後の Issue で実装する。
pub struct GizmoState {
    /// 現在のモード
    pub mode: TransformMode,
    /// 軸制約
    pub axis: TransformAxis,
    /// ギズモが操作中か
    pub active: bool,
    /// 操作開始時のオブジェクト位置
    pub start_position: [f32; 3],
    /// 操作開始時のオブジェクト回転（オイラー角、ラジアン）
    pub start_rotation: [f32; 3],
    /// 操作開始時のオブジェクトスケール
    pub start_scale: [f32; 3],
}

impl GizmoState {
    pub fn new() -> Self {
        Self {
            mode: TransformMode::Translate,
            axis: TransformAxis::Free,
            active: false,
            start_position: [0.0; 3],
            start_rotation: [0.0; 3],
            start_scale: [1.0; 3],
        }
    }

    /// ギズモを有効にして操作開始
    pub fn begin(&mut self, mode: TransformMode, position: [f32; 3]) {
        self.mode = mode;
        self.active = true;
        self.start_position = position;
        self.axis = TransformAxis::Free;
        tracing::debug!("Gizmo begin: {:?} at {:?}", mode, position);
    }

    /// 操作をキャンセル（Esc）
    pub fn cancel(&mut self) {
        self.active = false;
        tracing::debug!("Gizmo cancelled");
    }

    /// 操作を確定（クリック / Enter）
    pub fn confirm(&mut self) {
        self.active = false;
        tracing::debug!("Gizmo confirmed");
    }

    /// 軸制約を設定
    pub fn set_axis(&mut self, axis: TransformAxis) {
        self.axis = axis;
        tracing::debug!("Gizmo axis: {:?}", axis);
    }

    /// 移動量を計算（画面上のドラッグデルタ → ワールド空間のオフセット）
    ///
    /// 今後の Issue で実装する。現在はゼロベクトルを返す。
    pub fn compute_translation_delta(
        &self,
        _screen_delta: [f32; 2],
        _camera: &Camera,
    ) -> [f32; 3] {
        // TODO: カメラの向きと軸制約に基づいて計算
        [0.0, 0.0, 0.0]
    }
}

impl Default for GizmoState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_hit_test() {
        let aabb = Aabb::from_center_half([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);

        // 正面からのレイ → ヒット
        let ray = Ray {
            origin: [0.0, 0.0, 5.0],
            direction: [0.0, 0.0, -1.0],
        };
        assert!(ray.intersect_aabb(&aabb).is_some());

        // 横にずれたレイ → ミス
        let ray = Ray {
            origin: [3.0, 0.0, 5.0],
            direction: [0.0, 0.0, -1.0],
        };
        assert!(ray.intersect_aabb(&aabb).is_none());
    }

    #[test]
    fn aabb_hit_distance() {
        let aabb = Aabb::from_center_half([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let ray = Ray {
            origin: [0.0, 0.0, 5.0],
            direction: [0.0, 0.0, -1.0],
        };
        let t = ray.intersect_aabb(&aabb).unwrap();
        // AABB の +Z 面は z=1.0、origin は z=5.0 → t = 4.0
        assert!((t - 4.0).abs() < 0.01);
    }

    #[test]
    fn pick_nearest() {
        let mut state = SelectionState::new();
        state.add_selectable(
            "near",
            Aabb::from_center_half([0.0, 0.0, 2.0], [0.5, 0.5, 0.5]),
            0,
        );
        state.add_selectable(
            "far",
            Aabb::from_center_half([0.0, 0.0, -2.0], [0.5, 0.5, 0.5]),
            1,
        );

        let ray = Ray {
            origin: [0.0, 0.0, 5.0],
            direction: [0.0, 0.0, -1.0],
        };

        let picked = state.pick(&ray);
        assert_eq!(picked, Some(0)); // "near" が先にヒット
        assert_eq!(state.selected_object_index(), Some(0));
    }

    #[test]
    fn gizmo_lifecycle() {
        let mut gizmo = GizmoState::new();
        assert!(!gizmo.active);

        gizmo.begin(TransformMode::Translate, [1.0, 2.0, 3.0]);
        assert!(gizmo.active);
        assert_eq!(gizmo.mode, TransformMode::Translate);

        gizmo.set_axis(TransformAxis::X);
        assert_eq!(gizmo.axis, TransformAxis::X);

        gizmo.cancel();
        assert!(!gizmo.active);
    }
}

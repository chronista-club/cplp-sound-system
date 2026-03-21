//! PlacementEditor — ユーロラック 3D シーン上でモジュールをインタラクティブに配置するエディタ
//!
//! - マウスクリックによるモジュール選択（既存の AABB レイキャスト方式）
//! - ドラッグでモジュールを HP 単位でスナップ移動
//! - モジュールの追加・削除ロジック
//! - 空きスロットの可視化（ゴーストパネル）
//!
//! GPU 非依存のロジック層。`SceneRenderer` がこのモジュールを使って
//! MeshPipeline のオブジェクト位置やゴーストパネルを管理する。

use crate::mesh::{self, HP_UNIT, PANEL_DEPTH, PANEL_MARGIN, RAIL_THICKNESS, ROW_HEIGHT_3U};
use crate::scene_graph::MeshVertex;
use crate::selection::Aabb;

// ── モジュール定義 ──────────────────────────────

/// ラック上に配置されたモジュール
#[derive(Debug, Clone)]
pub struct PlacedModule {
    /// モジュール名
    pub name: String,
    /// HP 幅
    pub hp_width: u32,
    /// HP 位置（左端、0始まり）
    pub hp_pos: u32,
    /// 行番号（0始まり）
    pub row: u32,
    /// パネル色
    pub color: [f32; 3],
}

impl PlacedModule {
    /// AABB を計算（選択用）
    pub fn aabb(&self, rack_hp: u32) -> Aabb {
        let pos = mesh::module_world_position(self.hp_pos, self.hp_width, rack_hp, self.row);
        let width = self.hp_width as f32 * HP_UNIT - PANEL_MARGIN;
        let height = ROW_HEIGHT_3U - RAIL_THICKNESS * 2.0 - PANEL_MARGIN;
        let half = [width / 2.0, height / 2.0, PANEL_DEPTH / 2.0 + mesh::BEZEL_DEPTH];
        Aabb::from_center_half(pos, half)
    }

    /// ワールド座標を計算
    pub fn world_position(&self, rack_hp: u32) -> [f32; 3] {
        mesh::module_world_position(self.hp_pos, self.hp_width, rack_hp, self.row)
    }
}

/// ドラッグ中の状態
#[derive(Debug, Clone)]
struct DragState {
    /// ドラッグ中のモジュールインデックス
    module_index: usize,
    /// ドラッグ開始時のマウス位置（ピクセル座標）
    start_pixel: [f32; 2],
    /// ドラッグ開始時のモジュール HP 位置
    start_hp_pos: u32,
    /// ドラッグ開始時のモジュール行
    start_row: u32,
}

/// 空きスロット（ゴーストパネル表示用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptySlot {
    pub hp_pos: u32,
    pub hp_width: u32,
    pub row: u32,
}

// ── PlacementEditor ──────────────────────────────

/// ユーロラックモジュール配置エディタ
///
/// GPU 非依存。モジュール配置ロジック・衝突判定・空きスロット計算を担当。
/// 描画は `SceneRenderer` が `MeshPipeline` 経由で行う。
pub struct PlacementEditor {
    /// ラック全体の HP 幅
    pub rack_hp: u32,
    /// ラックの行数
    pub rack_rows: u32,
    /// 配置済みモジュール
    modules: Vec<PlacedModule>,
    /// 現在選択中のモジュールインデックス
    selected: Option<usize>,
    /// ドラッグ中の状態
    drag: Option<DragState>,
    /// ビューポートサイズ（ピクセル）
    viewport_width: u32,
    viewport_height: u32,
    /// モジュール配置が変更されたか（再描画トリガー）
    dirty: bool,
}

impl PlacementEditor {
    /// 新しいエディタを作成
    pub fn new(rack_hp: u32, rack_rows: u32, width: u32, height: u32) -> Self {
        Self {
            rack_hp,
            rack_rows,
            modules: Vec::new(),
            selected: None,
            drag: None,
            viewport_width: width,
            viewport_height: height,
            dirty: true,
        }
    }

    // ── モジュール管理 ──────────────────────────────

    /// モジュールを追加。衝突がなければ Ok(index) を返す。
    pub fn add_module(&mut self, module: PlacedModule) -> Result<usize, AddError> {
        // 範囲チェック
        if module.hp_pos + module.hp_width > self.rack_hp {
            return Err(AddError::OutOfBounds);
        }
        if module.row >= self.rack_rows {
            return Err(AddError::OutOfBounds);
        }
        // 衝突チェック
        if self.collides_with_any(&module, None) {
            return Err(AddError::Collision);
        }
        let idx = self.modules.len();
        self.modules.push(module);
        self.dirty = true;
        Ok(idx)
    }

    /// モジュールを削除
    pub fn remove_module(&mut self, index: usize) -> Option<PlacedModule> {
        if index >= self.modules.len() {
            return None;
        }
        // 選択状態をクリア
        if self.selected == Some(index) {
            self.selected = None;
        } else if let Some(sel) = self.selected {
            if sel > index {
                self.selected = Some(sel - 1);
            }
        }
        self.drag = None;
        self.dirty = true;
        Some(self.modules.remove(index))
    }

    /// 選択中のモジュールを削除
    pub fn remove_selected(&mut self) -> Option<PlacedModule> {
        let idx = self.selected?;
        self.remove_module(idx)
    }

    /// 配置済みモジュール一覧
    pub fn modules(&self) -> &[PlacedModule] {
        &self.modules
    }

    /// 選択中のモジュールインデックス
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// 選択を解除
    pub fn deselect(&mut self) {
        self.selected = None;
    }

    /// モジュールを選択
    pub fn select(&mut self, index: usize) {
        if index < self.modules.len() {
            self.selected = Some(index);
        }
    }

    /// ドラッグ中か
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// dirty フラグ（再描画が必要か）を取得してリセット
    pub fn take_dirty(&mut self) -> bool {
        let d = self.dirty;
        self.dirty = false;
        d
    }

    // ── HP スナップ計算 ──────────────────────────────

    /// ピクセル差分から HP オフセットを計算
    ///
    /// カメラの FOV・アスペクト比・距離から 1HP が画面上で何ピクセルかを概算する。
    fn pixel_delta_to_hp(&self, dx_pixels: f32, fov_y: f32, aspect: f32, eye_z: f32, target_z: f32) -> i32 {
        let distance = (eye_z - target_z).abs();
        let viewport_world_width = 2.0 * distance * (fov_y / 2.0).tan() * aspect;
        let pixels_per_world = self.viewport_width as f32 / viewport_world_width;
        let pixels_per_hp = HP_UNIT * pixels_per_world;
        if pixels_per_hp < 0.001 {
            return 0;
        }
        (dx_pixels / pixels_per_hp).round() as i32
    }

    /// ピクセル差分から行オフセットを計算
    fn pixel_delta_to_row(&self, dy_pixels: f32, fov_y: f32, eye_z: f32, target_z: f32) -> i32 {
        let distance = (eye_z - target_z).abs();
        let viewport_world_height = 2.0 * distance * (fov_y / 2.0).tan();
        let pixels_per_world = self.viewport_height as f32 / viewport_world_height;
        let pixels_per_row = ROW_HEIGHT_3U * pixels_per_world;
        if pixels_per_row < 0.001 {
            return 0;
        }
        // Y 軸は上がプラスだが画面座標は下がプラスなので反転
        (-dy_pixels / pixels_per_row).round() as i32
    }

    /// HP 位置をクランプ
    fn clamp_hp_pos(hp_pos: i32, hp_width: u32, rack_hp: u32) -> u32 {
        hp_pos.max(0).min((rack_hp - hp_width) as i32) as u32
    }

    /// 行をクランプ
    fn clamp_row(row: i32, rack_rows: u32) -> u32 {
        row.max(0).min((rack_rows - 1) as i32) as u32
    }

    // ── 衝突判定 ──────────────────────────────────

    /// 指定モジュールが他のモジュールと衝突するか（exclude_index を除外）
    fn collides_with_any(&self, module: &PlacedModule, exclude_index: Option<usize>) -> bool {
        for (i, other) in self.modules.iter().enumerate() {
            if Some(i) == exclude_index {
                continue;
            }
            if other.row != module.row {
                continue;
            }
            // HP 範囲が重なるか
            let a_start = module.hp_pos;
            let a_end = module.hp_pos + module.hp_width;
            let b_start = other.hp_pos;
            let b_end = other.hp_pos + other.hp_width;
            if a_start < b_end && b_start < a_end {
                return true;
            }
        }
        false
    }

    // ── 空きスロット計算 ──────────────────────────────

    /// 各行の空きスロットを計算（ゴーストパネル表示用）
    pub fn empty_slots(&self) -> Vec<EmptySlot> {
        let mut slots = Vec::new();

        for row in 0..self.rack_rows {
            // この行のモジュールを HP 位置順にソート
            let mut row_modules: Vec<&PlacedModule> = self
                .modules
                .iter()
                .filter(|m| m.row == row)
                .collect();
            row_modules.sort_by_key(|m| m.hp_pos);

            let mut cursor: u32 = 0;
            for m in &row_modules {
                if m.hp_pos > cursor {
                    slots.push(EmptySlot {
                        hp_pos: cursor,
                        hp_width: m.hp_pos - cursor,
                        row,
                    });
                }
                cursor = m.hp_pos + m.hp_width;
            }
            if cursor < self.rack_hp {
                slots.push(EmptySlot {
                    hp_pos: cursor,
                    hp_width: self.rack_hp - cursor,
                    row,
                });
            }
        }

        slots
    }

    // ── マウスイベント処理 ──────────────────────────────

    /// マウスボタン押下 — ドラッグ開始
    pub fn on_mouse_down(&mut self, x: f32, y: f32) {
        if let Some(selected) = self.selected {
            if selected < self.modules.len() {
                let m = &self.modules[selected];
                self.drag = Some(DragState {
                    module_index: selected,
                    start_pixel: [x, y],
                    start_hp_pos: m.hp_pos,
                    start_row: m.row,
                });
            }
        }
    }

    /// マウス移動 — ドラッグ中ならモジュールをスナップ移動
    ///
    /// カメラパラメータは呼び出し側（`SceneRenderer`）から渡す。
    pub fn on_mouse_move(
        &mut self,
        x: f32,
        y: f32,
        fov_y: f32,
        aspect: f32,
        eye_z: f32,
        target_z: f32,
    ) {
        let Some(drag) = &self.drag else { return };

        let dx = x - drag.start_pixel[0];
        let dy = y - drag.start_pixel[1];

        let hp_offset = self.pixel_delta_to_hp(dx, fov_y, aspect, eye_z, target_z);
        let row_offset = self.pixel_delta_to_row(dy, fov_y, eye_z, target_z);

        let module_index = drag.module_index;
        let start_hp = drag.start_hp_pos;
        let start_row = drag.start_row;
        let hp_width = self.modules[module_index].hp_width;

        let new_hp =
            Self::clamp_hp_pos(start_hp as i32 + hp_offset, hp_width, self.rack_hp);
        let new_row =
            Self::clamp_row(start_row as i32 + row_offset, self.rack_rows);

        // 仮に移動してみて衝突判定
        let candidate = PlacedModule {
            name: self.modules[module_index].name.clone(),
            hp_width,
            hp_pos: new_hp,
            row: new_row,
            color: self.modules[module_index].color,
        };

        if !self.collides_with_any(&candidate, Some(module_index)) {
            let prev_pos = self.modules[module_index].hp_pos;
            let prev_row = self.modules[module_index].row;
            self.modules[module_index].hp_pos = new_hp;
            self.modules[module_index].row = new_row;

            if new_hp != prev_pos || new_row != prev_row {
                self.dirty = true;
            }
        }
    }

    /// マウスボタン解放 — ドラッグ終了
    pub fn on_mouse_up(&mut self) {
        self.drag = None;
    }

    // ── リサイズ ──────────────────────────────────

    /// ビューポートサイズを更新
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.viewport_width = width;
            self.viewport_height = height;
        }
    }

    // ── メッシュ生成ヘルパー ──────────────────────────

    /// 全モジュールのワールド座標を返す
    pub fn module_positions(&self) -> Vec<[f32; 3]> {
        self.modules
            .iter()
            .map(|m| mesh::module_world_position(m.hp_pos, m.hp_width, self.rack_hp, m.row))
            .collect()
    }

    /// ゴーストパネル（空きスロット表示）のメッシュデータを生成
    pub fn build_ghost_meshes(&self) -> Vec<(Vec<MeshVertex>, Vec<u32>, [f32; 3])> {
        let ghost_color = [0.2, 0.2, 0.24];

        self.empty_slots()
            .iter()
            .map(|slot| {
                let (verts, idxs) = build_ghost_panel(slot.hp_width, ghost_color);
                let pos = mesh::module_world_position(
                    slot.hp_pos,
                    slot.hp_width,
                    self.rack_hp,
                    slot.row,
                );
                (verts, idxs, pos)
            })
            .collect()
    }
}

// ── ゴーストパネル ──────────────────────────────

/// 半透明（低彩度）のゴーストパネルメッシュを生成
///
/// 通常の `build_module_panel` と同じ形状だが、色を薄くして空きスロットを表現。
/// ベゼルなしのシンプルなフラットパネル。
pub fn build_ghost_panel(hp_width: u32, color: [f32; 3]) -> (Vec<MeshVertex>, Vec<u32>) {
    let width = hp_width as f32 * HP_UNIT - PANEL_MARGIN;
    let height = ROW_HEIGHT_3U - RAIL_THICKNESS * 2.0 - PANEL_MARGIN;
    let dark = [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5];

    // シンプルなボックス（ベゼルなし、奥行き薄め）
    let depth = PANEL_DEPTH * 0.5;
    mesh::build_box(width, height, depth, color, dark)
}

// ── エラー型 ──────────────────────────────────

/// モジュール追加エラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    /// ラック範囲外
    OutOfBounds,
    /// 他のモジュールと衝突
    Collision,
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddError::OutOfBounds => write!(f, "モジュールがラック範囲外です"),
            AddError::Collision => write!(f, "他のモジュールと衝突しています"),
        }
    }
}

impl std::error::Error for AddError {}

// ── テスト ────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_module(name: &str, hp_width: u32, hp_pos: u32, row: u32) -> PlacedModule {
        PlacedModule {
            name: name.to_string(),
            hp_width,
            hp_pos,
            row,
            color: [0.5, 0.5, 0.5],
        }
    }

    #[test]
    fn add_and_remove_modules() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        let idx = editor
            .add_module(make_module("Looper", 16, 0, 0))
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(editor.modules().len(), 1);

        let removed = editor.remove_module(0).unwrap();
        assert_eq!(removed.name, "Looper");
        assert_eq!(editor.modules().len(), 0);
    }

    #[test]
    fn collision_detection() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 16, 0, 0)).unwrap();

        // 完全に重なる
        assert_eq!(
            editor.add_module(make_module("B", 16, 0, 0)),
            Err(AddError::Collision)
        );
        // 部分的に重なる
        assert_eq!(
            editor.add_module(make_module("C", 8, 8, 0)),
            Err(AddError::Collision)
        );
        // 隣接（衝突しない）
        assert!(editor.add_module(make_module("D", 8, 16, 0)).is_ok());
        // 別の行（衝突しない）
        assert!(editor.add_module(make_module("E", 16, 0, 1)).is_ok());
    }

    #[test]
    fn out_of_bounds() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        assert_eq!(
            editor.add_module(make_module("X", 16, 80, 0)),
            Err(AddError::OutOfBounds)
        );
        assert_eq!(
            editor.add_module(make_module("Y", 8, 0, 2)),
            Err(AddError::OutOfBounds)
        );
    }

    #[test]
    fn empty_slots_calculation() {
        let mut editor = PlacementEditor::new(84, 1, 1280, 720);
        editor.add_module(make_module("A", 16, 10, 0)).unwrap();
        editor.add_module(make_module("B", 8, 40, 0)).unwrap();

        let slots = editor.empty_slots();
        assert_eq!(slots.len(), 3);

        // 0..10
        assert_eq!(slots[0].hp_pos, 0);
        assert_eq!(slots[0].hp_width, 10);
        // 26..40
        assert_eq!(slots[1].hp_pos, 26);
        assert_eq!(slots[1].hp_width, 14);
        // 48..84
        assert_eq!(slots[2].hp_pos, 48);
        assert_eq!(slots[2].hp_width, 36);
    }

    #[test]
    fn empty_slots_full_rack() {
        let mut editor = PlacementEditor::new(84, 1, 1280, 720);
        editor.add_module(make_module("Full", 84, 0, 0)).unwrap();
        let slots = editor.empty_slots();
        assert!(slots.is_empty());
    }

    #[test]
    fn empty_slots_empty_rack() {
        let editor = PlacementEditor::new(84, 2, 1280, 720);
        let slots = editor.empty_slots();
        assert_eq!(slots.len(), 2); // 各行に 1 つの空きスロット
        assert_eq!(slots[0].hp_width, 84);
        assert_eq!(slots[1].hp_width, 84);
    }

    #[test]
    fn hp_clamp() {
        assert_eq!(PlacementEditor::clamp_hp_pos(-5, 8, 84), 0);
        assert_eq!(PlacementEditor::clamp_hp_pos(80, 8, 84), 76);
        assert_eq!(PlacementEditor::clamp_hp_pos(10, 8, 84), 10);
    }

    #[test]
    fn selection_and_deselection() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 0, 0)).unwrap();
        editor.add_module(make_module("B", 8, 8, 0)).unwrap();

        assert_eq!(editor.selected(), None);

        editor.select(0);
        assert_eq!(editor.selected(), Some(0));

        editor.deselect();
        assert_eq!(editor.selected(), None);
    }

    #[test]
    fn remove_selected_module() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 0, 0)).unwrap();
        editor.add_module(make_module("B", 8, 8, 0)).unwrap();

        editor.select(1);
        let removed = editor.remove_selected().unwrap();
        assert_eq!(removed.name, "B");
        assert_eq!(editor.selected(), None);
        assert_eq!(editor.modules().len(), 1);
    }

    #[test]
    fn remove_adjusts_selection() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 0, 0)).unwrap();
        editor.add_module(make_module("B", 8, 8, 0)).unwrap();
        editor.add_module(make_module("C", 8, 16, 0)).unwrap();

        editor.select(2); // C を選択
        editor.remove_module(0); // A を削除 → 選択は 2→1 にずれる
        assert_eq!(editor.selected(), Some(1));
        assert_eq!(editor.modules()[1].name, "C");
    }

    #[test]
    fn dirty_flag() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        assert!(editor.take_dirty()); // 初期は dirty

        assert!(!editor.take_dirty()); // リセットされた

        editor.add_module(make_module("A", 8, 0, 0)).unwrap();
        assert!(editor.take_dirty()); // 追加で dirty

        editor.remove_module(0);
        assert!(editor.take_dirty()); // 削除で dirty
    }

    #[test]
    fn ghost_panel_mesh_generation() {
        let (verts, indices) = build_ghost_panel(8, [0.3, 0.3, 0.35]);
        // ボックスメッシュ: 24 頂点、36 インデックス
        assert_eq!(verts.len(), 24);
        assert_eq!(indices.len(), 36);
    }

    #[test]
    fn module_aabb() {
        let m = make_module("Test", 16, 0, 0);
        let aabb = m.aabb(84);
        // center は module_world_position と一致
        let pos = m.world_position(84);
        let center = aabb.center();
        for i in 0..3 {
            assert!((center[i] - pos[i]).abs() < 0.01);
        }
        // AABB は正の体積を持つ
        for i in 0..3 {
            assert!(aabb.max[i] > aabb.min[i]);
        }
    }

    // ── ドラッグ操作テスト (Phase 2b) ──────────────────

    /// カメラパラメータのデフォルト値（テスト用）
    /// FOV 45°, アスペクト比 16:9, カメラ Z=5.0, ターゲット Z=0.0
    const TEST_FOV: f32 = std::f32::consts::FRAC_PI_4; // 45°
    const TEST_ASPECT: f32 = 1280.0 / 720.0;
    const TEST_EYE_Z: f32 = 5.0;
    const TEST_TARGET_Z: f32 = 0.0;

    /// on_mouse_down → on_mouse_move で HP 単位スナップ移動を確認
    #[test]
    fn drag_move_hp_snap() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 10, 0)).unwrap();
        editor.select(0);

        // ドラッグ開始
        editor.on_mouse_down(100.0, 100.0);
        assert!(editor.is_dragging());

        // 1HP 分のピクセル移動量を計算
        // viewport_world_width = 2 * 5.0 * tan(PI/8) * (1280/720)
        let distance = TEST_EYE_Z - TEST_TARGET_Z;
        let viewport_world_width =
            2.0 * distance * (TEST_FOV / 2.0).tan() * TEST_ASPECT;
        let pixels_per_world = 1280.0 / viewport_world_width;
        let pixels_per_hp = HP_UNIT * pixels_per_world;

        // 右に 3HP 分ドラッグ
        let dx = pixels_per_hp * 3.0;
        editor.on_mouse_move(
            100.0 + dx,
            100.0,
            TEST_FOV,
            TEST_ASPECT,
            TEST_EYE_Z,
            TEST_TARGET_Z,
        );

        assert_eq!(editor.modules()[0].hp_pos, 13); // 10 + 3
    }

    /// ドラッグ先に他モジュールがある場合、移動しない
    #[test]
    fn drag_prevents_collision() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 0, 0)).unwrap();
        editor.add_module(make_module("B", 8, 8, 0)).unwrap(); // A の隣
        editor.select(0);

        editor.on_mouse_down(100.0, 100.0);

        // 1HP 分のピクセルを計算して右に大きくドラッグ（B と衝突する位置へ）
        let distance = (TEST_EYE_Z - TEST_TARGET_Z).abs();
        let viewport_world_width =
            2.0 * distance * (TEST_FOV / 2.0).tan() * TEST_ASPECT;
        let pixels_per_world = 1280.0 / viewport_world_width;
        let pixels_per_hp = HP_UNIT * pixels_per_world;

        // 右に 5HP（hp_pos=5 にしたい → B(8..16) と衝突）
        let dx = pixels_per_hp * 5.0;
        editor.on_mouse_move(
            100.0 + dx,
            100.0,
            TEST_FOV,
            TEST_ASPECT,
            TEST_EYE_Z,
            TEST_TARGET_Z,
        );

        // 衝突するので元の位置のまま
        assert_eq!(editor.modules()[0].hp_pos, 0);
    }

    /// ラック外へドラッグしても範囲内にクランプされる
    #[test]
    fn drag_clamps_to_rack_bounds() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 70, 0)).unwrap();
        editor.select(0);

        editor.on_mouse_down(500.0, 100.0);

        // 右に大量ドラッグ（ラック外へ）
        let distance = (TEST_EYE_Z - TEST_TARGET_Z).abs();
        let viewport_world_width =
            2.0 * distance * (TEST_FOV / 2.0).tan() * TEST_ASPECT;
        let pixels_per_world = 1280.0 / viewport_world_width;
        let pixels_per_hp = HP_UNIT * pixels_per_world;

        let dx = pixels_per_hp * 100.0; // 100HP 分
        editor.on_mouse_move(
            500.0 + dx,
            100.0,
            TEST_FOV,
            TEST_ASPECT,
            TEST_EYE_Z,
            TEST_TARGET_Z,
        );

        // 84 - 8 = 76 が最大 hp_pos
        assert_eq!(editor.modules()[0].hp_pos, 76);
    }

    /// Y 方向ドラッグで行が変わる
    #[test]
    fn drag_move_changes_row() {
        let mut editor = PlacementEditor::new(84, 3, 1280, 720);
        editor.add_module(make_module("A", 8, 0, 0)).unwrap();
        editor.select(0);

        editor.on_mouse_down(100.0, 400.0);

        // 行の高さ分ピクセル移動を計算（Y軸は画面座標が下がプラスで反転）
        let distance = (TEST_EYE_Z - TEST_TARGET_Z).abs();
        let viewport_world_height = 2.0 * distance * (TEST_FOV / 2.0).tan();
        let pixels_per_world = 720.0 / viewport_world_height;
        let pixels_per_row = ROW_HEIGHT_3U * pixels_per_world;

        // 上にドラッグ（画面座標では負方向 → row が増える）
        let dy = -pixels_per_row * 1.0;
        editor.on_mouse_move(
            100.0,
            400.0 + dy,
            TEST_FOV,
            TEST_ASPECT,
            TEST_EYE_Z,
            TEST_TARGET_Z,
        );

        assert_eq!(editor.modules()[0].row, 1);
    }

    /// on_mouse_up() 後に is_dragging() が false
    #[test]
    fn on_mouse_up_clears_drag() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 0, 0)).unwrap();
        editor.select(0);

        editor.on_mouse_down(100.0, 100.0);
        assert!(editor.is_dragging());

        editor.on_mouse_up();
        assert!(!editor.is_dragging());
    }

    /// 未選択状態でドラッグしても何も起きない
    #[test]
    fn drag_without_selection_is_noop() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 10, 0)).unwrap();
        // select しない

        editor.on_mouse_down(100.0, 100.0);
        assert!(!editor.is_dragging());

        editor.on_mouse_move(200.0, 100.0, TEST_FOV, TEST_ASPECT, TEST_EYE_Z, TEST_TARGET_Z);
        assert_eq!(editor.modules()[0].hp_pos, 10); // 変化なし
    }

    /// eye_z == target_z のときゼロ除算にならずゼロを返す
    #[test]
    fn pixel_delta_to_hp_zero_distance_returns_zero() {
        let editor = PlacementEditor::new(84, 2, 1280, 720);
        // pixel_delta_to_hp は private なので、ドラッグ操作経由で確認
        // eye_z == target_z → distance=0 → viewport_world_width=0 → pixels_per_hp≈0 → 0を返す
        let mut editor = editor;
        editor.add_module(make_module("A", 8, 10, 0)).unwrap();
        editor.select(0);
        editor.on_mouse_down(100.0, 100.0);

        // eye_z == target_z
        editor.on_mouse_move(300.0, 100.0, TEST_FOV, TEST_ASPECT, 0.0, 0.0);
        // ゼロ除算でパニックしないこと＆位置が変わらないこと
        assert_eq!(editor.modules()[0].hp_pos, 10);
    }

    /// resize(0, 0) でビューポートが変わらない
    #[test]
    fn resize_zero_values_ignored() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.resize(0, 0);
        // viewport はアクセサがないので、ドラッグのピクセル計算が壊れないか間接的に確認
        // resize(0,0) は無視されるので、元の 1280x720 のまま
        editor.add_module(make_module("A", 8, 10, 0)).unwrap();
        editor.select(0);
        editor.on_mouse_down(100.0, 100.0);

        let distance = (TEST_EYE_Z - TEST_TARGET_Z).abs();
        let viewport_world_width =
            2.0 * distance * (TEST_FOV / 2.0).tan() * TEST_ASPECT;
        let pixels_per_world = 1280.0 / viewport_world_width;
        let pixels_per_hp = HP_UNIT * pixels_per_world;

        let dx = pixels_per_hp * 2.0;
        editor.on_mouse_move(
            100.0 + dx,
            100.0,
            TEST_FOV,
            TEST_ASPECT,
            TEST_EYE_Z,
            TEST_TARGET_Z,
        );

        // 正常にスナップ移動する（viewport が 0 になっていれば壊れる）
        assert_eq!(editor.modules()[0].hp_pos, 12);
    }

    /// module_positions() が各モジュールの world_position() と一致する
    #[test]
    fn module_positions_returns_correct_world_coords() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 8, 0, 0)).unwrap();
        editor.add_module(make_module("B", 16, 20, 1)).unwrap();

        let positions = editor.module_positions();
        assert_eq!(positions.len(), 2);

        for (i, m) in editor.modules().iter().enumerate() {
            let expected = m.world_position(84);
            for axis in 0..3 {
                assert!(
                    (positions[i][axis] - expected[axis]).abs() < 1e-6,
                    "module {} axis {} mismatch: {} vs {}",
                    i,
                    axis,
                    positions[i][axis],
                    expected[axis]
                );
            }
        }
    }

    /// build_ghost_meshes().len() == empty_slots().len()
    #[test]
    fn build_ghost_meshes_matches_empty_slots() {
        let mut editor = PlacementEditor::new(84, 2, 1280, 720);
        editor.add_module(make_module("A", 16, 10, 0)).unwrap();
        editor.add_module(make_module("B", 8, 40, 1)).unwrap();

        let ghosts = editor.build_ghost_meshes();
        let slots = editor.empty_slots();
        assert_eq!(
            ghosts.len(),
            slots.len(),
            "ghost meshes ({}) should match empty slots ({})",
            ghosts.len(),
            slots.len()
        );

        // 各ゴーストメッシュが有効なデータを持つ
        for (verts, indices, pos) in &ghosts {
            assert!(!verts.is_empty(), "ghost mesh vertices should not be empty");
            assert!(!indices.is_empty(), "ghost mesh indices should not be empty");
            // 位置は有限値
            for &v in pos {
                assert!(v.is_finite(), "ghost position should be finite");
            }
        }
    }
}

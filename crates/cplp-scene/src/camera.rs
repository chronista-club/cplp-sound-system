/// 3D パースペクティブカメラ
///
/// 右手座標系、Y-up。USD の upAxis = "Y" に合わせる。
pub struct Camera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_y: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn new(aspect: f32) -> Self {
        Self {
            eye: [5.0, 4.0, 8.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y: 45.0_f32.to_radians(),
            aspect,
            near: 0.1,
            far: 100.0,
        }
    }

    /// ユーロラック正面から見るカメラ
    pub fn rack_view(aspect: f32) -> Self {
        Self {
            eye: [0.0, 1.28, 5.0],
            target: [0.0, 1.28, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y: 45.0_f32.to_radians(),
            aspect,
            near: 0.1,
            far: 100.0,
        }
    }

    /// View-Projection 行列を計算（列優先、wgpu 用）
    pub fn view_proj(&self) -> [[f32; 4]; 4] {
        let view = look_at(self.eye, self.target, self.up);
        let proj = perspective(self.fov_y, self.aspect, self.near, self.far);
        mat4_mul(proj, view)
    }

    /// View 行列のみ取得
    pub fn view(&self) -> [[f32; 4]; 4] {
        look_at(self.eye, self.target, self.up)
    }

    /// Projection 行列のみ取得
    pub fn projection(&self) -> [[f32; 4]; 4] {
        perspective(self.fov_y, self.aspect, self.near, self.far)
    }

    pub fn set_aspect(&mut self, width: u32, height: u32) {
        if height > 0 {
            self.aspect = width as f32 / height as f32;
        }
    }
}

// ── Orbit カメラコントローラー ──────────────────────

/// Orbit（周回）カメラコントローラー
///
/// target を中心にドラッグで回転、スクロールでズーム、中クリックでパン。
/// 球面座標 (yaw, pitch, distance) で管理する。
pub struct OrbitController {
    /// 水平回転角度（ラジアン）
    pub yaw: f32,
    /// 垂直回転角度（ラジアン）— クランプされる
    pub pitch: f32,
    /// ターゲットからの距離
    pub distance: f32,
    /// 注視点
    pub target: [f32; 3],

    /// ドラッグ感度（回転）
    pub rotate_speed: f32,
    /// パン感度
    pub pan_speed: f32,
    /// ズーム感度
    pub zoom_speed: f32,
    /// 最小距離
    pub min_distance: f32,
    /// 最大距離
    pub max_distance: f32,
    /// ピッチの最小値（ラジアン）
    pub min_pitch: f32,
    /// ピッチの最大値（ラジアン）
    pub max_pitch: f32,
}

impl OrbitController {
    /// Camera の現在の状態から OrbitController を初期化
    pub fn from_camera(camera: &Camera) -> Self {
        let dx = camera.eye[0] - camera.target[0];
        let dy = camera.eye[1] - camera.target[1];
        let dz = camera.eye[2] - camera.target[2];
        let distance = (dx * dx + dy * dy + dz * dz).sqrt();

        let pitch = if distance > 0.0 {
            (dy / distance).asin()
        } else {
            0.0
        };

        let yaw = dz.atan2(dx);

        Self {
            yaw,
            pitch,
            distance,
            target: camera.target,
            rotate_speed: 0.005,
            pan_speed: 0.005,
            zoom_speed: 0.1,
            min_distance: 0.5,
            max_distance: 50.0,
            min_pitch: -std::f32::consts::FRAC_PI_2 + 0.01,
            max_pitch: std::f32::consts::FRAC_PI_2 - 0.01,
        }
    }

    /// マウスドラッグによる回転（左ボタン）
    pub fn rotate(&mut self, delta_x: f32, delta_y: f32) {
        self.yaw -= delta_x * self.rotate_speed;
        self.pitch += delta_y * self.rotate_speed;
        self.pitch = self.pitch.clamp(self.min_pitch, self.max_pitch);
    }

    /// マウスドラッグによるパン（中ボタン）
    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        // カメラのローカル右方向・上方向に沿ってパン
        let cos_yaw = self.yaw.cos();
        let sin_yaw = self.yaw.sin();
        let cos_pitch = self.pitch.cos();

        // 右方向（XZ 平面上）
        let right = [-sin_yaw, 0.0, cos_yaw];
        // 上方向（カメラのローカル up）
        let up = [
            -cos_yaw * self.pitch.sin(),
            cos_pitch,
            -sin_yaw * self.pitch.sin(),
        ];

        let scale = self.distance * self.pan_speed;
        for i in 0..3 {
            self.target[i] -= right[i] * delta_x * scale;
            self.target[i] += up[i] * delta_y * scale;
        }
    }

    /// スクロールによるズーム
    pub fn zoom(&mut self, delta: f32) {
        self.distance *= 1.0 - delta * self.zoom_speed;
        self.distance = self.distance.clamp(self.min_distance, self.max_distance);
    }

    /// 現在のコントローラー状態を Camera に反映
    pub fn apply(&self, camera: &mut Camera) {
        let cos_pitch = self.pitch.cos();
        let sin_pitch = self.pitch.sin();
        let cos_yaw = self.yaw.cos();
        let sin_yaw = self.yaw.sin();

        camera.eye = [
            self.target[0] + self.distance * cos_pitch * cos_yaw,
            self.target[1] + self.distance * sin_pitch,
            self.target[2] + self.distance * cos_pitch * sin_yaw,
        ];
        camera.target = self.target;
        camera.up = [0.0, 1.0, 0.0];
    }
}

// ── 行列演算（外部クレート不使用）─────────────────

pub(crate) fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);

    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// wgpu 用パースペクティブ行列（NDC z: [0, 1]）
pub(crate) fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y / 2.0).tan();
    let range_inv = 1.0 / (near - far);

    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far * range_inv, -1.0],
        [0.0, 0.0, near * far * range_inv, 0.0],
    ]
}

pub(crate) fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = a[0][j] * b[i][0] + a[1][j] * b[i][1] + a[2][j] * b[i][2] + a[3][j] * b[i][3];
        }
    }
    out
}

/// 4x4 行列の逆行列（selection のレイキャスト用）
pub(crate) fn mat4_inverse(m: [[f32; 4]; 4]) -> Option<[[f32; 4]; 4]> {
    // Cramer's rule による 4x4 逆行列
    let a = m[0];
    let b = m[1];
    let c = m[2];
    let d = m[3];

    let s0 = a[0] * b[1] - b[0] * a[1];
    let s1 = a[0] * b[2] - b[0] * a[2];
    let s2 = a[0] * b[3] - b[0] * a[3];
    let s3 = a[1] * b[2] - b[1] * a[2];
    let s4 = a[1] * b[3] - b[1] * a[3];
    let s5 = a[2] * b[3] - b[2] * a[3];

    let c5 = c[2] * d[3] - d[2] * c[3];
    let c4 = c[1] * d[3] - d[1] * c[3];
    let c3 = c[1] * d[2] - d[1] * c[2];
    let c2 = c[0] * d[3] - d[0] * c[3];
    let c1 = c[0] * d[2] - d[0] * c[2];
    let c0 = c[0] * d[1] - d[0] * c[1];

    let det = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
    if det.abs() < 1e-10 {
        return None;
    }
    let inv_det = 1.0 / det;

    Some([
        [
            (b[1] * c5 - b[2] * c4 + b[3] * c3) * inv_det,
            (-a[1] * c5 + a[2] * c4 - a[3] * c3) * inv_det,
            (d[1] * s5 - d[2] * s4 + d[3] * s3) * inv_det,
            (-c[1] * s5 + c[2] * s4 - c[3] * s3) * inv_det,
        ],
        [
            (-b[0] * c5 + b[2] * c2 - b[3] * c1) * inv_det,
            (a[0] * c5 - a[2] * c2 + a[3] * c1) * inv_det,
            (-d[0] * s5 + d[2] * s2 - d[3] * s1) * inv_det,
            (c[0] * s5 - c[2] * s2 + c[3] * s1) * inv_det,
        ],
        [
            (b[0] * c4 - b[1] * c2 + b[3] * c0) * inv_det,
            (-a[0] * c4 + a[1] * c2 - a[3] * c0) * inv_det,
            (d[0] * s4 - d[1] * s2 + d[3] * s0) * inv_det,
            (-c[0] * s4 + c[1] * s2 - c[3] * s0) * inv_det,
        ],
        [
            (-b[0] * c3 + b[1] * c1 - b[2] * c0) * inv_det,
            (a[0] * c3 - a[1] * c1 + a[2] * c0) * inv_det,
            (-d[0] * s3 + d[1] * s1 - d[2] * s0) * inv_det,
            (c[0] * s3 - c[1] * s1 + c[2] * s0) * inv_det,
        ],
    ])
}

pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn _add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn _scale(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

pub(crate) fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = length(v);
    if len == 0.0 {
        return [0.0; 3];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_controller_roundtrip() {
        let camera = Camera::rack_view(16.0 / 9.0);
        let original_eye = camera.eye;
        let original_target = camera.target;

        let controller = OrbitController::from_camera(&camera);
        let mut camera2 = Camera::rack_view(16.0 / 9.0);
        controller.apply(&mut camera2);

        // eye と target が概ね一致すること
        for i in 0..3 {
            assert!((camera2.eye[i] - original_eye[i]).abs() < 0.01,
                "eye[{i}]: expected {}, got {}", original_eye[i], camera2.eye[i]);
            assert!((camera2.target[i] - original_target[i]).abs() < 0.01,
                "target[{i}]: expected {}, got {}", original_target[i], camera2.target[i]);
        }
    }

    #[test]
    fn orbit_zoom_clamps() {
        let camera = Camera::new(1.0);
        let mut ctrl = OrbitController::from_camera(&camera);
        // 大量にズームイン
        for _ in 0..100 {
            ctrl.zoom(1.0);
        }
        assert!(ctrl.distance >= ctrl.min_distance);

        // 大量にズームアウト
        for _ in 0..100 {
            ctrl.zoom(-1.0);
        }
        assert!(ctrl.distance <= ctrl.max_distance);
    }

    #[test]
    fn orbit_pitch_clamps() {
        let camera = Camera::new(1.0);
        let mut ctrl = OrbitController::from_camera(&camera);
        // 大量に上方向回転
        ctrl.rotate(0.0, -100000.0);
        assert!(ctrl.pitch <= ctrl.max_pitch);
        // 大量に下方向回転
        ctrl.rotate(0.0, 100000.0);
        assert!(ctrl.pitch >= ctrl.min_pitch);
    }

    #[test]
    fn mat4_inverse_identity() {
        let id: [[f32; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let inv = mat4_inverse(id).unwrap();
        for i in 0..4 {
            for j in 0..4 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((inv[i][j] - expected).abs() < 1e-6,
                    "inv[{i}][{j}]: expected {expected}, got {}", inv[i][j]);
            }
        }
    }
}

/// FFI 結果コード
#[repr(C)]
pub enum CplpResult {
    /// 成功
    Ok = 0,
    /// 初期化エラー
    InitError = 1,
    /// 無効な引数
    InvalidArgument = 2,
    /// ランタイムが未初期化
    NotInitialized = 3,
    /// オーディオエンジンエラー
    AudioError = 10,
    /// セッションエラー
    SessionError = 20,
    /// 内部エラー
    InternalError = 99,
}

/// FFI バージョン情報
#[repr(C)]
pub struct CplpVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

// ─── セッション FFI 型 ─────────────────────────────────────

/// セッション接続状態
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CplpSessionStatus {
    /// 未接続
    Disconnected = 0,
    /// 接続中
    Connecting = 1,
    /// 接続済み
    Connected = 2,
    /// 切断中
    Disconnecting = 3,
}

/// セッション状態（C 構造体）
#[repr(C)]
pub struct CplpSessionState {
    /// 接続状態
    pub status: CplpSessionStatus,
    /// 接続中のピア数（自分を含む）
    pub peer_count: u32,
    /// ロビー URL（C 文字列。Disconnected のときは null）
    pub lobby_url: *const std::ffi::c_char,
}

// ─── ミキサー FFI 型 ───────────────────────────────────────

/// 1 トラックの状態（C 構造体）
#[repr(C)]
pub struct CplpTrackInfo {
    /// ピア ID（C 文字列）
    pub peer_id: *const std::ffi::c_char,
    /// ラベル（C 文字列）
    pub label: *const std::ffi::c_char,
    /// ボリューム（0.0–1.0）
    pub volume: f32,
    /// パン（-1.0=L, 0.0=C, 1.0=R）
    pub pan: f32,
    /// ミュート
    pub mute: bool,
    /// ソロ
    pub solo: bool,
}

/// ミキサー状態（トラック一覧 + マスター）
#[repr(C)]
pub struct CplpMixerState {
    /// トラック配列
    pub tracks: *mut CplpTrackInfo,
    /// トラック数
    pub track_count: u32,
    /// マスターボリューム（0.0–1.0）
    pub master_volume: f32,
}

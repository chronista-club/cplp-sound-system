# Full Mesh P2P + 共有ミキサー + ロビーサーバー 設計

**日付**: 2026-02-20
**ステータス**: Approved

---

## 1. 概要

cplp-sound-system を「2人P2P」から「最大5人フルメッシュ + 共有ミキサー + ロビーサーバー」に拡張する設計。

### 設計判断サマリー

| 項目 | 決定 |
|------|------|
| 最大人数 | 4-5人（バンド編成） |
| トポロジー | フルメッシュP2P（ハブなし） |
| ミキシング | 各ピアがローカルで実行（共有設定適用） |
| 共有ミキサー | Volume + Pan + Mute/Solo、全員操作可能 |
| 競合解決 | Last-write-wins（タイムスタンプ） |
| 切断時 | そのトラック消えてセッション継続 |
| 途中参加 | 可能 |
| ホスト権限 | 完全対等（ロビーホストは接続入口なだけ） |
| ロビーサーバー | Axum + SurrealDB |
| OAuth | 複数対応（GitHub, Google, Discord） |
| セッション開始 | 開始通知 + 常時ロビー |
| P2P通信 | Unison Protocol (QUIC) |

---

## 2. アーキテクチャ全体像

```
┌─────────────────────────────────────────────┐
│         Lobby Server (Axum + SurrealDB)     │
│                                              │
│  HTTP API:                                   │
│    - OAuth (GitHub/Google/Discord)           │
│    - ユーザー管理 (CRUD)                     │
│    - グループ管理 (CRUD)                     │
│    - セッション管理                           │
│                                              │
│  WebSocket:                                  │
│    - プレゼンス（オンライン/オフライン）       │
│    - セッション開始通知                       │
│    - ピアリスト配信                           │
│                                              │
│  SurrealDB:                                  │
│    - users, groups, sessions テーブル         │
└──────────────┬──────────────────────────────┘
               │ シグナリングのみ
               │ （オーディオは経由しない）
               │
    ┌──────────┼──────────┐
    │          │          │
┌───▼──┐  ┌───▼──┐  ┌───▼──┐
│Peer A│←→│Peer B│←→│Peer C│  フルメッシュ P2P
│      │←→│      │←→│      │  (Unison QUIC)
└──────┘  └──────┘  └──────┘
    ↕          ↕          ↕
 ┌──────────────────────────┐
 │  共有ミキサー状態          │
 │  (レプリケーテッド)        │
 │  各ピアがローカルコピー保持 │
 └──────────────────────────┘
```

---

## 3. フルメッシュ P2P

### 3.1 トポロジー

各ピアが他の全ピアと直接接続。N人で N*(N-1)/2 本の双方向接続。

```
4人の場合:
  A ←→ B
  A ←→ C
  A ←→ D
  B ←→ C
  B ←→ D
  C ←→ D
  = 6本の双方向接続
```

### 3.2 帯域要件

48kHz / stereo / f32 = 384KB/s per stream

| 人数 | 送信 | 受信 | 合計/ピア |
|------|------|------|-----------|
| 2人 | 384KB/s | 384KB/s | 768KB/s |
| 3人 | 768KB/s | 768KB/s | 1.5MB/s |
| 4人 | 1.2MB/s | 1.2MB/s | 2.3MB/s |
| 5人 | 1.5MB/s | 1.5MB/s | 3.0MB/s |

### 3.3 ピア間チャネル（各ペアに2本）

| チャネル | 用途 | プロトコル |
|----------|------|-----------|
| `audio` | PCM ストリーム | `send_raw()` / `recv_raw()` |
| `control` | ミキサー操作 + セッション管理 | `send_event()` / `recv()` JSON |

### 3.4 接続フロー（ロビー方式）

```
1. Player A がロビーサーバーで「セッション開始」
   → グループメンバーに通知
   → A の ProtocolServer 起動、アドレスをロビーに登録

2. Player B がロビーで「参加」
   → B の ProtocolServer 起動、アドレスをロビーに登録
   → ロビーから PeerList([A]) を取得
   → B が A に直接接続（Unison QUIC）
   → A-B 間で audio + control チャネル開設

3. Player C がロビーで「参加」
   → C の ProtocolServer 起動
   → ロビーから PeerList([A, B]) を取得
   → C が A, B それぞれに直接接続
   → 全ペアで audio + control チャネル開設
```

ロビーサーバーはピア発見（シグナリング）のみ。オーディオは一切経由しない。

---

## 4. 共有ミキサー

### 4.1 データモデル

```rust
/// 各ピアがローカルに保持するレプリケートされたミキサー状態
struct MixerState {
    tracks: HashMap<PeerId, TrackState>,
    master_volume: f32,
}

struct TrackState {
    volume: f32,     // 0.0 - 1.0
    pan: f32,        // -1.0 (L) to 1.0 (R)
    mute: bool,
    solo: bool,
    label: String,   // プレイヤー名 or 楽器名
}
```

### 4.2 同期方式

- 操作発生時に `control` チャネルで全ピアに broadcast
- 各ピアがローカルの MixerState を更新
- 競合は Last-write-wins（タイムスタンプが新しい方が勝つ）

### 4.3 control チャネルイベント

```rust
#[serde(tag = "type")]
enum ControlEvent {
    // ミキサー操作
    FaderChange { track: PeerId, volume: f32, ts: u64 },
    PanChange   { track: PeerId, pan: f32, ts: u64 },
    MuteToggle  { track: PeerId, mute: bool, ts: u64 },
    SoloToggle  { track: PeerId, solo: bool, ts: u64 },
    MasterVol   { volume: f32, ts: u64 },

    // セッション管理
    PeerJoined  { peer: PeerId, addr: SocketAddr, label: String },
    PeerLeft    { peer: PeerId },
    MixerSync   { state: MixerState },  // 途中参加者への全状態同期

    // 既存
    LatencyReport { rtt_us: u64, jitter_us: u64 },
    PluginInfo { name: String, vendor: String },
}
```

### 4.4 途中参加時のミキサー同期

新ピア参加時に、最初に接続したピアが `MixerSync` で現在のミキサー全状態を送信。

---

## 5. ローカルミキシングパイプライン

```
各ピアのローカル処理:

  自分の CLAP Plugin ──→ ┐
  受信トラック Peer B ──→ │  LocalMixer    ──→ 🔊 cpal output
  受信トラック Peer C ──→ │  (MixerState
  受信トラック Peer D ──→ ┘   を適用)

  同時に:
  自分の CLAP Plugin ──→ send_raw() to B, C, D
```

各トラックに MixerState の volume/pan/mute/solo を適用してミックス。
Solo が1つでもアクティブなら、Solo されたトラックだけを出力。

---

## 6. ロビーサーバー

### 6.1 技術スタック

| 層 | 技術 |
|----|------|
| HTTP フレームワーク | Axum |
| データベース | SurrealDB |
| リアルタイム通信 | WebSocket (axum built-in) |
| 認証 | OAuth 2.0 (GitHub, Google, Discord) |
| 非同期ランタイム | tokio |
| デプロイ | FleetFlow (KDL) |

### 6.2 データモデル (SurrealDB)

```surql
-- ユーザー
DEFINE TABLE users SCHEMAFULL;
DEFINE FIELD name ON users TYPE string;
DEFINE FIELD email ON users TYPE string;
DEFINE FIELD avatar_url ON users TYPE option<string>;
DEFINE FIELD oauth_provider ON users TYPE string;
DEFINE FIELD oauth_id ON users TYPE string;
DEFINE FIELD created_at ON users TYPE datetime DEFAULT time::now();
DEFINE INDEX unique_oauth ON users FIELDS oauth_provider, oauth_id UNIQUE;

-- グループ
DEFINE TABLE groups SCHEMAFULL;
DEFINE FIELD name ON groups TYPE string;
DEFINE FIELD created_by ON groups TYPE record<users>;
DEFINE FIELD created_at ON groups TYPE datetime DEFAULT time::now();

-- グループメンバーシップ（グラフエッジ）
DEFINE TABLE member_of SCHEMAFULL TYPE RELATION FROM users TO groups;
DEFINE FIELD role ON member_of TYPE string DEFAULT "member";
DEFINE FIELD joined_at ON member_of TYPE datetime DEFAULT time::now();

-- アクティブセッション
DEFINE TABLE sessions SCHEMAFULL;
DEFINE FIELD group ON sessions TYPE record<groups>;
DEFINE FIELD started_by ON sessions TYPE record<users>;
DEFINE FIELD status ON sessions TYPE string DEFAULT "waiting"; -- waiting | active | ended
DEFINE FIELD created_at ON sessions TYPE datetime DEFAULT time::now();

-- セッション参加者
DEFINE TABLE session_peers SCHEMAFULL TYPE RELATION FROM users TO sessions;
DEFINE FIELD addr ON session_peers TYPE string; -- ProtocolServer のアドレス
DEFINE FIELD joined_at ON session_peers TYPE datetime DEFAULT time::now();
```

### 6.3 API エンドポイント

#### 認証
| メソッド | パス | 説明 |
|---------|------|------|
| GET | `/auth/:provider` | OAuth フロー開始 (redirect) |
| GET | `/auth/:provider/callback` | OAuth コールバック |
| POST | `/auth/logout` | ログアウト |
| GET | `/auth/me` | 現在のユーザー情報 |

#### グループ
| メソッド | パス | 説明 |
|---------|------|------|
| POST | `/groups` | グループ作成 |
| GET | `/groups` | 自分のグループ一覧 |
| GET | `/groups/:id` | グループ詳細 |
| POST | `/groups/:id/invite` | メンバー招待 |
| DELETE | `/groups/:id/members/:uid` | メンバー削除 |

#### セッション
| メソッド | パス | 説明 |
|---------|------|------|
| POST | `/groups/:id/sessions` | セッション開始 |
| POST | `/sessions/:id/join` | セッション参加（アドレス登録） |
| GET | `/sessions/:id/peers` | ピアリスト取得 |
| POST | `/sessions/:id/leave` | セッション離脱 |

#### WebSocket
| パス | 説明 |
|------|------|
| `WS /ws` | プレゼンス + セッション開始通知 |

### 6.4 WebSocket イベント

```rust
/// サーバー → クライアント
#[serde(tag = "type")]
enum WsEvent {
    SessionStarted { group_id: String, session_id: String, started_by: String },
    PeerJoined { session_id: String, peer: PeerInfo },
    PeerLeft { session_id: String, peer_id: String },
    Presence { user_id: String, status: PresenceStatus },
}

/// クライアント → サーバー
#[serde(tag = "type")]
enum WsCommand {
    SubscribeGroup { group_id: String },
    SetPresence { status: PresenceStatus },
}

enum PresenceStatus { Online, Offline }
```

---

## 7. cplp-network クレート変更

### 7.1 P2pManager（2人→N人）

```rust
// Before
struct P2pManager {
    server_handle: Option<ServerHandle>,
    // 1つのピア接続
}

// After
struct P2pManager {
    server_handle: Option<ServerHandle>,
    peers: HashMap<PeerId, PeerConnection>,  // N-1 本の接続
    mixer_state: Arc<RwLock<MixerState>>,    // 共有ミキサー
    local_peer_id: PeerId,
}

struct PeerConnection {
    addr: SocketAddr,
    audio_channel: UnisonChannel,    // raw bytes
    control_channel: UnisonChannel,  // JSON events
    status: PeerStatus,
}
```

### 7.2 AudioStreamer（1本→N本）

```rust
// Before: 1ピアとの送受信
struct AudioStreamer { send_tx, recv_rx }

// After: N-1ピアとの送受信
struct AudioStreamer {
    /// 自分のオーディオを全ピアに送信
    send_tx: mpsc::Sender<AudioPacket>,
    /// 各ピアからの受信トラック
    peer_tracks: HashMap<PeerId, mpsc::Receiver<AudioPacket>>,
}
```

### 7.3 ControlHandler（ミキサーイベント追加）

```rust
struct ControlHandler {
    /// 各ピアの control チャネル
    channels: HashMap<PeerId, UnisonChannel>,
    /// 共有ミキサー状態（P2pManager と共有）
    mixer_state: Arc<RwLock<MixerState>>,
}
```

---

## 8. 切断・再参加

### 8.1 切断検知

- QUIC コネクション断を `ConnectionEvent::Disconnected` で検知
- 該当ピアの `PeerConnection` を `peers` から削除
- ミキサーから該当トラックを削除
- 全ピアに `PeerLeft` イベント送信
- 残りのメンバーでセッション継続

### 8.2 再参加

通常の途中参加と同じフロー:
1. ロビーサーバーに再度 join
2. PeerList を取得して全ピアに接続
3. MixerSync で現在状態を受信

---

## 9. 新規クレート/パッケージ

| パッケージ | 言語 | 役割 |
|-----------|------|------|
| `cplp-lobby` | Rust (Axum) | ロビーサーバー |
| `cplp-core` (拡張) | Rust | MixerState, PeerId 追加 |
| `cplp-network` (拡張) | Rust | HashMap<PeerId, PeerConnection> 対応 |

---

## 10. 実装フェーズ

### Phase 1: フルメッシュ P2P（Unison統合）
- P2pManager を HashMap<PeerId, PeerConnection> に拡張
- AudioStreamer を N本対応
- ControlHandler にミキサーイベント追加
- MixerState のローカルミキシング

### Phase 2: ロビーサーバー
- Hono プロジェクトセットアップ
- SurrealDB スキーマ
- OAuth (GitHub → Google → Discord)
- グループ CRUD
- セッション管理 + WebSocket

### Phase 3: クライアント統合
- cplp-app からロビーサーバーに接続
- ロビー経由のピア発見
- 途中参加 / 再参加

---

**設計バージョン**: 0.2.0-draft
**最終更新**: 2026-02-20

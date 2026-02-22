# Cadence: AI バンドメンバー 設計

## 概要

Cadence は P2P セッションに参加する AI バンドメンバー。
Player A からテキストで演奏指示を受け、CLAP プラグインで演奏し、PCM を送り返す。
開発中の E2E テスト相手としても機能する。

## 要件

- 常駐サーバーとして listen し、Player A の再接続に対応
- 演奏指示の解釈: ローカルパーサー (`/parse`) と Claude SDK (`/ask`) の明示切り替え
- CLAP プラグイン: 起動時指定 + セッション中の動的切り替え
- 管理: CLI + MCP サーバー（Claude Code から操作）
- デフォルトテンポ: 122 BPM

## アーキテクチャ

モノリス型。`cplp-cadence` クレートとして独立バイナリ。

```
cplp-cadence (単一バイナリ)
┌─────────────────────────────────────────────────┐
│                                                 │
│  CLI Layer                                      │
│  cadence listen --plugin <ID> --port 5000       │
│                                                 │
│  ┌─────────────────────────────────────────┐    │
│  │ SessionHost (cplp-session 再利用)       │    │
│  │ - P2P listen (Unison/QUIC)              │    │
│  │ - Audio streaming (cplp-audio)          │    │
│  │ - Control channel (既存)                │    │
│  │ + Command channel (新設)                │    │
│  └────────────┬────────────────────────────┘    │
│               │ テキストコマンド受信              │
│               ▼                                 │
│  ┌─────────────────────────────────────────┐    │
│  │ CommandRouter                            │    │
│  │ /parse → LocalParser (高速)             │    │
│  │ /ask  → ClaudeClient (柔軟)             │    │
│  └────────────┬────────────────────────────┘    │
│               │ MidiSequence                    │
│               ▼                                 │
│  ┌─────────────────────────────────────────┐    │
│  │ MidiSequencer                            │    │
│  │ - MidiSequence → タイムスケジュール      │    │
│  │ - NoteOn/Off をオーディオスレッドに送信  │    │
│  │ → CLAP Plugin → PCM → 相手に送信        │    │
│  └─────────────────────────────────────────┘    │
│                                                 │
│  MCP Server (管理用、同一プロセス内)             │
│                                                 │
└─────────────────────────────────────────────────┘
```

## コマンドプロトコル

既存の `ControlEvent` を拡張。Unison のコントロールチャンネルに載せる。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ControlEvent {
    // ... 既存イベント ...

    // Cadence コマンド
    Command {
        from: PeerId,
        mode: CommandMode,   // Parse or Ask
        text: String,
    },

    // Cadence レスポンス
    CommandAck {
        status: CommandStatus,  // Accepted, Rejected, Error
        message: String,
    },

    // プラグイン切り替え
    PluginSwitch {
        from: PeerId,
        plugin_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandMode {
    Parse,  // ローカルパーサー
    Ask,    // Claude SDK
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandStatus {
    Accepted,
    Rejected,
    Error,
}
```

## ローカルパーサー (/parse モード)

テキストを MIDI ノート列に変換する。

### 対応文法

```
"C major scale 120bpm"         → Cメジャースケール上昇+下降
"Am pentatonic 90bpm 4bars"    → Amペンタトニック、4小節
"Cmaj7 Dm7 G7 Cmaj7 80bpm"    → コード進行
"stop"                         → 演奏停止
"tempo 140"                    → テンポ変更
```

### 中間表現

```rust
pub struct MidiSequence {
    pub tempo_bpm: f32,          // デフォルト 122 BPM
    pub events: Vec<MidiEvent>,
}

pub struct MidiEvent {
    pub tick: u64,
    pub note: u8,            // MIDI ノート番号 (0-127)
    pub velocity: u8,
    pub duration_ticks: u64,
}
```

## Claude SDK 連携 (/ask モード)

`/ask ブルースのバッキング弾いて` のような自然言語指示を Claude SDK で解釈。

```
Player A → "/ask ブルースのバッキング Key=A"
    → CommandRouter (mode = Ask)
    → ClaudeClient
        - tool_use で MidiSequence を構造化出力
        - 数秒かかるため CommandAck "考え中..." を先行送信
    → MidiSequencer → CLAP → PCM → Player A
```

## MCP サーバー

Cadence プロセス内に stdio MCP サーバーを起動。Claude Code から操作可能。

| ツール | 説明 |
|-------|------|
| `cadence_status` | 状態確認 |
| `cadence_restart` | 再起動 |
| `cadence_plugin_switch` | プラグイン切り替え |
| `cadence_command` | 演奏指示 |
| `cadence_stop` | 演奏停止 |

`.mcp.json` 登録:
```json
{
  "cadence": {
    "command": "cadence",
    "args": ["mcp"]
  }
}
```

## CLI

```bash
# 基本: listen で常駐
cadence listen --plugin <PLUGIN_ID> --port 5000

# ゲストモード
cadence connect <ADDR> --plugin <PLUGIN_ID>

# 管理
cadence status
cadence stop
cadence plugin switch <ID>

# MCP モード
cadence mcp
```

## 成功基準

### マイルストーン 1: E2E フロー

```
1. cadence listen --plugin <ID> --port 5000
2. cplp session connect [::1]:5000 <ID> --hud
3. Player A が "/parse C major scale" を送信
4. Cadence が CLAP で演奏 → PCM が Player A の HUD に表示
```

### 後続マイルストーン

1. `/ask` モード（Claude SDK 連携）
2. MCP サーバー（Claude Code からの管理）
3. プラグイン動的切り替え

## テスト方針

| レイヤー | テスト内容 |
|---------|-----------|
| ローカルパーサー | テキスト → MidiSequence 変換 |
| コマンドプロトコル | Command/CommandAck serialize/deserialize |
| MidiSequencer | MidiSequence → タイミング通りの NoteOn/Off |
| E2E | 2 プロセス接続 → コマンド → PCM 受信 |

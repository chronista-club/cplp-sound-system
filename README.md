# cplp-sound-system

**CLAP Plugin Live Performance** — P2P リアルタイムジャムセッション

物理的に離れた 2 台の macOS を P2P で直接接続し、CLAP プラグインの演奏をリアルタイムに共有するセッションアプリ。

## コンセプト

既存のリモート音楽コラボツールはサーバー経由のルーティングでレイテンシが避けられない。
cplp-sound-system は **QUIC ベースの P2P 直接接続**で LAN 環境 20ms 以下のレイテンシを目標とし、
まるで同じ部屋にいるかのようなジャムセッション体験を実現する。

```
Player A (macOS)                    Player B (macOS)
┌─────────────────┐                ┌─────────────────┐
│ MIDI → CLAP     │   QUIC P2P    │ MIDI → CLAP     │
│ Plugin → Mixer ←┼───────────────┼→ Plugin → Mixer  │
│          ↓      │  生 PCM (f32) │          ↓       │
│     Audio Out   │                │     Audio Out    │
└─────────────────┘                └─────────────────┘
```

## アーキテクチャ

8 つのクレートで構成される Cargo ワークスペース:

| クレート | 役割 |
|---------|------|
| **cplp-app** | CLI アプリケーション |
| **cplp-core** | 共通型定義・設定 |
| **cplp-audio** | オーディオエンジン（CLAP ホスティング + cpal + Mixer） |
| **cplp-network** | P2P ネットワーク層（Unison Protocol） |
| **cplp-session** | セッション管理・シグナリング |
| **cplp-lobby** | ロビーサーバー（グループ・セッション仲介） |
| **cplp-hud** | ライブ演奏向け HUD（wgpu 直上フルスクラッチ、レベルメーター・波形・セッション状態表示） |
| **cplp-cadence** | Cadence - AI バンドメンバー（独立バイナリ `cadence`、セッションに接続して自律演奏） |

```
cplp-app ─→ cplp-session ─→ cplp-network ─→ unison-protocol
    │             │
    ├─→ cplp-audio ─→ clack-host / cpal
    │
    ├─→ cplp-hud ─→ wgpu / glyphon（ライブ演奏 GUI）
    │
    └─→ cplp-core (共通型)

cadence (独立バイナリ) ─→ cplp-session / cplp-audio / cplp-network
```

詳細: [design/01-architecture.md](design/01-architecture.md)

## CLI 使用例

### デバイス確認

```bash
# CLAP プラグインをスキャン
cplp device scan

# MIDI 入力ポートを一覧
cplp device midi

# オーディオ出力テスト（サイン波）
cplp device test --freq 440 --duration 3
```

### プラグイン演奏

```bash
# プラグインを再生（テストノート）
cplp play <PLUGIN_ID>

# MIDI キーボードで演奏
cplp play <PLUGIN_ID> --midi 0

# エフェクトチェイン: シンセ → エフェクト
cplp play <PLUGIN_ID> --fx <FX_PLUGIN_ID>

# プラグイン GUI を表示
cplp play <PLUGIN_ID> --gui

# 再生時間を指定（秒）
cplp play <PLUGIN_ID> --duration 10
```

### HUD（ライブ演奏向け GUI）

```bash
# インタラクティブ HUD を起動
# プラグイン・MIDI ポートのスキャン → GUI 上でプラグイン選択 → 演奏
cplp hud
```

HUD はプラグイン一覧・MIDI ポート一覧を表示し、GUI 上から演奏操作が可能。
wgpu で描画されたレベルメーター・波形ビジュアライザをリアルタイム表示する。

セッション中に `--hud` フラグを付けると HUD を同時起動できる:

```bash
cplp session listen <PLUGIN_ID> --port 5000 --hud
cplp session connect [::1]:5000 <PLUGIN_ID> --port 5001 --hud
```

### P2P セッション（直接接続）

```bash
# ターミナル 1: ホストとして待機
cplp session listen <PLUGIN_ID> --port 5000

# ターミナル 2: ピアに接続
cplp session connect [::1]:5000 <PLUGIN_ID> --port 5001
```

### ロビー経由セッション

```bash
# グループ一覧
cplp session lobby groups

# ホストとしてセッション開始
cplp session lobby host --group <GROUP_ID> <PLUGIN_ID>

# セッションに参加
cplp session lobby join --session <SESSION_ID> <PLUGIN_ID>
```

### Cadence（AI バンドメンバー）

Cadence は独立バイナリ `cadence` として動作する AI バンドメンバー。
P2P セッションに接続し、自律的に演奏する。

```bash
# ホストとして接続を待機
cadence listen <PLUGIN_ID> --port 5000

# 指定アドレスに接続
cadence connect [::1]:5000 <PLUGIN_ID> --port 5001

# 稼働状況を表示
cadence status
```

セッション中に `/parse` や `/ask` コマンドで Cadence に演奏指示を送信できる:

```bash
# セッション内コマンド
/parse C4 E4 G4 120bpm        # MIDIノートを直接指定
/ask C major chord             # 自然言語で指示
```

### ログ出力

```bash
# ファイルにログ出力
cplp play <PLUGIN_ID> --log-file cplp.log

# プリセットでログレベル指定
CPLP_LOG=dev cplp device scan          # debug + audio trace
CPLP_LOG=audio cplp play <PLUGIN_ID>   # オーディオ系 trace
CPLP_LOG=network cplp session listen   # ネットワーク系 debug
CPLP_LOG=production cplp play <PLUGIN_ID>  # warn 以上のみ
```

## 技術スタック

| 層 | 技術 | 用途 |
|----|------|------|
| 言語 | Rust (edition 2024) | システムプログラミング・リアルタイム処理 |
| オーディオ I/O | cpal | クロスプラットフォームオーディオ入出力 |
| プラグインホスト | clack-host | CLAP プラグインホスティング |
| MIDI | midir | MIDI 入力 |
| ネットワーク | Unison Protocol (QUIC) | P2P 通信 |
| GPU レンダリング | wgpu + glyphon | HUD 描画（レベルメーター・波形・テキスト） |
| 非同期ランタイム | tokio | ネットワーク・制御の非同期処理 |
| ロビーサーバー | axum + SurrealDB | セッション仲介 |

## 開発セットアップ

### 前提条件

- macOS 14+ (Sonoma 以降)
- Rust 1.85.0+ (`rustup update` で更新)

### ビルド・テスト

```bash
git clone https://github.com/chronista-club/cplp-sound-system.git
cd cplp-sound-system

# ビルド
cargo build

# テスト
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings
```

### ローカル動作確認

同一マシン上で 2 プロセスを起動してループバック接続:

```bash
# ターミナル 1
cargo run -- session listen <PLUGIN_ID> --port 5000

# ターミナル 2
cargo run -- session connect [::1]:5000 <PLUGIN_ID> --port 5001
```

詳細: [guides/01-getting-started.md](guides/01-getting-started.md)

## ドキュメント

### 仕様 (spec/)

| ドキュメント | 内容 |
|-------------|------|
| [spec/01-core-concept.md](spec/01-core-concept.md) | コアコンセプト・要件定義 |
| [spec/02-audio-pipeline.md](spec/02-audio-pipeline.md) | オーディオパイプライン仕様 |
| [spec/03-p2p-protocol.md](spec/03-p2p-protocol.md) | P2P プロトコル仕様 |

### 設計 (design/)

| ドキュメント | 内容 |
|-------------|------|
| [design/01-architecture.md](design/01-architecture.md) | 全体アーキテクチャ設計 |
| [design/02-p2p-connection.md](design/02-p2p-connection.md) | P2P 接続設計 |

### ガイド (guides/)

| ドキュメント | 内容 |
|-------------|------|
| [guides/01-getting-started.md](guides/01-getting-started.md) | 開発ガイド |

## ライセンス

MIT

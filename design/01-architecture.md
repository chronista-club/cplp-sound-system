# design/01: 全体アーキテクチャ設計

**バージョン**: 0.1.0-draft
**最終更新**: 2026-02-20
**ステータス**: Draft

---

## 目次

1. [概要](#1-概要)
2. [レイヤー構成](#2-レイヤー構成)
3. [クレート構成](#3-クレート構成)
4. [スレッド構成](#4-スレッド構成)
5. [データフロー](#5-データフロー)
6. [依存クレート](#6-依存クレート)
7. [要件トレーサビリティ](#7-要件トレーサビリティ)

---

## 1. 概要

cplp-sound-system は、3 つのレイヤー（Session / Audio / Network）と Unison Protocol ライブラリで構成される。各レイヤーは独立したクレートとして分離し、リアルタイムオーディオ処理とネットワーク通信のスレッドを明確に分ける。

---

## 2. レイヤー構成

```mermaid
graph TB
    subgraph "cplp-sound-system"
        direction TB

        subgraph "Session Layer"
            SM[Session Manager<br/>セッション管理]
            PD[Peer Discovery<br/>シグナリング]
        end

        subgraph "Audio Layer"
            PH[CLAP Plugin Host<br/>clack-host]
            MX[Audio Mixer<br/>ローカル + リモート]
            IO[Audio I/O<br/>cpal]
        end

        subgraph "Network Layer"
            P2P[P2P Manager<br/>デュアルロール接続]
            AC[Audio Channel<br/>PCM ストリーミング]
            CC[Control Channel<br/>制御メッセージ]
        end
    end

    subgraph "外部ライブラリ"
        UP[Unison Protocol<br/>QUIC + Channel]
    end

    SM --> PD
    SM --> P2P
    SM --> PH

    PH --> MX
    IO --> MX
    MX --> IO

    P2P --> UP
    AC --> UP
    CC --> UP

    PH -.->|送信バッファ| AC
    AC -.->|受信バッファ| MX
    SM -.->|制御| CC
```

### 2.1 レイヤー責務

| レイヤー | 責務 | 主要構造体 |
|---------|------|-----------|
| **Session** | セッションライフサイクル管理、ピアディスカバリー | `SessionManager`, `PeerDiscovery` |
| **Audio** | CLAP ホスティング、ミキシング、オーディオ I/O | `PluginHost`, `AudioMixer`, `AudioEngine` |
| **Network** | P2P 接続管理、チャネル通信 | `P2pManager`, `AudioStreamer`, `ControlHandler` |

---

## 3. クレート構成

### 3.1 ワークスペース構成

```
cplp-sound-system/
├── Cargo.toml                    # ワークスペースルート
├── crates/
│   ├── cplp-core/                # 共通型定義、設定
│   │   └── src/lib.rs
│   ├── cplp-audio/               # オーディオエンジン（CLAP + cpal + Mixer）
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── engine.rs         # AudioEngine: cpal コールバック管理
│   │       ├── plugin_host.rs    # PluginHost: clack-host ラッパー
│   │       └── mixer.rs          # AudioMixer: ローカル + リモートミキシング
│   ├── cplp-network/             # P2P ネットワーク層
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── p2p.rs            # P2pManager: デュアルロール接続
│   │       ├── audio_channel.rs  # AudioStreamer: PCM 送受信
│   │       └── control.rs        # ControlHandler: 制御メッセージ
│   ├── cplp-session/             # セッション管理
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── manager.rs        # SessionManager: ライフサイクル管理
│   │       └── signaling.rs      # PeerDiscovery: シグナリング
│   └── cplp-app/                 # CLI アプリケーション
│       └── src/
│           └── main.rs
├── schemas/
│   └── cplp-session.kdl          # Unison プロトコルスキーマ
├── spec/
├── design/
└── guides/
```

### 3.2 クレート依存関係

```mermaid
graph TB
    APP[cplp-app<br/>CLI]
    SESSION[cplp-session<br/>セッション管理]
    AUDIO[cplp-audio<br/>オーディオ]
    NETWORK[cplp-network<br/>ネットワーク]
    CORE[cplp-core<br/>共通型]

    APP --> SESSION
    APP --> AUDIO
    APP --> NETWORK
    APP --> CORE

    SESSION --> NETWORK
    SESSION --> AUDIO
    SESSION --> CORE

    AUDIO --> CORE
    NETWORK --> CORE

    NETWORK -.->|依存| UNISON[unison-protocol]
    AUDIO -.->|依存| CLACK[clack-host]
    AUDIO -.->|依存| CPAL[cpal]
```

### 3.3 ワークスペース共通設定

| 設定 | 値 |
|------|-----|
| edition | 2024 |
| rust-version | 1.85.0 |
| version | 0.1.0 |
| resolver | 2 |

---

## 4. スレッド構成

### 4.1 スレッドモデル

```mermaid
graph TB
    subgraph "Audio Thread（リアルタイム）"
        direction TB
        CPAL_CB[cpal Callback]
        PLUGIN[CLAP Plugin Process]
        MIXER[Audio Mixer]

        CPAL_CB --> PLUGIN --> MIXER --> CPAL_CB
    end

    subgraph "Network Thread（tokio 非同期）"
        direction TB
        SEND[Audio Send Task<br/>Ring Buffer → QUIC]
        RECV[Audio Recv Task<br/>QUIC → Jitter Buffer]
        CTRL[Control Task<br/>制御メッセージ処理]
    end

    subgraph "Session Thread（tokio 非同期）"
        direction TB
        MGMT[Session Management]
        SIGNAL[Signaling]
        LATENCY[Latency Monitor]
    end

    PLUGIN -.->|lock-free ring buffer| SEND
    RECV -.->|lock-free ring buffer| MIXER
    MGMT -.->|制御コマンド| CTRL
```

### 4.2 リアルタイム制約

| スレッド | 制約 | 許可される操作 | 禁止される操作 |
|---------|------|--------------|--------------|
| Audio Thread | リアルタイム | メモリ読み書き、lock-free 操作 | メモリ確保、ロック、I/O、システムコール |
| Network Thread | なし | すべて | - |
| Session Thread | なし | すべて | - |

### 4.3 スレッド間通信

```mermaid
graph LR
    subgraph "Lock-free 構造"
        RB_SEND[Ring Buffer<br/>送信用]
        RB_RECV[Ring Buffer<br/>受信 + ジッタバッファ]
        CMD[Command Channel<br/>lock-free mpsc]
    end

    AT[Audio Thread] -->|write| RB_SEND
    NT_S[Network Send] -->|read| RB_SEND

    NT_R[Network Recv] -->|write| RB_RECV
    AT -->|read| RB_RECV

    ST[Session Thread] -->|write| CMD
    AT -->|read| CMD
```

---

## 5. データフロー

### 5.1 送信パス

```mermaid
sequenceDiagram
    participant MIDI as MIDI Input
    participant CLAP as CLAP Plugin
    participant RB as Ring Buffer (Send)
    participant NET as Network Thread
    participant QUIC as QUIC Stream

    loop 128 samples ごと
        MIDI->>CLAP: MIDI イベント
        CLAP->>CLAP: process(audio_in, audio_out)
        CLAP->>RB: audio_out を書き込み（lock-free）
        Note over RB: Audio Thread はここで完了

        RB->>NET: 128 samples 読み出し
        NET->>NET: AudioPacket 構築<br/>(seq, timestamp, pcm_data)
        NET->>QUIC: send_event (audio channel)
    end
```

### 5.2 受信パス

```mermaid
sequenceDiagram
    participant QUIC as QUIC Stream
    participant NET as Network Thread
    participant JB as Jitter Buffer
    participant MIX as Audio Mixer
    participant OUT as cpal Output

    loop パケット受信ごと
        QUIC->>NET: recv (audio channel)
        NET->>NET: AudioPacket パース
        NET->>JB: サンプルを書き込み（lock-free）
    end

    loop 128 samples ごと
        JB->>MIX: remote samples 読み出し
        Note over MIX: local samples + remote samples
        MIX->>OUT: mixed output
    end
```

### 5.3 全体データフロー

```mermaid
graph LR
    subgraph "Player A"
        MIDI_A[MIDI] --> CLAP_A[CLAP]
        CLAP_A --> SPLIT_A{Split}
        SPLIT_A --> MIX_A[Mixer]
        SPLIT_A --> RB_A[Ring Buf]
        JB_A[Jitter Buf] --> MIX_A
        MIX_A --> OUT_A[Output]
    end

    subgraph "Player B"
        MIDI_B[MIDI] --> CLAP_B[CLAP]
        CLAP_B --> SPLIT_B{Split}
        SPLIT_B --> MIX_B[Mixer]
        SPLIT_B --> RB_B[Ring Buf]
        JB_B[Jitter Buf] --> MIX_B
        MIX_B --> OUT_B[Output]
    end

    RB_A -->|QUIC| JB_B
    RB_B -->|QUIC| JB_A
```

---

## 6. 依存クレート

### 6.1 直接依存

| クレート | バージョン | 用途 | 使用クレート |
|---------|-----------|------|------------|
| `unison-protocol` | git | QUIC 通信、チャネル管理 | cplp-network |
| `clack-host` | 0.1.x | CLAP プラグインホスティング | cplp-audio |
| `cpal` | 0.15.x | オーディオ I/O | cplp-audio |
| `tokio` | 1.x | 非同期ランタイム | cplp-network, cplp-session |
| `ringbuf` | 0.4.x | lock-free ring buffer | cplp-audio, cplp-network |
| `serde` / `serde_json` | 1.x | シリアライゼーション | cplp-core |
| `clap` (CLI) | 4.x | CLI 引数パース | cplp-app |
| `tracing` | 0.1.x | ロギング | 全クレート |
| `anyhow` | 1.x | エラーハンドリング | 全クレート |

### 6.2 間接依存（Unison 経由）

| クレート | 用途 |
|---------|------|
| `quinn` | QUIC 実装 |
| `rustls` | TLS 1.3 |
| `rkyv` | ゼロコピーシリアライゼーション |

---

## 7. 要件トレーサビリティ

| REQ-ID | 設計上の対応 |
|--------|------------|
| REQ-CORE-001 | セクション 5: フルデュプレックスデータフロー |
| REQ-CORE-002 | セクション 4: リアルタイムスレッド + lock-free 通信 |
| REQ-CORE-003 | セクション 5: 生 PCM データフロー |
| REQ-AUDIO-001 | セクション 3: cplp-audio クレート（clack-host） |
| REQ-AUDIO-002 | セクション 5: Split による同時処理 |
| REQ-AUDIO-003 | セクション 5: Audio Mixer |
| REQ-NET-001 | セクション 2: Network Layer（デュアルロール P2P） |
| REQ-NET-002 | セクション 3: cplp-session（シグナリング） |
| REQ-NET-003 | セクション 2: チャネル分離 |
| REQ-SESSION-001 | セクション 3: cplp-session クレート |
| REQ-SESSION-002 | セクション 3: cplp-audio（プラグイン切り替え） |

---

### 関連ドキュメント

- [spec/01: コアコンセプト](../spec/01-core-concept.md) -- 要件定義
- [spec/02: オーディオパイプライン](../spec/02-audio-pipeline.md) -- オーディオ仕様
- [spec/03: P2P プロトコル](../spec/03-p2p-protocol.md) -- ネットワーク仕様
- [design/02: P2P 接続設計](02-p2p-connection.md) -- デュアルロール P2P の実装設計

---

**設計バージョン**: 0.1.0-draft
**最終更新**: 2026-02-20
**ステータス**: Draft

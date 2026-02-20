# spec/03: P2P プロトコル仕様

**バージョン**: 0.1.0-draft
**最終更新**: 2026-02-20
**ステータス**: Draft

---

## 目次

1. [概要](#1-概要)
2. [対等 P2P トポロジー](#2-対等-p2p-トポロジー)
3. [シグナリングフロー](#3-シグナリングフロー)
4. [デュアルロール接続](#4-デュアルロール接続)
5. [チャネル構成](#5-チャネル構成)
6. [接続ライフサイクル](#6-接続ライフサイクル)
7. [オーディオストリーミングプロトコル](#7-オーディオストリーミングプロトコル)
8. [関連要件](#8-関連要件)

---

## 1. 概要

cplp-sound-system の P2P 通信は、Unison Protocol をライブラリとして活用し、各ピアが **ProtocolServer + ProtocolClient のデュアルロール** で動作する。Unison Protocol 自体は変更せず、cplp-sound-system 側で P2P 接続ロジックを実装する。

### 1.1 設計方針

| 方針 | 説明 |
|------|------|
| **Unison はライブラリ** | Unison Protocol の Server/Client 機能をそのまま使う。上流への変更不要 |
| **デュアルロール** | 各ピアが Server と Client の両方を同時に持つ |
| **シグナリングは最小限** | IPv6 アドレス交換のみ。以降は直接 P2P |
| **QUIC ネイティブ** | TLS 1.3 暗号化、独立ストリームによる HoL Blocking 排除 |

---

## 2. 対等 P2P トポロジー

### 2.1 トポロジー定義

```mermaid
graph LR
    subgraph "Player A"
        AS[Unison Server<br/>ポート自動割当]
        AC[Unison Client]
    end

    subgraph "Player B"
        BS[Unison Server<br/>ポート自動割当]
        BC[Unison Client]
    end

    AC -->|"QUIC 接続"| BS
    BC -->|"QUIC 接続"| AS
```

各ピアは以下の 2 つの QUIC コネクションを持つ:

| コネクション | 方向 | 用途 |
|-------------|------|------|
| 自分の Client → 相手の Server | 出力 | 自分のオーディオを相手に送る |
| 相手の Client → 自分の Server | 入力 | 相手のオーディオを受け取る |

### 2.2 なぜデュアルロールか

Unison Protocol は Server → Client への Identity 送信でチャネル構成を通知する設計。この仕組みをそのまま活かし、各ピアが相手に「自分が提供するチャネル」を通知する。

```mermaid
sequenceDiagram
    participant AS as A:Server
    participant BC as B:Client
    participant BS as B:Server
    participant AC as A:Client

    Note over AS,BC: コネクション 1
    BC->>AS: QUIC 接続
    AS-->>BC: ServerIdentity（A のチャネル一覧）
    BC->>AS: open_channel("audio")

    Note over BS,AC: コネクション 2
    AC->>BS: QUIC 接続
    BS-->>AC: ServerIdentity（B のチャネル一覧）
    AC->>BS: open_channel("audio")
```

---

## 3. シグナリングフロー

### 3.1 シグナリングサーバーの役割

シグナリングサーバーは **IPv6 アドレスとポートの交換のみ** を担当する。オーディオデータは一切経由しない。

```mermaid
sequenceDiagram
    participant A as Player A
    participant S as Signaling Server
    participant B as Player B

    A->>S: POST /sessions<br/>{addr: "[::1]:xxxxx"}
    S-->>A: {session_id: "abc123"}

    Note over A,B: セッション ID を帯域外で共有

    B->>S: POST /sessions/abc123/join<br/>{addr: "[::1]:yyyyy"}
    S-->>B: {peer_addr: "[::1]:xxxxx"}
    S-->>A: {peer_addr: "[::1]:yyyyy"}<br/>(WebSocket or polling)

    Note over A,B: 両者が相手のアドレスを取得

    A->>B: QUIC 直接接続
    B->>A: QUIC 直接接続
```

### 3.2 シグナリング API（最小構成）

| エンドポイント | メソッド | 説明 |
|---------------|---------|------|
| `POST /sessions` | Create | セッション作成。自分のアドレスを登録 |
| `GET /sessions/:id` | Read | セッション情報取得 |
| `POST /sessions/:id/join` | Join | セッションに参加。自分のアドレスを登録 |
| `WS /sessions/:id/events` | Events | ピア参加通知の WebSocket |

### 3.3 シグナリングメッセージ

```kdl
// セッション作成リクエスト
session-create {
    addr "[::1]:12345"
    name "Player A"
}

// セッション参加リクエスト
session-join {
    session_id "abc123"
    addr "[::1]:23456"
    name "Player B"
}

// ピア通知（WebSocket イベント）
peer-joined {
    addr "[::1]:23456"
    name "Player B"
}
```

### 3.4 MVP でのシグナリング

MVP ではシグナリングサーバーを省略し、**コマンドライン引数で相手のアドレスを直接指定** する。

```bash
# Player A: サーバーを起動して待機
cplp-sound-system listen --port 5000

# Player B: Player A に接続
cplp-sound-system connect [::1]:5000
```

---

## 4. デュアルロール接続

### 4.1 接続確立シーケンス

```mermaid
sequenceDiagram
    participant A as Player A
    participant B as Player B

    Note over A: ProtocolServer 起動<br/>ポート P_A で Listen

    Note over B: ProtocolServer 起動<br/>ポート P_B で Listen

    Note over A,B: シグナリングでアドレス交換完了

    par 同時接続
        A->>B: ProtocolClient.connect(B:P_B)
        B->>A: ProtocolClient.connect(A:P_A)
    end

    Note over A: Server: B からの接続を受付<br/>Client: B の Server に接続済
    Note over B: Server: A からの接続を受付<br/>Client: A の Server に接続済

    par チャネル開設
        A->>B: open_channel("audio") on B:Server
        B->>A: open_channel("audio") on A:Server
    end

    Note over A,B: フルデュプレックス通信確立
```

### 4.2 接続状態管理

```mermaid
stateDiagram-v2
    [*] --> Idle: 起動
    Idle --> ServerStarted: ProtocolServer.listen()
    ServerStarted --> Connecting: 相手のアドレス取得
    Connecting --> HalfConnected: 片方の接続完了
    HalfConnected --> Connected: 双方の接続完了
    Connected --> SessionActive: チャネル開設完了
    SessionActive --> Disconnecting: 切断要求
    Disconnecting --> Idle: 切断完了

    SessionActive --> Reconnecting: 接続断検知
    Reconnecting --> Connected: 再接続成功
    Reconnecting --> Idle: 再接続失敗
```

---

## 5. チャネル構成

### 5.1 KDL スキーマ定義

各ピアの ProtocolServer が以下のチャネルを公開する:

```kdl
protocol "cplp-session" version="1.0.0" {
    namespace "club.chronista.cplp"
    description "CLAP Plugin Live Performance - P2P Session"

    // オーディオチャネル: 生 PCM ストリーミング
    // 最低レイテンシが求められる
    channel "audio" direction="bidirectional" lifetime="persistent" {
        // オーディオデータは生バイト列として送信（JSON オーバーヘッド回避）
        // フォーマット: f32 samples × channels
    }

    // コントロールチャネル: セッション制御
    channel "control" direction="bidirectional" lifetime="persistent" {
        request "SetBufferSize" {
            field "size" type="u32"
            returns "Ack" {
                field "accepted" type="bool"
            }
        }
        request "SetSampleRate" {
            field "rate" type="u32"
            returns "Ack" {
                field "accepted" type="bool"
            }
        }
        event "LatencyReport" {
            field "rtt_us" type="u64"
            field "jitter_us" type="u64"
        }
    }

    // セッションチャネル: メタデータ交換
    channel "session" direction="bidirectional" lifetime="persistent" {
        request "PluginInfo" {
            field "name" type="string"
            field "vendor" type="string"
            returns "Ack" {
                field "received" type="bool"
            }
        }
        event "PeerStatus" {
            field "status" type="string"
        }
        event "PluginChanged" {
            field "name" type="string"
            field "vendor" type="string"
        }
    }
}
```

### 5.2 チャネル一覧

| チャネル | 方向 | データ形式 | 用途 | 関連要件 |
|---------|------|-----------|------|---------|
| `audio` | 双方向 | 生バイト列 (f32 PCM) | オーディオストリーミング | REQ-CORE-001, REQ-CORE-003 |
| `control` | 双方向 | JSON (Request/Response) | パラメータネゴシエーション、レイテンシ監視 | REQ-NET-003 |
| `session` | 双方向 | JSON (Request/Event) | プラグイン情報、ピア状態 | REQ-SESSION-001, REQ-SESSION-002 |

### 5.3 チャネル分離の意義

QUIC の独立ストリームにより、各チャネルは独立した HoL Blocking 境界を持つ（REQ-NET-003）。

```mermaid
graph TB
    subgraph "QUIC コネクション"
        S1["Stream: audio<br/>PCM データ<br/>-- 最高優先度 --"]
        S2["Stream: control<br/>制御メッセージ<br/>-- パケットロスが audio に影響しない --"]
        S3["Stream: session<br/>メタデータ<br/>-- パケットロスが audio に影響しない --"]
    end
```

---

## 6. 接続ライフサイクル

### 6.1 セッション作成・参加フロー

```mermaid
graph TB
    START([開始]) --> SERVER[ProtocolServer 起動]
    SERVER --> SIGNAL{シグナリング方式}

    SIGNAL -->|MVP| MANUAL[手動アドレス指定]
    SIGNAL -->|将来| AUTO[シグナリングサーバー]

    MANUAL --> ADDR[相手のアドレスを取得]
    AUTO --> ADDR

    ADDR --> CONNECT[ProtocolClient.connect]
    ADDR --> WAIT[Server で接続待ち]

    CONNECT --> READY{双方接続済?}
    WAIT --> READY

    READY -->|Yes| NEGOTIATE[パラメータネゴシエーション<br/>control チャネル]
    READY -->|No| WAIT2[待機]
    WAIT2 --> READY

    NEGOTIATE --> AUDIO[audio チャネル開設]
    AUDIO --> SESSION([セッション開始])
```

### 6.2 切断フロー

```mermaid
sequenceDiagram
    participant A as Player A
    participant B as Player B

    A->>B: session チャネル: PeerStatus("disconnecting")
    A->>A: audio チャネル close
    A->>A: control チャネル close
    A->>A: session チャネル close
    A->>A: ProtocolClient.disconnect()
    A->>A: ProtocolServer.stop()

    Note over B: 接続断を検知
    B->>B: クリーンアップ
```

---

## 7. オーディオストリーミングプロトコル

### 7.1 パケットフォーマット

オーディオチャネルでは、JSON オーバーヘッドを回避するため、生バイト列でオーディオデータを送信する。

```
┌──────────────────────────────────────────┐
│ Audio Packet                             │
├──────────┬──────────┬───────────────────┤
│ seq: u32 │ ts: u64  │ PCM data: [f32]   │
│ 4 bytes  │ 8 bytes  │ 1024 bytes        │
│          │          │ (128 samples × 2ch │
│          │          │  × 4 bytes)        │
├──────────┴──────────┴───────────────────┤
│ Total: 1,036 bytes per packet           │
└──────────────────────────────────────────┘
```

| フィールド | 型 | サイズ | 説明 |
|-----------|-----|--------|------|
| `seq` | u32 | 4 bytes | シーケンス番号（パケットロス検知、順序復元） |
| `ts` | u64 | 8 bytes | タイムスタンプ（サンプル単位、同期用） |
| `pcm_data` | [f32] | 可変 | PCM オーディオデータ |

### 7.2 パケットロス対策

| 方式 | 説明 |
|------|------|
| 検知 | シーケンス番号の欠番で検知 |
| 対策 | 再送しない（リアルタイム性優先）。欠番バッファは無音で埋める |
| 報告 | control チャネルの LatencyReport でパケットロス率を報告 |

---

## 8. 関連要件

| REQ-ID | 本仕様での対応箇所 |
|--------|------------------|
| REQ-NET-001 | セクション 2, 4: デュアルロール P2P |
| REQ-NET-002 | セクション 3: シグナリングフロー |
| REQ-NET-003 | セクション 5: チャネル構成と HoL Blocking 排除 |
| REQ-SESSION-001 | セクション 6: セッション作成・参加フロー |
| REQ-SESSION-002 | セクション 5: session チャネルの PluginChanged イベント |
| REQ-CORE-001 | セクション 7: フルデュプレックスオーディオストリーミング |
| REQ-CORE-003 | セクション 7: 生 PCM パケットフォーマット |

---

### 関連ドキュメント

- [spec/01: コアコンセプト](01-core-concept.md) -- プロジェクト全体像と要件
- [spec/02: オーディオパイプライン](02-audio-pipeline.md) -- オーディオ処理仕様
- [design/01: アーキテクチャ](../design/01-architecture.md) -- 全体設計
- [design/02: P2P 接続設計](../design/02-p2p-connection.md) -- デュアルロール P2P の実装設計

---

**仕様バージョン**: 0.1.0-draft
**最終更新**: 2026-02-20
**ステータス**: Draft

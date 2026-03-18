# Looper マルチトラック実装計画

**日付**: 2026-02-28
**ステータス**: In Progress
**関連設計**: [design/looper-multitrack.md](../design/looper-multitrack.md)
**関連仕様**: [spec/looper-multitrack.md](../spec/looper-multitrack.md)

---

## Phase 1: Hello Looper — 単一ルーパーを触れる

**対応要件**: REQ-LP-001, REQ-LP-002, REQ-LP-003

### 変更ファイル

| ファイル | 変更内容 |
|----------|----------|
| `crates/cplp-audio/src/plugin_host.rs` | CC サポート、MidiEventSender/Receiver 新設 |
| `crates/cplp-audio/src/midi_input.rs` | CC パース、MidiEventSender 対応 |
| `crates/cplp-app/src/main.rs` | `--looper` フラグ、Looper 統合 |
| `crates/cplp-app/Cargo.toml` | `cplp-plug-looper` 依存追加 |

### 検証

```bash
cplp play <synth-id> --looper -m <midi-port>
# Keystage で演奏 → C3 録音 → D3 停止 → E3 再生
```

---

## Phase 2: マルチトラック — 5トラック独立操作

**対応要件**: REQ-LP-004, REQ-LP-005

### 変更ファイル

| ファイル | 変更内容 |
|----------|----------|
| `crates/cplp-plug-looper/src/lib.rs` | `MultiTrackLooper` 構造体追加 |
| `crates/cplp-app/src/main.rs` | MultiTrackLooper に切替 |

### 検証

```bash
# 同じコマンドで起動（MultiTrackLooper に自動切替）
# トラック1に録音 → トラック2に別フレーズ → 同時再生
```

---

## Phase 3: LPD8 フルマッピング + UX

**対応要件**: REQ-LP-006, REQ-LP-007, REQ-LP-008, REQ-LP-009

### 変更ファイル

| ファイル | 変更内容 |
|----------|----------|
| `crates/cplp-audio/src/midi_input.rs` | マルチデバイス MIDI ルーティング |
| `crates/cplp-plug-looper/src/lib.rs` | LPD8 パッド/ノブマッピング |
| `crates/cplp-hud/` | Looper 状態表示 |

### 検証

```bash
cplp play <synth-id> --looper -m <keystage-port> --looper-midi <lpd8-port>
```

---

## 全 Phase 共通テスト

```bash
cargo test -p cplp-plug-looper
cargo test -p cplp-audio
cargo build
```

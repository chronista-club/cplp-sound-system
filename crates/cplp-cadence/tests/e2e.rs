//! E2E 統合テスト: Cadence コマンドプロトコルフロー
//!
//! バイナリクレートの内部モジュールにはアクセスできないため、
//! ここでは `cplp-network` / `cplp-core` の公開型を使い、
//! Player A → Cadence 間のコマンドプロトコル全体を JSON 経由でテストする。

use cplp_core::types::PeerId;
use cplp_network::control::{CommandMode, CommandStatus, ControlEvent};

/// Player A がコマンドを送信 → JSON シリアライズ → Cadence がデシリアライズ
/// → Cadence が CommandAck を返す、という一連のプロトコルフローを検証する。
#[test]
fn command_roundtrip_via_json() {
    // ── Step 1: Player A がスケールコマンドを送信 ──
    let command = ControlEvent::Command {
        from: PeerId::new("player-a"),
        mode: CommandMode::Parse,
        text: "C major scale 120bpm".into(),
    };

    // JSON シリアライズ（ネットワーク送信をシミュレート）
    let json = serde_json::to_string(&command).expect("Command のシリアライズに失敗");

    // JSON にプロトコル上必要なフィールドが含まれていることを確認
    assert!(
        json.contains("\"type\":\"Command\""),
        "type タグが含まれるべき"
    );
    assert!(json.contains("player-a"), "送信元 PeerId が含まれるべき");
    assert!(json.contains("Parse"), "CommandMode が含まれるべき");
    assert!(
        json.contains("C major scale 120bpm"),
        "コマンドテキストが含まれるべき"
    );

    // Cadence 側でデシリアライズ（ネットワーク受信をシミュレート）
    let received: ControlEvent =
        serde_json::from_str(&json).expect("Command のデシリアライズに失敗");

    match received {
        ControlEvent::Command { from, mode, text } => {
            assert_eq!(from, PeerId::new("player-a"));
            assert!(matches!(mode, CommandMode::Parse));
            assert_eq!(text, "C major scale 120bpm");
        }
        _ => panic!("Command バリアントが期待されたが、異なるバリアントを受信"),
    }

    // ── Step 2: Cadence が CommandAck（受理）を返す ──
    let ack = ControlEvent::CommandAck {
        status: CommandStatus::Accepted,
        message: "演奏開始: C major scale @ 120bpm".into(),
    };

    let ack_json = serde_json::to_string(&ack).expect("CommandAck のシリアライズに失敗");

    assert!(
        ack_json.contains("\"type\":\"CommandAck\""),
        "type タグが含まれるべき"
    );
    assert!(ack_json.contains("Accepted"), "ステータスが含まれるべき");
    assert!(ack_json.contains("演奏開始"), "メッセージが含まれるべき");

    let received_ack: ControlEvent =
        serde_json::from_str(&ack_json).expect("CommandAck のデシリアライズに失敗");

    match received_ack {
        ControlEvent::CommandAck { status, message } => {
            assert!(matches!(status, CommandStatus::Accepted));
            assert!(message.contains("演奏開始"));
        }
        _ => panic!("CommandAck バリアントが期待されたが、異なるバリアントを受信"),
    }
}

/// Ask モードでの委譲コマンドフロー。
/// Player A が自然言語で指示 → Cadence が DelegateToLlm として処理し、
/// 最終的に CommandAck を返すプロトコルをテストする。
#[test]
fn ask_mode_command_roundtrip() {
    let command = ControlEvent::Command {
        from: PeerId::new("player-b"),
        mode: CommandMode::Ask,
        text: "ブルースっぽいバッキング弾いて".into(),
    };

    let json = serde_json::to_string(&command).unwrap();
    let received: ControlEvent = serde_json::from_str(&json).unwrap();

    match received {
        ControlEvent::Command { from, mode, text } => {
            assert_eq!(from, PeerId::new("player-b"));
            assert!(matches!(mode, CommandMode::Ask));
            assert_eq!(text, "ブルースっぽいバッキング弾いて");
        }
        _ => panic!("Command バリアントが期待された"),
    }

    // Cadence が LLM 委譲後に Accepted を返す
    let ack = ControlEvent::CommandAck {
        status: CommandStatus::Accepted,
        message: "LLM 生成シーケンスを演奏開始".into(),
    };
    let ack_json = serde_json::to_string(&ack).unwrap();
    let received_ack: ControlEvent = serde_json::from_str(&ack_json).unwrap();

    match received_ack {
        ControlEvent::CommandAck { status, message } => {
            assert!(matches!(status, CommandStatus::Accepted));
            assert!(message.contains("LLM"));
        }
        _ => panic!("CommandAck バリアントが期待された"),
    }
}

/// エラー時のコマンドフロー。
/// 不正なコマンド → Cadence が Rejected/Error の CommandAck を返すプロトコル。
#[test]
fn error_command_flow() {
    // Player A が不正なコマンドを送信
    let command = ControlEvent::Command {
        from: PeerId::new("player-a"),
        mode: CommandMode::Parse,
        text: "unknown gibberish".into(),
    };
    let json = serde_json::to_string(&command).unwrap();
    let _received: ControlEvent = serde_json::from_str(&json).unwrap();

    // Cadence がエラーで応答
    let ack = ControlEvent::CommandAck {
        status: CommandStatus::Error,
        message: "不明なスケール名: gibberish".into(),
    };
    let ack_json = serde_json::to_string(&ack).unwrap();
    let received_ack: ControlEvent = serde_json::from_str(&ack_json).unwrap();

    match received_ack {
        ControlEvent::CommandAck { status, message } => {
            assert!(matches!(status, CommandStatus::Error));
            assert!(message.contains("不明な"));
        }
        _ => panic!("CommandAck バリアントが期待された"),
    }
}

/// PluginSwitch コマンドとの組み合わせフロー。
/// プラグイン切替 → スケールコマンド → ACK の一連の流れ。
#[test]
fn plugin_switch_then_command_flow() {
    // Step 1: プラグイン切替
    let switch = ControlEvent::PluginSwitch {
        from: PeerId::new("player-a"),
        plugin_id: "com.u-he.Diva".into(),
    };
    let switch_json = serde_json::to_string(&switch).unwrap();
    let received_switch: ControlEvent = serde_json::from_str(&switch_json).unwrap();

    match received_switch {
        ControlEvent::PluginSwitch {
            from, plugin_id, ..
        } => {
            assert_eq!(from, PeerId::new("player-a"));
            assert_eq!(plugin_id, "com.u-he.Diva");
        }
        _ => panic!("PluginSwitch バリアントが期待された"),
    }

    // Step 2: 続けてスケールコマンド
    let command = ControlEvent::Command {
        from: PeerId::new("player-a"),
        mode: CommandMode::Parse,
        text: "A minor 90bpm".into(),
    };
    let cmd_json = serde_json::to_string(&command).unwrap();
    let received_cmd: ControlEvent = serde_json::from_str(&cmd_json).unwrap();

    match received_cmd {
        ControlEvent::Command { from, mode, text } => {
            assert_eq!(from, PeerId::new("player-a"));
            assert!(matches!(mode, CommandMode::Parse));
            assert_eq!(text, "A minor 90bpm");
        }
        _ => panic!("Command バリアントが期待された"),
    }

    // Step 3: Cadence が ACK
    let ack = ControlEvent::CommandAck {
        status: CommandStatus::Accepted,
        message: "A minor @ 90bpm で演奏開始".into(),
    };
    let ack_json = serde_json::to_string(&ack).unwrap();
    let received_ack: ControlEvent = serde_json::from_str(&ack_json).unwrap();

    match received_ack {
        ControlEvent::CommandAck { status, message } => {
            assert!(matches!(status, CommandStatus::Accepted));
            assert!(message.contains("90bpm"));
        }
        _ => panic!("CommandAck バリアントが期待された"),
    }
}

/// stop コマンドのプロトコルフロー。
#[test]
fn stop_command_protocol_flow() {
    let command = ControlEvent::Command {
        from: PeerId::new("player-a"),
        mode: CommandMode::Parse,
        text: "stop".into(),
    };

    let json = serde_json::to_string(&command).unwrap();
    let received: ControlEvent = serde_json::from_str(&json).unwrap();

    match &received {
        ControlEvent::Command { text, .. } => {
            assert_eq!(text, "stop");
        }
        _ => panic!("Command バリアントが期待された"),
    }

    // Cadence が停止 ACK を返す
    let ack = ControlEvent::CommandAck {
        status: CommandStatus::Accepted,
        message: "演奏停止".into(),
    };
    let ack_json = serde_json::to_string(&ack).unwrap();
    let received_ack: ControlEvent = serde_json::from_str(&ack_json).unwrap();

    match received_ack {
        ControlEvent::CommandAck { status, message } => {
            assert!(matches!(status, CommandStatus::Accepted));
            assert_eq!(message, "演奏停止");
        }
        _ => panic!("CommandAck バリアントが期待された"),
    }
}

/// serde_json::Value 経由のラウンドトリップ（Unison Protocol がValue を使うため）。
/// 実際のネットワーク層では `serde_json::Value` を介してイベントをやり取りする。
#[test]
fn command_roundtrip_via_json_value() {
    let command = ControlEvent::Command {
        from: PeerId::new("player-a"),
        mode: CommandMode::Parse,
        text: "C major 120bpm".into(),
    };

    // Value 経由（ControlHandler::broadcast と同じパス）
    let value = serde_json::to_value(&command).expect("to_value に失敗");
    let deserialized: ControlEvent = serde_json::from_value(value).expect("from_value に失敗");

    match deserialized {
        ControlEvent::Command { from, mode, text } => {
            assert_eq!(from, PeerId::new("player-a"));
            assert!(matches!(mode, CommandMode::Parse));
            assert_eq!(text, "C major 120bpm");
        }
        _ => panic!("Command バリアントが期待された"),
    }

    // CommandAck も Value 経由
    let ack = ControlEvent::CommandAck {
        status: CommandStatus::Rejected,
        message: "プラグインが見つかりません".into(),
    };
    let value = serde_json::to_value(&ack).unwrap();
    let deserialized: ControlEvent = serde_json::from_value(value).unwrap();

    match deserialized {
        ControlEvent::CommandAck { status, message } => {
            assert!(matches!(status, CommandStatus::Rejected));
            assert!(message.contains("プラグイン"));
        }
        _ => panic!("CommandAck バリアントが期待された"),
    }
}

use cplp_network::control::CommandMode;

use crate::midi_types::MidiSequence;
use crate::parser;

/// コマンドルーティング結果
pub enum RouteResult {
    /// ローカルパース成功
    Parsed(MidiSequence),
    /// Claude SDK に委譲（テキストをそのまま渡す）
    DelegateToLlm(String),
    /// パースエラー
    Error(String),
}

/// テキストコマンドを適切なハンドラにルーティング
pub fn route_command(mode: &CommandMode, text: &str) -> RouteResult {
    match mode {
        CommandMode::Parse => match parser::parse_command(text) {
            Ok(seq) => RouteResult::Parsed(seq),
            Err(e) => RouteResult::Error(e.to_string()),
        },
        CommandMode::Ask => RouteResult::DelegateToLlm(text.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_parse_mode_success() {
        let result = route_command(&CommandMode::Parse, "C major 120bpm");
        assert!(matches!(result, RouteResult::Parsed(_)));
    }

    #[test]
    fn route_parse_mode_error() {
        let result = route_command(&CommandMode::Parse, "gibberish nonsense");
        assert!(matches!(result, RouteResult::Error(_)));
    }

    #[test]
    fn route_ask_mode_delegates() {
        let result = route_command(&CommandMode::Ask, "ブルースっぽいバッキング弾いて");
        match result {
            RouteResult::DelegateToLlm(text) => {
                assert_eq!(text, "ブルースっぽいバッキング弾いて");
            }
            _ => panic!("Expected DelegateToLlm"),
        }
    }
}

// ── 内部統合テスト: コンポーネント間のパイプラインを検証 ──────────

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::sequencer::{MidiSequencer, NoteCommand};

    /// parse コマンド → MidiSequencer パイプライン統合テスト。
    /// パーサーが生成したシーケンスをシーケンサーにセットし、
    /// NoteOn イベントが正しく発火されることを確認する。
    #[test]
    fn parse_then_sequence_integration() {
        // パーサーでコマンドをシーケンスに変換
        let seq = parser::parse_command("C major 120bpm").expect("C major のパースに失敗");
        assert!((seq.tempo_bpm - 120.0).abs() < f32::EPSILON);
        assert!(!seq.events.is_empty());
        assert_eq!(seq.events[0].note, 60); // C4

        // シーケンサーにセットして再生
        let mut sequencer = MidiSequencer::new();
        sequencer.set_sequence(seq);
        assert!(sequencer.is_playing());

        // 初回 update で時刻を初期化
        let cmds = sequencer.update(0.0);
        assert!(cmds.is_empty(), "初回 update はイベントなし");

        // 0.01 秒後: C4 の NoteOn が発火するはず
        let cmds = sequencer.update(0.01);
        assert!(
            cmds.iter()
                .any(|c| matches!(c, NoteCommand::NoteOn { note: 60, .. })),
            "C4 (note=60) の NoteOn が発火されるべき"
        );
    }

    /// CommandRouter → パーサー → シーケンスのパイプライン統合テスト。
    /// ルーターが Parse モードで受けたコマンドを正しくパースし、
    /// 生成されたシーケンスのルートノートとテンポを検証する。
    #[test]
    fn route_parse_to_sequence() {
        let result = route_command(&CommandMode::Parse, "A minor 90bpm");

        match result {
            RouteResult::Parsed(seq) => {
                // テンポが 90 BPM であること
                assert!(
                    (seq.tempo_bpm - 90.0).abs() < f32::EPSILON,
                    "テンポは 90 BPM であるべき (実際: {})",
                    seq.tempo_bpm
                );

                // シーケンスにイベントが存在すること
                assert!(!seq.events.is_empty(), "イベントが生成されるべき");

                // 最初のノートが A4 (69) であること
                assert_eq!(
                    seq.events[0].note, 69,
                    "ルートノートは A4 (69) であるべき (実際: {})",
                    seq.events[0].note
                );

                // A minor スケール: A(69), B(71), C(72), D(74), E(76), F(77), G(79), A(81)
                // 上昇 8 音 + 下降 7 音（最高音を除く）= 15 イベント
                assert_eq!(
                    seq.events.len(),
                    15,
                    "上昇+下降で 15 イベントであるべき (実際: {})",
                    seq.events.len()
                );
            }
            RouteResult::Error(e) => panic!("パースが成功するべきだが、エラー: {e}"),
            RouteResult::DelegateToLlm(_) => panic!("Parse モードなので委譲されるべきではない"),
        }
    }

    /// stop コマンドフロー統合テスト。
    /// stop → テンポ 0.0 のシーケンス → シーケンサーが停止する一連の流れを検証。
    #[test]
    fn stop_command_flow() {
        // まずスケールを演奏中にする
        let scale_seq = parser::parse_command("C major 120bpm").expect("C major のパースに失敗");
        let mut sequencer = MidiSequencer::new();
        sequencer.set_sequence(scale_seq);
        assert!(sequencer.is_playing(), "演奏中であるべき");

        // stop コマンドをパース
        let stop_seq = parser::parse_command("stop").expect("stop のパースに失敗");
        assert_eq!(stop_seq.tempo_bpm, 0.0, "stop はテンポ 0.0 を生成するべき");
        assert!(stop_seq.events.is_empty(), "stop はイベントなしであるべき");

        // 停止シーケンスをセット → シーケンサーが停止
        sequencer.set_sequence(stop_seq);
        assert!(
            !sequencer.is_playing(),
            "stop 後はシーケンサーが停止しているべき"
        );

        // update しても何も返らない
        let cmds = sequencer.update(0.0);
        assert!(cmds.is_empty(), "停止後は NoteCommand が空であるべき");
        let cmds = sequencer.update(0.01);
        assert!(cmds.is_empty(), "停止後は NoteCommand が空であるべき");
    }
}

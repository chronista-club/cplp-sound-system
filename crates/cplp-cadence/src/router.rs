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

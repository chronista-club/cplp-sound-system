use crate::midi_types::{MidiEvent, MidiSequence, TICKS_PER_QUARTER};

/// デフォルトテンポ (BPM)
const DEFAULT_TEMPO: f32 = 122.0;

/// デフォルトの小節数
const DEFAULT_BARS: u32 = 2;

/// パースエラー
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("空の入力")]
    EmptyInput,
    #[error("不明なノート名: {0}")]
    UnknownNote(String),
    #[error("不明なスケール名: {0}")]
    UnknownScale(String),
    #[error("不明なコマンド: {0}")]
    UnknownCommand(String),
    #[error("テンポの解析に失敗: {0}")]
    InvalidTempo(String),
}

/// テキストコマンドを `MidiSequence` に変換する
pub fn parse_command(input: &str) -> Result<MidiSequence, ParseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ParseError::EmptyInput);
    }

    let lower = input.to_ascii_lowercase();

    // "stop" → テンポ 0.0 の停止シグナル
    if lower == "stop" {
        return Ok(MidiSequence::new(0.0));
    }

    // "tempo <N>" → テンポ変更のみ
    if lower.starts_with("tempo ") {
        let rest = lower.strip_prefix("tempo ").unwrap().trim();
        let bpm: f32 = rest
            .parse()
            .map_err(|_| ParseError::InvalidTempo(rest.to_string()))?;
        return Ok(MidiSequence::new(bpm));
    }

    // スケールコマンド: "<Note> <scale> [<N>bpm] [<N>bars]"
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.len() < 2 {
        return Err(ParseError::UnknownCommand(input.to_string()));
    }

    let root_midi = note_name_to_midi(tokens[0])?;
    let intervals = scale_intervals(tokens[1])?;

    let mut bpm = DEFAULT_TEMPO;
    let mut bars = DEFAULT_BARS;

    for token in &tokens[2..] {
        if let Some(bpm_str) = token.strip_suffix("bpm") {
            bpm = bpm_str
                .parse()
                .map_err(|_| ParseError::InvalidTempo(token.to_string()))?;
        } else if let Some(bars_str) = token.strip_suffix("bars") {
            bars = bars_str
                .parse()
                .map_err(|_| ParseError::UnknownCommand(token.to_string()))?;
        } else {
            return Err(ParseError::UnknownCommand(input.to_string()));
        }
    }

    let seq = build_scale_sequence(root_midi, intervals, bpm, bars);
    Ok(seq)
}

/// ノート名を MIDI ノート番号に変換する (C4 = 60)
pub fn note_name_to_midi(name: &str) -> Result<u8, ParseError> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "c" => Ok(60),
        "d" => Ok(62),
        "e" => Ok(64),
        "f" => Ok(65),
        "g" => Ok(67),
        "a" => Ok(69),
        "b" => Ok(71),
        _ => Err(ParseError::UnknownNote(name.to_string())),
    }
}

/// スケール名からインターバル配列を返す
pub fn scale_intervals(name: &str) -> Result<&'static [u8], ParseError> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "major" => Ok(&[0, 2, 4, 5, 7, 9, 11, 12]),
        "minor" => Ok(&[0, 2, 3, 5, 7, 8, 10, 12]),
        "pentatonic" => Ok(&[0, 2, 4, 7, 9, 12]),
        "blues" => Ok(&[0, 3, 5, 6, 7, 10, 12]),
        _ => Err(ParseError::UnknownScale(name.to_string())),
    }
}

/// スケールの上昇＋下降シーケンスを構築する
fn build_scale_sequence(root: u8, intervals: &[u8], bpm: f32, bars: u32) -> MidiSequence {
    let mut seq = MidiSequence::new(bpm);

    // 1小節 = 4拍 = 4 * TICKS_PER_QUARTER ticks
    let ticks_per_bar = 4 * TICKS_PER_QUARTER;
    let total_ticks = ticks_per_bar * bars as u64;

    // 上昇 + 下降（最高音を重複させない）
    let mut scale_notes: Vec<u8> = intervals.iter().map(|&i| root + i).collect();
    // 下降: 最高音を除いた逆順
    let descending: Vec<u8> = scale_notes[..scale_notes.len() - 1]
        .iter()
        .rev()
        .copied()
        .collect();
    scale_notes.extend(descending);

    if scale_notes.is_empty() {
        return seq;
    }

    // 各ノートの長さを均等に割り当て
    let note_count = scale_notes.len() as u64;
    let note_duration = total_ticks / note_count;

    for (i, &note) in scale_notes.iter().enumerate() {
        seq.events.push(MidiEvent {
            tick: i as u64 * note_duration,
            note,
            velocity: 100,
            duration_ticks: note_duration,
        });
    }

    seq
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stop() {
        let seq = parse_command("stop").unwrap();
        assert_eq!(seq.tempo_bpm, 0.0);
        assert!(seq.events.is_empty());
    }

    #[test]
    fn parse_tempo_change() {
        let seq = parse_command("tempo 140").unwrap();
        assert!((seq.tempo_bpm - 140.0).abs() < f32::EPSILON);
        assert!(seq.events.is_empty());
    }

    #[test]
    fn parse_c_major_scale() {
        let seq = parse_command("C major 120bpm").unwrap();
        assert!((seq.tempo_bpm - 120.0).abs() < f32::EPSILON);
        assert!(!seq.events.is_empty());
        assert_eq!(seq.events[0].note, 60); // C4
        assert_eq!(seq.events[1].note, 62); // D4
    }

    #[test]
    fn parse_default_bpm_is_122() {
        let seq = parse_command("A minor").unwrap();
        assert!((seq.tempo_bpm - 122.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_a_minor_pentatonic() {
        let seq = parse_command("A pentatonic 90bpm 4bars").unwrap();
        assert!((seq.tempo_bpm - 90.0).abs() < f32::EPSILON);
        assert_eq!(seq.events[0].note, 69); // A4
    }

    #[test]
    fn parse_unknown_returns_error() {
        assert!(parse_command("").is_err());
        assert!(parse_command("dance salsa").is_err());
    }

    #[test]
    fn note_name_to_midi_basics() {
        assert_eq!(note_name_to_midi("C").unwrap(), 60);
        assert_eq!(note_name_to_midi("A").unwrap(), 69);
        assert_eq!(note_name_to_midi("G").unwrap(), 67);
    }
}

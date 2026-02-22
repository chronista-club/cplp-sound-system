/// MIDI シーケンス（パーサーまたは LLM の出力）
#[derive(Debug, Clone)]
pub struct MidiSequence {
    pub tempo_bpm: f32,
    pub events: Vec<MidiEvent>,
}

#[derive(Debug, Clone)]
pub struct MidiEvent {
    /// タイミング（ティック、4分音符 = 480 ticks）
    pub tick: u64,
    /// MIDI ノート番号 (0-127)
    pub note: u8,
    /// ベロシティ (0-127)
    pub velocity: u8,
    /// ノート長（ティック）
    pub duration_ticks: u64,
}

/// ティック解像度: 4分音符あたりのティック数
pub const TICKS_PER_QUARTER: u64 = 480;

impl MidiSequence {
    pub fn new(tempo_bpm: f32) -> Self {
        Self {
            tempo_bpm,
            events: Vec::new(),
        }
    }

    /// 全イベントの終了時刻（最後のノートオフ）
    pub fn duration_ticks(&self) -> u64 {
        self.events
            .iter()
            .map(|e| e.tick + e.duration_ticks)
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_sequence_duration() {
        let seq = MidiSequence::new(122.0);
        assert_eq!(seq.duration_ticks(), 0);
    }

    #[test]
    fn sequence_duration_tracks_last_note() {
        let seq = MidiSequence {
            tempo_bpm: 122.0,
            events: vec![
                MidiEvent {
                    tick: 0,
                    note: 60,
                    velocity: 100,
                    duration_ticks: 480,
                },
                MidiEvent {
                    tick: 480,
                    note: 62,
                    velocity: 100,
                    duration_ticks: 960,
                },
            ],
        };
        assert_eq!(seq.duration_ticks(), 1440);
    }
}

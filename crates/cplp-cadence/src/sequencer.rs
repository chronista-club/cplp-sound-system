use crate::midi_types::{MidiSequence, TICKS_PER_QUARTER};

/// MIDI イベントをリアルタイムにスケジュールし、NoteOn/Off を送信する
pub struct MidiSequencer {
    sequence: Option<MidiSequence>,
    current_tick: u64,
    last_time: Option<f64>,
    pub looping: bool,
}

/// NoteOn/Off イベント（オーディオスレッドに送る）
#[derive(Debug, Clone)]
pub enum NoteCommand {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    Stop,
}

impl MidiSequencer {
    pub fn new() -> Self {
        Self {
            sequence: None,
            current_tick: 0,
            last_time: None,
            looping: true,
        }
    }

    /// 新しいシーケンスをセット（演奏開始）
    pub fn set_sequence(&mut self, seq: MidiSequence) {
        if seq.tempo_bpm == 0.0 {
            self.sequence = None;
            self.current_tick = 0;
            return;
        }
        self.sequence = Some(seq);
        self.current_tick = 0;
        self.last_time = None;
    }

    pub fn stop(&mut self) {
        self.sequence = None;
        self.current_tick = 0;
    }

    pub fn is_playing(&self) -> bool {
        self.sequence.is_some()
    }

    /// 時間経過分のイベントを収集する
    /// `current_time`: 秒単位の現在時刻（monotonic）
    pub fn update(&mut self, current_time: f64) -> Vec<NoteCommand> {
        let seq = match &self.sequence {
            Some(s) => s,
            None => return Vec::new(),
        };

        let prev_time = match self.last_time {
            Some(t) => t,
            None => {
                self.last_time = Some(current_time);
                return Vec::new();
            }
        };

        let dt = current_time - prev_time;
        self.last_time = Some(current_time);

        let ticks_per_sec = (seq.tempo_bpm as f64 / 60.0) * TICKS_PER_QUARTER as f64;
        let delta_ticks = (dt * ticks_per_sec) as u64;
        let prev_tick = self.current_tick;
        self.current_tick += delta_ticks;

        let duration = seq.duration_ticks();
        if duration == 0 {
            return Vec::new();
        }

        let mut commands = Vec::new();

        let effective_prev = if self.looping {
            prev_tick % duration
        } else {
            prev_tick
        };
        let window_end = effective_prev + delta_ticks;

        // ループ巻き戻し: window が duration を跨ぐ場合、2 区間に分割
        let wraps = self.looping && window_end > duration;

        for event in &seq.events {
            let on_tick = event.tick;
            let in_window = if wraps {
                // [effective_prev, duration) or [0, window_end % duration)
                on_tick >= effective_prev || on_tick < window_end % duration
            } else {
                on_tick >= effective_prev && on_tick < window_end
            };
            if in_window {
                commands.push(NoteCommand::NoteOn {
                    note: event.note,
                    velocity: event.velocity,
                });
            }

            let off_tick = event.tick + event.duration_ticks;
            let off_in_window = if wraps {
                off_tick >= effective_prev || off_tick < window_end % duration
            } else {
                off_tick >= effective_prev && off_tick < window_end
            };
            if off_in_window {
                commands.push(NoteCommand::NoteOff { note: event.note });
            }
        }

        if self.looping && self.current_tick >= duration {
            self.current_tick %= duration;
        }

        if !self.looping && self.current_tick >= duration {
            self.sequence = None;
        }

        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_types::MidiEvent;

    fn make_test_sequence() -> MidiSequence {
        MidiSequence {
            tempo_bpm: 120.0, // 120BPM = 960 ticks/sec
            events: vec![
                MidiEvent {
                    tick: 0,
                    note: 60,
                    velocity: 100,
                    duration_ticks: 240,
                },
                MidiEvent {
                    tick: 480,
                    note: 62,
                    velocity: 100,
                    duration_ticks: 240,
                },
            ],
        }
    }

    #[test]
    fn sequencer_starts_not_playing() {
        let seq = MidiSequencer::new();
        assert!(!seq.is_playing());
    }

    #[test]
    fn sequencer_plays_after_set() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());
        assert!(seq.is_playing());
    }

    #[test]
    fn sequencer_stop_command() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());
        seq.set_sequence(MidiSequence::new(0.0));
        assert!(!seq.is_playing());
    }

    #[test]
    fn sequencer_emits_note_on_at_start() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());
        let cmds = seq.update(0.0); // init
        assert!(cmds.is_empty());
        let cmds = seq.update(0.01); // 0.01s later
        assert!(
            cmds.iter()
                .any(|c| matches!(c, NoteCommand::NoteOn { note: 60, .. }))
        );
    }

    #[test]
    fn sequencer_emits_note_off() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());
        seq.update(0.0);
        seq.update(0.01); // NoteOn at tick 0
        let cmds = seq.update(0.26); // ~240 ticks later = NoteOff
        assert!(
            cmds.iter()
                .any(|c| matches!(c, NoteCommand::NoteOff { note: 60 }))
        );
    }

    #[test]
    fn sequencer_manual_stop() {
        let mut seq = MidiSequencer::new();
        seq.set_sequence(make_test_sequence());
        seq.stop();
        assert!(!seq.is_playing());
    }
}

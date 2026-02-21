/// AudioMixer: ローカル + リモートオーディオのミキシング
///
/// REQ-AUDIO-003: 相手のオーディオと自分のオーディオのミキシング
pub struct AudioMixer {
    pub local_gain: f32,
    pub remote_gain: f32,
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self {
            local_gain: 1.0,
            remote_gain: 1.0,
        }
    }
}

impl AudioMixer {
    /// サンプルレベルで加算ミキシング
    pub fn mix(&self, local: &[f32], remote: &[f32], output: &mut [f32]) {
        for (i, out) in output.iter_mut().enumerate() {
            let l = local.get(i).copied().unwrap_or(0.0);
            let r = remote.get(i).copied().unwrap_or(0.0);
            *out = (l * self.local_gain + r * self.remote_gain).clamp(-1.0, 1.0);
        }
    }
}

use cplp_core::{MixerState, PeerId};
use std::collections::HashMap;

/// N トラック対応ミキシング（MixerState 適用）
///
/// stereo interleaved フォーマット前提（偶数インデックス=L、奇数=R）。
/// パンは equal-power panning: L = cos(θ), R = sin(θ) where θ = (pan+1)/2 * π/2
pub fn mix_with_state(
    local_id: &PeerId,
    state: &MixerState,
    local_buf: &[f32],
    remote_bufs: &HashMap<PeerId, Vec<f32>>,
    output: &mut [f32],
) {
    let has_solo = state.has_solo();
    let channels = 2; // stereo

    // 出力をゼロクリア
    output.iter_mut().for_each(|s| *s = 0.0);

    // ローカルトラック
    if let Some(track) = state.tracks.get(local_id) {
        if should_output(track, has_solo) {
            add_track(local_buf, track, channels, output);
        }
    }

    // リモートトラック
    for (peer_id, buf) in remote_bufs {
        if let Some(track) = state.tracks.get(peer_id) {
            if should_output(track, has_solo) {
                add_track(buf, track, channels, output);
            }
        }
    }

    // マスターボリューム + クランプ
    let master = state.master_volume;
    for sample in output.iter_mut() {
        *sample = (*sample * master).clamp(-1.0, 1.0);
    }
}

/// トラックが出力されるべきか判定
fn should_output(track: &cplp_core::TrackState, has_solo: bool) -> bool {
    if track.mute {
        return false;
    }
    if has_solo && !track.solo {
        return false;
    }
    true
}

/// トラックを出力バッファに加算（volume + pan 適用）
fn add_track(src: &[f32], track: &cplp_core::TrackState, channels: usize, output: &mut [f32]) {
    // Equal-power panning: θ = (pan + 1) / 2 * π/2
    let theta = (track.pan + 1.0) / 2.0 * std::f32::consts::FRAC_PI_2;
    let gain_l = theta.cos() * track.volume;
    let gain_r = theta.sin() * track.volume;

    for (i, frame) in src.chunks(channels).enumerate() {
        let out_idx = i * channels;
        if out_idx + 1 < output.len() && frame.len() >= 2 {
            output[out_idx] += frame[0] * gain_l;
            output[out_idx + 1] += frame[1] * gain_r;
        } else if out_idx < output.len() && !frame.is_empty() {
            // mono fallback
            output[out_idx] += frame[0] * track.volume;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cplp_core::{MixerState, PeerId, TrackState};
    use std::collections::HashMap;

    #[test]
    fn mix_basic() {
        let mixer = AudioMixer::default();
        let local = [0.5, -0.3];
        let remote = [0.3, 0.2];
        let mut output = [0.0; 2];
        mixer.mix(&local, &remote, &mut output);
        assert!((output[0] - 0.8).abs() < f32::EPSILON);
        assert!((output[1] - -0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn mix_clamps() {
        let mixer = AudioMixer {
            local_gain: 1.0,
            remote_gain: 1.0,
        };
        let local = [0.8];
        let remote = [0.8];
        let mut output = [0.0; 1];
        mixer.mix(&local, &remote, &mut output);
        assert_eq!(output[0], 1.0);
    }

    #[test]
    fn mix_multi_tracks_with_mixer_state() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer_a = PeerId::new("peer-a");
        let peer_b = PeerId::new("peer-b");

        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer_a.clone(), TrackState::new("Guitar"));
        state.add_track(peer_b.clone(), TrackState::new("Bass"));

        state.apply_fader(&peer_b, 0.5, 1);

        let local_buf = vec![0.4, 0.4];
        let mut remote_bufs = HashMap::new();
        remote_bufs.insert(peer_a.clone(), vec![0.2, 0.2]);
        remote_bufs.insert(peer_b.clone(), vec![0.6, 0.6]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // local(0.4*0.707) + peer_a(0.2*0.707) + peer_b(0.6*0.5*0.707) ≈ 0.636
        let expected_per_ch: f32 = 0.4 * 0.707 + 0.2 * 0.707 + 0.6 * 0.5 * 0.707;
        assert!((output[0] - expected_per_ch).abs() < 0.02);
        assert!((output[1] - expected_per_ch).abs() < 0.02);
    }

    #[test]
    fn mix_with_mute() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer_a = PeerId::new("peer-a");

        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer_a.clone(), TrackState::new("Guitar"));
        state.apply_mute(&peer_a, true, 1);

        let local_buf = vec![0.5, 0.5];
        let mut remote_bufs = HashMap::new();
        remote_bufs.insert(peer_a.clone(), vec![0.5, 0.5]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // peer_a is muted → only local: 0.5 * cos(π/4) ≈ 0.354
        assert!(output[0] > 0.3 && output[0] < 0.4);
        assert!(output[1] > 0.3 && output[1] < 0.4);
    }

    #[test]
    fn mix_with_solo() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer_a = PeerId::new("peer-a");
        let peer_b = PeerId::new("peer-b");

        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer_a.clone(), TrackState::new("Guitar"));
        state.add_track(peer_b.clone(), TrackState::new("Bass"));
        state.apply_solo(&peer_a, true, 1);

        let local_buf = vec![0.3, 0.3];
        let mut remote_bufs = HashMap::new();
        remote_bufs.insert(peer_a.clone(), vec![0.4, 0.4]);
        remote_bufs.insert(peer_b.clone(), vec![0.5, 0.5]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // solo = peer_a only → 0.4 * cos(π/4) ≈ 0.283
        assert!(output[0] > 0.25 && output[0] < 0.32);
    }

    #[test]
    fn mix_with_pan_hard_right() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer_a = PeerId::new("peer-a");

        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer_a.clone(), TrackState::new("Guitar"));
        state.apply_pan(&peer_a, 1.0, 1); // hard right

        let local_buf = vec![0.0, 0.0]; // silent local
        let mut remote_bufs = HashMap::new();
        remote_bufs.insert(peer_a.clone(), vec![0.8, 0.8]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // hard right: θ = (1+1)/2 * π/2 = π/2, cos(π/2) ≈ 0, sin(π/2) = 1
        assert!(output[0].abs() < 0.01_f32); // L should be ~0
        assert!((output[1] - 0.8_f32).abs() < 0.01); // R should be 0.8
    }
}

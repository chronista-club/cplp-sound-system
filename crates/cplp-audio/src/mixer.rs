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

    // ─── CPS-25 Phase 2a: 境界値テスト ──────────────────────

    #[test]
    fn mix_gain_zero() {
        let mixer = AudioMixer {
            local_gain: 0.0,
            remote_gain: 1.0,
        };
        let local = [0.8, -0.5];
        let remote = [0.3, 0.2];
        let mut output = [0.0; 2];
        mixer.mix(&local, &remote, &mut output);
        // local 成分がゼロ → remote のみ
        assert!((output[0] - 0.3).abs() < f32::EPSILON);
        assert!((output[1] - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn mix_empty_slices() {
        let mixer = AudioMixer::default();
        let local: &[f32] = &[];
        let remote: &[f32] = &[];
        let mut output = [0.0; 4];
        // パニックしないことを検証
        mixer.mix(local, remote, &mut output);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn mix_output_shorter_than_input() {
        let mixer = AudioMixer::default();
        let local = [0.5, 0.6, 0.7, 0.8];
        let remote = [0.1, 0.1, 0.1, 0.1];
        let mut output = [0.0; 2]; // 入力より短い
        mixer.mix(&local, &remote, &mut output);
        // output の長さ分だけ処理される
        assert_eq!(output.len(), 2);
        assert!((output[0] - 0.6).abs() < f32::EPSILON);
        assert!((output[1] - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn mix_with_state_no_tracks() {
        let state = MixerState::new();
        let local_id = PeerId::new("local");
        let local_buf = vec![0.5, 0.5];
        let remote_bufs = HashMap::new();
        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);
        // トラックなし → 出力ゼロ
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn mix_with_state_master_volume_zero() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.apply_master(0.0, 1);

        let local_buf = vec![0.8, 0.8];
        let remote_bufs = HashMap::new();
        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn mix_with_state_master_volume_clamps() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        state.add_track(local_id.clone(), TrackState::new("Me"));
        // master_volume > 1.0 は apply_master で clamp されるが、
        // 直接設定して過大ゲインでもクランプされることを検証
        state.master_volume = 5.0;

        let local_buf = vec![0.9, 0.9];
        let remote_bufs = HashMap::new();
        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);
        // clamp で [-1.0, 1.0] に収まる
        for &s in &output {
            assert!(s >= -1.0 && s <= 1.0);
        }
    }

    #[test]
    fn mix_pan_hard_left() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer = PeerId::new("peer");
        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer.clone(), TrackState::new("Guitar"));
        state.apply_pan(&peer, -1.0, 1); // hard left

        let local_buf = vec![0.0, 0.0];
        let mut remote_bufs = HashMap::new();
        remote_bufs.insert(peer.clone(), vec![0.8, 0.8]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // hard left: θ = (-1+1)/2 * π/2 = 0, cos(0)=1, sin(0)=0
        assert!((output[0] - 0.8).abs() < 0.01); // L が最大
        assert!(output[1].abs() < 0.01);          // R がゼロ近似
    }

    #[test]
    fn mix_pan_center() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer = PeerId::new("peer");
        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer.clone(), TrackState::new("Guitar"));
        state.apply_pan(&peer, 0.0, 1); // center

        let local_buf = vec![0.0, 0.0];
        let mut remote_bufs = HashMap::new();
        remote_bufs.insert(peer.clone(), vec![1.0, 1.0]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // center: θ = π/4, cos(π/4) ≈ sin(π/4) ≈ 0.707
        let expected = std::f32::consts::FRAC_PI_4.cos(); // ≈ 0.707
        assert!((output[0] - expected).abs() < 0.01);
        assert!((output[1] - expected).abs() < 0.01);
    }

    #[test]
    fn should_output_muted_track_ignored() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer = PeerId::new("peer");
        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer.clone(), TrackState::new("Guitar"));
        state.apply_mute(&peer, true, 1);

        let local_buf = vec![0.0, 0.0];
        let mut remote_bufs = HashMap::new();
        remote_bufs.insert(peer.clone(), vec![1.0, 1.0]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // mute なので出力ゼロ
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn should_output_solo_multiple_tracks() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let pa = PeerId::new("pa");
        let pb = PeerId::new("pb");
        let pc = PeerId::new("pc");
        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(pa.clone(), TrackState::new("A"));
        state.add_track(pb.clone(), TrackState::new("B"));
        state.add_track(pc.clone(), TrackState::new("C"));
        state.apply_solo(&pa, true, 1);
        state.apply_solo(&pb, true, 1);
        // pc は solo OFF

        let local_buf = vec![0.0, 0.0];
        let mut remote_bufs = HashMap::new();
        remote_bufs.insert(pa.clone(), vec![0.5, 0.5]);
        remote_bufs.insert(pb.clone(), vec![0.5, 0.5]);
        remote_bufs.insert(pc.clone(), vec![0.5, 0.5]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // pa + pb が出力される（pc は solo 外で無視）
        // 0.5*0.707 * 2 = 0.707
        assert!(output[0] > 0.6);
        assert!(output[1] > 0.6);
    }

    #[test]
    fn add_track_mono_fallback() {
        let mut state = MixerState::new();
        let local_id = PeerId::new("local");
        let peer = PeerId::new("mono-peer");
        state.add_track(local_id.clone(), TrackState::new("Me"));
        state.add_track(peer.clone(), TrackState::new("Mono"));

        let local_buf = vec![0.0, 0.0];
        let mut remote_bufs = HashMap::new();
        // mono: 1ch のバッファ（奇数長 = 1サンプル）
        remote_bufs.insert(peer.clone(), vec![0.6]);

        let mut output = vec![0.0; 2];
        mix_with_state(&local_id, &state, &local_buf, &remote_bufs, &mut output);

        // mono fallback: frame[0] * volume が output[0] に加算
        assert!(output[0] > 0.0);
    }

    #[test]
    fn lww_fader_older_timestamp_rejected() {
        let mut state = MixerState::new();
        let peer = PeerId::new("peer");
        state.add_track(peer.clone(), TrackState::new("X"));
        state.apply_fader(&peer, 0.5, 100);
        // 古い ts → 拒否
        state.apply_fader(&peer, 0.9, 50);
        assert_eq!(state.tracks[&peer].volume, 0.5);
        assert_eq!(state.tracks[&peer].last_fader_ts, 100);
    }

    #[test]
    fn lww_pan_same_timestamp_rejected() {
        let mut state = MixerState::new();
        let peer = PeerId::new("peer");
        state.add_track(peer.clone(), TrackState::new("Y"));
        state.apply_pan(&peer, 0.3, 100);
        // 同値 ts → 拒否（> のみ許可）
        state.apply_pan(&peer, -0.8, 100);
        assert_eq!(state.tracks[&peer].pan, 0.3);
        assert_eq!(state.tracks[&peer].last_pan_ts, 100);
    }
}

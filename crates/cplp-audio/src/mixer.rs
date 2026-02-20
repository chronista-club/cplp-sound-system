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
        for i in 0..output.len() {
            let l = local.get(i).copied().unwrap_or(0.0);
            let r = remote.get(i).copied().unwrap_or(0.0);
            output[i] = (l * self.local_gain + r * self.remote_gain).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mixer = AudioMixer { local_gain: 1.0, remote_gain: 1.0 };
        let local = [0.8];
        let remote = [0.8];
        let mut output = [0.0; 1];
        mixer.mix(&local, &remote, &mut output);
        assert_eq!(output[0], 1.0);
    }
}

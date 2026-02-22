use atomic_float::AtomicF32;

/// オーディオスレッドから直接書き込まれるメーター値（最速パス）
pub struct AudioMeters {
    pub local_level: AtomicF32,
    pub local_peak: AtomicF32,
    pub remote_level: AtomicF32,
    pub remote_peak: AtomicF32,
}

impl Default for AudioMeters {
    fn default() -> Self {
        Self {
            local_level: AtomicF32::new(0.0),
            local_peak: AtomicF32::new(0.0),
            remote_level: AtomicF32::new(0.0),
            remote_peak: AtomicF32::new(0.0),
        }
    }
}

/// ネットワーク/セッション → HUD（triple buffer 経由）
#[derive(Clone, Default)]
pub struct SessionSnapshot {
    pub peer_name: String,
    pub connected: bool,
    pub latency_ms: f32,
    pub jitter_ms: f32,
    pub local_plugin: String,
    pub remote_plugin: String,
    pub mix_local: f32,
    pub mix_remote: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::Relaxed;

    #[test]
    fn audio_meters_default_is_zero() {
        let meters = AudioMeters::default();
        assert_eq!(meters.local_level.load(Relaxed), 0.0);
        assert_eq!(meters.local_peak.load(Relaxed), 0.0);
        assert_eq!(meters.remote_level.load(Relaxed), 0.0);
        assert_eq!(meters.remote_peak.load(Relaxed), 0.0);
    }

    #[test]
    fn audio_meters_atomic_write_read() {
        let meters = AudioMeters::default();
        meters.local_level.store(0.75, Relaxed);
        meters.remote_peak.store(0.95, Relaxed);
        assert!((meters.local_level.load(Relaxed) - 0.75).abs() < f32::EPSILON);
        assert!((meters.remote_peak.load(Relaxed) - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn session_snapshot_default() {
        let snap = SessionSnapshot::default();
        assert!(!snap.connected);
        assert_eq!(snap.peer_name, "");
        assert_eq!(snap.latency_ms, 0.0);
    }

    #[test]
    fn session_snapshot_triple_buffer() {
        let (mut input, mut output) = triple_buffer::triple_buffer(&SessionSnapshot::default());
        input.write(SessionSnapshot {
            peer_name: "Player B".into(),
            connected: true,
            latency_ms: 8.2,
            jitter_ms: 2.1,
            local_plugin: "Diva".into(),
            remote_plugin: "Vital".into(),
            mix_local: 0.7,
            mix_remote: 0.3,
        });
        let snap = output.read();
        assert_eq!(snap.peer_name, "Player B");
        assert!(snap.connected);
        assert!((snap.latency_ms - 8.2).abs() < f32::EPSILON);
        assert_eq!(snap.local_plugin, "Diva");
    }

    #[test]
    fn audio_meters_cross_thread() {
        use std::sync::Arc;
        use std::thread;

        let meters = Arc::new(AudioMeters::default());
        let meters_clone = meters.clone();

        let handle = thread::spawn(move || {
            meters_clone.local_level.store(0.5, Relaxed);
        });
        handle.join().unwrap();
        assert!((meters.local_level.load(Relaxed) - 0.5).abs() < f32::EPSILON);
    }
}

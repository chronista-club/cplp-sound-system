use mdns_sd::{ServiceDaemon, ServiceInfo};

/// mDNS サービスタイプ（cplp-session::discovery と同じ値）
const SERVICE_TYPE: &str = "_cplp-lobby._tcp.local.";

/// mDNS 広告のハンドル。Drop 時に自動 Unregister。
pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsAdvertiser {
    /// mDNS でロビーサーバーを広告開始する
    pub fn start(port: u16, mode: &str) -> anyhow::Result<Self> {
        let daemon = ServiceDaemon::new()?;

        let hostname = hostname::get()
            .unwrap_or_else(|_| "cplp-lobby".into())
            .to_string_lossy()
            .to_string();

        let instance_name = format!("cplp-lobby-{}", hostname);
        let version = env!("CARGO_PKG_VERSION");

        let properties = [("mode", mode), ("version", version)];

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &format!("{}.local.", hostname),
            "",
            port,
            &properties[..],
        )?;

        let fullname = service_info.get_fullname().to_string();
        daemon.register(service_info)?;

        tracing::info!(
            "mDNS 広告開始: {} (port: {}, mode: {}, version: {})",
            fullname,
            port,
            mode,
            version
        );

        Ok(Self { daemon, fullname })
    }
}

impl Drop for MdnsAdvertiser {
    fn drop(&mut self) {
        if let Err(e) = self.daemon.unregister(&self.fullname) {
            tracing::warn!("mDNS 広告解除に失敗: {}", e);
        } else {
            tracing::info!("mDNS 広告解除: {}", self.fullname);
        }
        self.daemon.shutdown().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_type_matches_discovery() {
        // discovery.rs と同じサービスタイプであることを確認
        assert_eq!(SERVICE_TYPE, "_cplp-lobby._tcp.local.");
    }
}

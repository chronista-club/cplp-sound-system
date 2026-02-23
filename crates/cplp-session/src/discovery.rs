use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};

/// mDNS サービスタイプ（ロビーとクライアントで共有）
pub const SERVICE_TYPE: &str = "_cplp-lobby._tcp.local.";

/// mDNS で発見されたロビーサーバー
#[derive(Debug, Clone)]
pub struct DiscoveredLobby {
    /// サービス名（ホスト名ベース）
    pub name: String,
    /// 接続用 URL (例: http://192.168.1.10:3000)
    pub url: String,
    /// ロビーモード (local / global)
    pub mode: String,
    /// サーバーバージョン
    pub version: String,
}

/// LAN 内の cplp-lobby サーバーを mDNS で発見する
pub async fn discover_lobbies(timeout: Duration) -> anyhow::Result<Vec<DiscoveredLobby>> {
    // mDNS デーモンはブロッキング API のため spawn_blocking で実行
    let lobbies = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<DiscoveredLobby>> {
        let mdns = ServiceDaemon::new()?;
        let receiver = mdns.browse(SERVICE_TYPE)?;

        let mut found: Vec<DiscoveredLobby> = Vec::new();
        let deadline = std::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    let addresses = info.get_addresses();
                    let Some(addr) = addresses.iter().next() else {
                        continue;
                    };
                    let port = info.get_port();
                    let url = format!("http://{}:{}", addr, port);

                    let properties = info.get_properties();
                    let mode = properties
                        .get_property_val_str("mode")
                        .unwrap_or("local")
                        .to_string();
                    let version = properties
                        .get_property_val_str("version")
                        .unwrap_or("unknown")
                        .to_string();

                    // 重複排除（同一 URL）
                    if !found.iter().any(|l| l.url == url) {
                        found.push(DiscoveredLobby {
                            name: info.get_fullname().to_string(),
                            url,
                            mode,
                            version,
                        });
                    }
                }
                Ok(_) => {
                    // SearchStarted, ServiceFound 等は無視
                }
                Err(_) => break,
            }
        }

        mdns.stop_browse(SERVICE_TYPE).ok();
        mdns.shutdown().ok();
        Ok(found)
    })
    .await??;

    Ok(lobbies)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_type_is_valid() {
        assert!(SERVICE_TYPE.starts_with('_'));
        assert!(SERVICE_TYPE.ends_with(".local."));
        assert!(SERVICE_TYPE.contains("._tcp"));
    }

    #[test]
    fn discovered_lobby_clone() {
        let lobby = DiscoveredLobby {
            name: "test._cplp-lobby._tcp.local.".into(),
            url: "http://192.168.1.10:3000".into(),
            mode: "local".into(),
            version: "0.2.0".into(),
        };
        let cloned = lobby.clone();
        assert_eq!(cloned.url, "http://192.168.1.10:3000");
        assert_eq!(cloned.mode, "local");
    }
}

//! ロギング初期化モジュール
//!
//! `CPLP_LOG` 環境変数によるプリセット制御、non-blocking 出力、
//! オプションのファイルログ出力を提供する。

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// `CPLP_LOG` プリセットをフィルタ文字列に変換
fn preset_to_filter(preset: &str) -> &str {
    match preset {
        "dev" => {
            "debug,cplp_audio=trace,cosmic_text=warn,naga=warn,wgpu_core=warn,wgpu_hal=warn,winit=warn"
        }
        "audio" => "info,cplp_audio=trace,cplp_audio::engine=trace",
        "network" => "info,cplp_network=debug,cplp_session=debug",
        "production" => "warn",
        _ => preset, // 不明なプリセットはそのままフィルタ文字列として使う
    }
}

/// フィルタ文字列を決定する（テスト可能な純粋関数版）
///
/// 優先順位:
/// 1. `cplp_log`（`CPLP_LOG` 環境変数相当）
/// 2. `rust_log`（`RUST_LOG` 環境変数相当）
/// 3. デフォルト `info`
fn resolve_filter_from(cplp_log: Option<&str>, rust_log: Option<&str>) -> String {
    if let Some(preset) = cplp_log {
        return preset_to_filter(preset).to_string();
    }
    if let Some(filter) = rust_log {
        return filter.to_string();
    }
    "info".to_string()
}

/// 環境変数からフィルタ文字列を決定する
fn resolve_filter() -> String {
    resolve_filter_from(
        std::env::var("CPLP_LOG").ok().as_deref(),
        std::env::var("RUST_LOG").ok().as_deref(),
    )
}

/// ロギングを初期化する
///
/// 全出力を non-blocking 化し、オプションでファイル出力レイヤーを追加する。
/// 返される `Vec<WorkerGuard>` はプロセス終了まで保持すること
/// （drop されると未 flush のログが失われる）。
pub fn init_logging(log_file: Option<&str>) -> Vec<WorkerGuard> {
    let filter_str = resolve_filter();
    let mut guards = Vec::new();

    // stdout non-blocking layer
    let (non_blocking_stdout, guard) = tracing_appender::non_blocking(std::io::stdout());
    guards.push(guard);

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_stdout)
        .with_target(true);

    let registry = tracing_subscriber::registry()
        .with(EnvFilter::new(&filter_str))
        .with(fmt_layer);

    // ファイル出力レイヤー（オプション）
    if let Some(path) = log_file {
        let parent = std::path::Path::new(path)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let filename = std::path::Path::new(path)
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("cplp.log"));

        let file_appender = tracing_appender::rolling::never(parent, filename);
        let (non_blocking_file, file_guard) = tracing_appender::non_blocking(file_appender);
        guards.push(file_guard);

        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(non_blocking_file)
            .with_target(true)
            .with_ansi(false)
            .json();

        registry.with(file_layer).init();
    } else {
        registry.init();
    }

    guards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_dev() {
        let filter = preset_to_filter("dev");
        assert!(filter.starts_with("debug,cplp_audio=trace"));
        assert!(filter.contains("cosmic_text=warn"));
        assert!(filter.contains("naga=warn"));
        assert!(filter.contains("wgpu_core=warn"));
    }

    #[test]
    fn test_preset_audio() {
        assert_eq!(
            preset_to_filter("audio"),
            "info,cplp_audio=trace,cplp_audio::engine=trace"
        );
    }

    #[test]
    fn test_preset_network() {
        assert_eq!(
            preset_to_filter("network"),
            "info,cplp_network=debug,cplp_session=debug"
        );
    }

    #[test]
    fn test_preset_production() {
        assert_eq!(preset_to_filter("production"), "warn");
    }

    #[test]
    fn test_preset_passthrough() {
        assert_eq!(
            preset_to_filter("debug,my_crate=trace"),
            "debug,my_crate=trace"
        );
    }

    #[test]
    fn test_resolve_filter_default() {
        assert_eq!(resolve_filter_from(None, None), "info");
    }

    #[test]
    fn test_resolve_filter_cplp_log_takes_priority() {
        let filter = resolve_filter_from(Some("dev"), Some("warn"));
        assert!(filter.starts_with("debug,cplp_audio=trace"));
    }

    #[test]
    fn test_resolve_filter_rust_log_fallback() {
        assert_eq!(
            resolve_filter_from(None, Some("warn,my_crate=debug")),
            "warn,my_crate=debug"
        );
    }

    #[test]
    fn test_resolve_filter_custom_cplp_log() {
        assert_eq!(
            resolve_filter_from(Some("trace,hyper=warn"), None),
            "trace,hyper=warn"
        );
    }
}

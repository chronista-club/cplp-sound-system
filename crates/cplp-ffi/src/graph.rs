//! AudioGraph FFI — Swift から AudioGraph を操作する

use std::ffi::CStr;

use cplp_core::audio_graph::{EdgeType, NodeId, NodeType};

use crate::{runtime, types::CplpResult};

/// AudioGraph にプラグインノードを追加
///
/// 戻り値: 追加されたノードの ID（0 はエラー）
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplp_graph_add_plugin(
    plugin_id: *const std::ffi::c_char,
    plugin_name: *const std::ffi::c_char,
    is_instrument: bool,
) -> u32 {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return 0,
    };

    let plugin_id = if plugin_id.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(plugin_id) }
            .to_string_lossy()
            .into_owned()
    };

    let plugin_name = if plugin_name.is_null() {
        plugin_id.clone()
    } else {
        unsafe { CStr::from_ptr(plugin_name) }
            .to_string_lossy()
            .into_owned()
    };

    let node_type = if is_instrument {
        NodeType::ClapInstrument {
            plugin_id: plugin_id.clone(),
        }
    } else {
        NodeType::ClapEffect {
            plugin_id: plugin_id.clone(),
        }
    };

    match rt.graph.lock() {
        Ok(mut graph) => {
            let node_id = graph.add_node(&plugin_name, node_type);
            tracing::info!("AudioGraph: added plugin '{}' as node {}", plugin_name, node_id);
            node_id
        }
        Err(_) => 0,
    }
}

/// AudioGraph のノード数を取得
#[unsafe(no_mangle)]
pub extern "C" fn cplp_graph_node_count() -> u32 {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return 0,
    };
    match rt.graph.lock() {
        Ok(graph) => graph.node_count() as u32,
        Err(_) => 0,
    }
}

/// AudioGraph からノードを削除
#[unsafe(no_mangle)]
pub extern "C" fn cplp_graph_remove_node(node_id: u32) -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };
    match rt.graph.lock() {
        Ok(mut graph) => {
            if graph.remove_node(node_id) {
                CplpResult::Ok
            } else {
                CplpResult::InvalidArgument
            }
        }
        Err(_) => CplpResult::InternalError,
    }
}

/// 2つのノードを接続
#[unsafe(no_mangle)]
pub extern "C" fn cplp_graph_connect(
    from_node: u32,
    to_node: u32,
    is_midi: bool,
) -> CplpResult {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return CplpResult::NotInitialized,
    };
    let edge_type = if is_midi {
        EdgeType::Midi
    } else {
        EdgeType::Audio
    };
    match rt.graph.lock() {
        Ok(mut graph) => {
            if graph.connect(from_node, to_node, edge_type).is_some() {
                CplpResult::Ok
            } else {
                CplpResult::InvalidArgument
            }
        }
        Err(_) => CplpResult::InternalError,
    }
}

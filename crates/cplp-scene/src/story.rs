//! Story DSL — KDL ベースのカスタムシーン記述言語
//!
//! ユーロラックのモジュール配置、パッチング、シーン遷移を宣言的に記述する。
//!
//! # KDL 構文例
//!
//! ```kdl
//! scene "live-setup" {
//!   rack rows=2 hp=84 {
//!     module "surge-xt" hp=16 row=0 position=0
//!     module "looper" hp=8 row=0 position=16
//!     module "beat-machine" hp=12 row=1 position=0
//!   }
//!   patch {
//!     connect from="surge-xt" to="looper" port="input"
//!     connect from="looper" to="master" port="output"
//!   }
//! }
//! ```

use std::collections::HashMap;

// ── エラー型 ────────────────────────────────────

/// Story DSL のパースエラー
#[derive(Debug, thiserror::Error)]
pub enum StoryError {
    /// KDL パースエラー
    #[error("KDL parse error: {0}")]
    Kdl(#[from] kdl::KdlError),
    /// 構文エラー（期待するノードや属性が見つからない）
    #[error("{0}")]
    Syntax(String),
}

// ── AST ─────────────────────────────────────────

/// Story ドキュメント全体（複数シーンを保持）
#[derive(Debug, Clone, PartialEq)]
pub struct Story {
    pub scenes: Vec<Scene>,
}

/// シーン定義
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// シーン名
    pub name: String,
    /// ラック定義（1 シーンに複数ラック可）
    pub racks: Vec<Rack>,
    /// パッチ定義（接続リスト）
    pub patches: Vec<Patch>,
    /// シーン遷移定義
    pub transitions: Vec<Transition>,
}

/// ユーロラック（物理フレーム）
#[derive(Debug, Clone, PartialEq)]
pub struct Rack {
    /// 行数（デフォルト 1）
    pub rows: u32,
    /// 1 行あたりの HP 幅（デフォルト 84）
    pub hp: u32,
    /// ラック内のモジュール配置
    pub modules: Vec<ModulePlacement>,
}

/// モジュール配置
#[derive(Debug, Clone, PartialEq)]
pub struct ModulePlacement {
    /// モジュール名（プラグイン ID としても使う）
    pub name: String,
    /// HP 幅
    pub hp: u32,
    /// 行番号（0 始まり）
    pub row: u32,
    /// 行内の HP 位置（0 始まり）
    pub position: u32,
    /// カスタム属性
    pub attrs: HashMap<String, AttrValue>,
}

/// パッチ定義（接続グループ）
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub connections: Vec<Connection>,
}

/// 接続
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    /// 送信元モジュール名
    pub from: String,
    /// 送信先モジュール名
    pub to: String,
    /// ポート名
    pub port: String,
    /// カスタム属性
    pub attrs: HashMap<String, AttrValue>,
}

/// シーン遷移
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    /// 遷移先シーン名
    pub to: String,
    /// トリガー条件
    pub trigger: String,
    /// フェード時間（秒）
    pub duration: f64,
}

/// 属性値
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

// ── シーングラフ（中間表現）─────────────────────

/// シーングラフ — レンダリングに必要な情報を保持
#[derive(Debug, Clone)]
pub struct SceneGraph {
    /// シーン名
    pub name: String,
    /// ラック情報
    pub rack_nodes: Vec<RackNode>,
    /// 接続情報
    pub connections: Vec<ConnectionEdge>,
}

/// ラックノード（レンダリング用）
#[derive(Debug, Clone)]
pub struct RackNode {
    pub rows: u32,
    pub hp: u32,
    pub modules: Vec<ModuleNode>,
}

/// モジュールノード（レンダリング用）
#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub name: String,
    pub hp: u32,
    pub row: u32,
    pub position: u32,
    /// ワールド座標（build_scene_graph で計算）
    pub world_position: [f32; 3],
}

/// 接続エッジ（レンダリング用）
#[derive(Debug, Clone)]
pub struct ConnectionEdge {
    pub from: String,
    pub to: String,
    pub port: String,
}

// ── パーサー ────────────────────────────────────

/// KDL テキストから Story AST をパース
pub fn parse_story(input: &str) -> Result<Story, StoryError> {
    let doc: kdl::KdlDocument = input.parse()?;
    let mut scenes = Vec::new();

    for node in doc.nodes() {
        if node.name().value() == "scene" {
            scenes.push(parse_scene(node)?);
        }
    }

    if scenes.is_empty() {
        return Err(StoryError::Syntax(
            "少なくとも 1 つの scene ノードが必要です".into(),
        ));
    }

    Ok(Story { scenes })
}

fn parse_scene(node: &kdl::KdlNode) -> Result<Scene, StoryError> {
    let name = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .ok_or_else(|| StoryError::Syntax("scene ノードに名前が必要です".into()))?
        .to_string();

    let children = node
        .children()
        .ok_or_else(|| StoryError::Syntax(format!("scene \"{}\" にボディが必要です", name)))?;

    let mut racks = Vec::new();
    let mut patches = Vec::new();
    let mut transitions = Vec::new();

    for child in children.nodes() {
        match child.name().value() {
            "rack" => racks.push(parse_rack(child)?),
            "patch" => patches.push(parse_patch(child)?),
            "transition" => transitions.push(parse_transition(child)?),
            other => {
                return Err(StoryError::Syntax(format!(
                    "scene 内の未知のノード: \"{}\"",
                    other,
                )));
            }
        }
    }

    Ok(Scene {
        name,
        racks,
        patches,
        transitions,
    })
}

fn parse_rack(node: &kdl::KdlNode) -> Result<Rack, StoryError> {
    let rows = get_entry_i64(node, "rows").unwrap_or(1) as u32;
    let hp = get_entry_i64(node, "hp").unwrap_or(84) as u32;

    let mut modules = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "module" {
                modules.push(parse_module(child)?);
            } else {
                return Err(StoryError::Syntax(format!(
                    "rack 内の未知のノード: \"{}\"",
                    child.name().value(),
                )));
            }
        }
    }

    Ok(Rack { rows, hp, modules })
}

fn parse_module(node: &kdl::KdlNode) -> Result<ModulePlacement, StoryError> {
    let name = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .ok_or_else(|| StoryError::Syntax("module ノードに名前が必要です".into()))?
        .to_string();

    let hp = get_entry_i64(node, "hp")
        .ok_or_else(|| StoryError::Syntax(format!("module \"{}\" に hp が必要です", name)))?
        as u32;

    let row = get_entry_i64(node, "row").unwrap_or(0) as u32;
    let position = get_entry_i64(node, "position").unwrap_or(0) as u32;

    // 既知のキー以外をカスタム属性として収集
    let known_keys = ["hp", "row", "position"];
    let mut attrs = HashMap::new();
    for entry in node.entries() {
        if let Some(name_entry) = entry.name() {
            let key = name_entry.value();
            if !known_keys.contains(&key) {
                attrs.insert(key.to_string(), kdl_value_to_attr(entry.value()));
            }
        }
    }

    Ok(ModulePlacement {
        name,
        hp,
        row,
        position,
        attrs,
    })
}

fn parse_patch(node: &kdl::KdlNode) -> Result<Patch, StoryError> {
    let mut connections = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "connect" {
                connections.push(parse_connection(child)?);
            } else {
                return Err(StoryError::Syntax(format!(
                    "patch 内の未知のノード: \"{}\"",
                    child.name().value(),
                )));
            }
        }
    }
    Ok(Patch { connections })
}

fn parse_connection(node: &kdl::KdlNode) -> Result<Connection, StoryError> {
    let from = get_entry_str(node, "from")
        .ok_or_else(|| StoryError::Syntax("connect に from が必要です".into()))?;
    let to = get_entry_str(node, "to")
        .ok_or_else(|| StoryError::Syntax("connect に to が必要です".into()))?;
    let port = get_entry_str(node, "port").unwrap_or_default();

    let known_keys = ["from", "to", "port"];
    let mut attrs = HashMap::new();
    for entry in node.entries() {
        if let Some(name_entry) = entry.name() {
            let key = name_entry.value();
            if !known_keys.contains(&key) {
                attrs.insert(key.to_string(), kdl_value_to_attr(entry.value()));
            }
        }
    }

    Ok(Connection {
        from,
        to,
        port,
        attrs,
    })
}

fn parse_transition(node: &kdl::KdlNode) -> Result<Transition, StoryError> {
    let to = get_entry_str(node, "to")
        .ok_or_else(|| StoryError::Syntax("transition に to が必要です".into()))?;
    let trigger = get_entry_str(node, "trigger").unwrap_or_default();
    let duration = get_entry_f64(node, "duration").unwrap_or(0.0);

    Ok(Transition {
        to,
        trigger,
        duration,
    })
}

// ── KDL ヘルパー ────────────────────────────────

fn get_entry_i64(node: &kdl::KdlNode, name: &str) -> Option<i64> {
    node.entries()
        .iter()
        .find(|e| e.name().map(|n| n.value()) == Some(name))
        .and_then(|e| e.value().as_integer())
        .map(|i| i as i64)
}

fn get_entry_f64(node: &kdl::KdlNode, name: &str) -> Option<f64> {
    node.entries()
        .iter()
        .find(|e| e.name().map(|n| n.value()) == Some(name))
        .and_then(|e| {
            e.value()
                .as_float()
                .or_else(|| e.value().as_integer().map(|i| i as f64))
        })
}

fn get_entry_str(node: &kdl::KdlNode, name: &str) -> Option<String> {
    node.entries()
        .iter()
        .find(|e| e.name().map(|n| n.value()) == Some(name))
        .and_then(|e| e.value().as_string())
        .map(|s| s.to_string())
}

fn kdl_value_to_attr(v: &kdl::KdlValue) -> AttrValue {
    if let Some(s) = v.as_string() {
        AttrValue::String(s.to_string())
    } else if let Some(i) = v.as_integer() {
        AttrValue::Int(i as i64)
    } else if let Some(f) = v.as_float() {
        AttrValue::Float(f)
    } else if let Some(b) = v.as_bool() {
        AttrValue::Bool(b)
    } else {
        AttrValue::String(format!("{}", v))
    }
}

// ── Scene → SceneGraph 変換 ─────────────────────

/// Scene AST からレンダリング用の SceneGraph を構築
pub fn build_scene_graph(scene: &Scene) -> SceneGraph {
    use crate::mesh::{HP_UNIT, PANEL_DEPTH, RAIL_DEPTH, ROW_HEIGHT_3U};

    let mut rack_nodes = Vec::new();

    for rack in &scene.racks {
        let total_hp = rack.hp;
        let total_width = total_hp as f32 * HP_UNIT;

        let modules = rack
            .modules
            .iter()
            .map(|m| {
                // module_world_position 相当のロジック
                let x = (m.position as f32 + m.hp as f32 / 2.0) * HP_UNIT - total_width / 2.0;
                let y = m.row as f32 * ROW_HEIGHT_3U + ROW_HEIGHT_3U / 2.0;
                let z = RAIL_DEPTH / 2.0 + PANEL_DEPTH / 2.0 + 0.001;

                ModuleNode {
                    name: m.name.clone(),
                    hp: m.hp,
                    row: m.row,
                    position: m.position,
                    world_position: [x, y, z],
                }
            })
            .collect();

        rack_nodes.push(RackNode {
            rows: rack.rows,
            hp: rack.hp,
            modules,
        });
    }

    // パッチの接続をフラット化
    let connections = scene
        .patches
        .iter()
        .flat_map(|p| {
            p.connections.iter().map(|c| ConnectionEdge {
                from: c.from.clone(),
                to: c.to.clone(),
                port: c.port.clone(),
            })
        })
        .collect();

    SceneGraph {
        name: scene.name.clone(),
        rack_nodes,
        connections,
    }
}

// ── テスト ──────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── パーサー正常系 ──────────────────────────

    #[test]
    fn parse_minimal_scene() {
        let input = r#"
scene "live-setup" {
    rack rows=1 hp=84 {
        module "surge-xt" hp=16 row=0 position=0
    }
    patch {
        connect from="surge-xt" to="master" port="output"
    }
}
"#;
        let story = parse_story(input).unwrap();
        assert_eq!(story.scenes.len(), 1);

        let scene = &story.scenes[0];
        assert_eq!(scene.name, "live-setup");
        assert_eq!(scene.racks.len(), 1);
        assert_eq!(scene.patches.len(), 1);

        let rack = &scene.racks[0];
        assert_eq!(rack.rows, 1);
        assert_eq!(rack.hp, 84);
        assert_eq!(rack.modules.len(), 1);

        let module = &rack.modules[0];
        assert_eq!(module.name, "surge-xt");
        assert_eq!(module.hp, 16);
        assert_eq!(module.row, 0);
        assert_eq!(module.position, 0);
    }

    #[test]
    fn parse_full_example() {
        let input = r#"
scene "live-setup" {
    rack rows=2 hp=84 {
        module "surge-xt" hp=16 row=0 position=0
        module "looper" hp=8 row=0 position=16
        module "beat-machine" hp=12 row=1 position=0
    }
    patch {
        connect from="surge-xt" to="looper" port="input"
        connect from="looper" to="master" port="output"
    }
}
"#;
        let story = parse_story(input).unwrap();
        let scene = &story.scenes[0];

        assert_eq!(scene.racks[0].modules.len(), 3);
        assert_eq!(scene.patches[0].connections.len(), 2);

        let conn = &scene.patches[0].connections[0];
        assert_eq!(conn.from, "surge-xt");
        assert_eq!(conn.to, "looper");
        assert_eq!(conn.port, "input");
    }

    #[test]
    fn parse_multiple_scenes() {
        let input = r#"
scene "setup-a" {
    rack rows=1 {
        module "synth" hp=16 row=0 position=0
    }
    patch {
    }
}
scene "setup-b" {
    rack rows=1 {
        module "drum" hp=8 row=0 position=0
    }
    patch {
    }
}
"#;
        let story = parse_story(input).unwrap();
        assert_eq!(story.scenes.len(), 2);
        assert_eq!(story.scenes[0].name, "setup-a");
        assert_eq!(story.scenes[1].name, "setup-b");
    }

    #[test]
    fn parse_with_transitions() {
        let input = r#"
scene "intro" {
    rack rows=1 {
        module "pad" hp=16 row=0 position=0
    }
    patch {
    }
    transition to="main" trigger="midi-cc-64" duration=2
}
"#;
        let story = parse_story(input).unwrap();
        let scene = &story.scenes[0];
        assert_eq!(scene.transitions.len(), 1);

        let t = &scene.transitions[0];
        assert_eq!(t.to, "main");
        assert_eq!(t.trigger, "midi-cc-64");
        assert_eq!(t.duration, 2.0);
    }

    #[test]
    fn parse_default_rack_values() {
        let input = r#"
scene "defaults" {
    rack {
        module "test" hp=4
    }
    patch {
    }
}
"#;
        let story = parse_story(input).unwrap();
        let rack = &story.scenes[0].racks[0];
        assert_eq!(rack.rows, 1);
        assert_eq!(rack.hp, 84);

        let module = &rack.modules[0];
        assert_eq!(module.row, 0);
        assert_eq!(module.position, 0);
    }

    #[test]
    fn parse_custom_attrs() {
        let input = r#"
scene "custom" {
    rack {
        module "synth" hp=16 row=0 position=0 color="red" gain=3
    }
    patch {
        connect from="synth" to="master" port="output" channel=1
    }
}
"#;
        let story = parse_story(input).unwrap();
        let module = &story.scenes[0].racks[0].modules[0];
        assert_eq!(module.attrs["color"], AttrValue::String("red".into()));
        assert_eq!(module.attrs["gain"], AttrValue::Int(3));

        let conn = &story.scenes[0].patches[0].connections[0];
        assert_eq!(conn.attrs["channel"], AttrValue::Int(1));
    }

    // ── パーサー異常系 ──────────────────────────

    #[test]
    fn error_no_scene() {
        let input = r#"
rack rows=1 {
    module "test" hp=4
}
"#;
        let err = parse_story(input).unwrap_err();
        assert!(matches!(err, StoryError::Syntax(_)));
    }

    #[test]
    fn error_scene_without_name() {
        let input = r#"
scene {
    rack {
        module "test" hp=4
    }
    patch {
    }
}
"#;
        let err = parse_story(input).unwrap_err();
        assert!(matches!(err, StoryError::Syntax(_)));
    }

    #[test]
    fn error_module_without_hp() {
        let input = r#"
scene "test" {
    rack {
        module "no-hp"
    }
    patch {
    }
}
"#;
        let err = parse_story(input).unwrap_err();
        assert!(matches!(err, StoryError::Syntax(_)));
    }

    #[test]
    fn error_connect_without_from() {
        let input = r#"
scene "test" {
    rack {
        module "synth" hp=8
    }
    patch {
        connect to="master" port="output"
    }
}
"#;
        let err = parse_story(input).unwrap_err();
        assert!(matches!(err, StoryError::Syntax(_)));
    }

    #[test]
    fn error_invalid_kdl_syntax() {
        let input = r#"scene "test" {{{{ broken"#;
        let err = parse_story(input).unwrap_err();
        assert!(matches!(err, StoryError::Kdl(_)));
    }

    #[test]
    fn error_unknown_node_in_scene() {
        let input = r#"
scene "test" {
    rack {
        module "synth" hp=8
    }
    patch {
    }
    foobar {
    }
}
"#;
        let err = parse_story(input).unwrap_err();
        assert!(matches!(err, StoryError::Syntax(_)));
    }

    // ── SceneGraph 変換 ─────────────────────────

    #[test]
    fn build_scene_graph_positions() {
        let input = r#"
scene "test" {
    rack rows=2 hp=84 {
        module "a" hp=16 row=0 position=0
        module "b" hp=8 row=0 position=16
        module "c" hp=12 row=1 position=0
    }
    patch {
        connect from="a" to="b" port="audio"
    }
}
"#;
        let story = parse_story(input).unwrap();
        let graph = build_scene_graph(&story.scenes[0]);

        assert_eq!(graph.name, "test");
        assert_eq!(graph.rack_nodes.len(), 1);
        assert_eq!(graph.rack_nodes[0].modules.len(), 3);
        assert_eq!(graph.connections.len(), 1);

        // モジュール "a": position=0, hp=16
        // x = (0 + 8) * 0.05 - 84*0.05/2 = 0.4 - 2.1 = -1.7
        let a = &graph.rack_nodes[0].modules[0];
        assert_eq!(a.name, "a");
        assert!((a.world_position[0] - (-1.7)).abs() < 0.001);

        // モジュール "c": row=1 なので y が高い
        let c = &graph.rack_nodes[0].modules[2];
        assert_eq!(c.name, "c");
        assert!(c.world_position[1] > a.world_position[1]);

        // 接続
        assert_eq!(graph.connections[0].from, "a");
        assert_eq!(graph.connections[0].to, "b");
        assert_eq!(graph.connections[0].port, "audio");
    }

    #[test]
    fn build_scene_graph_multiple_patches() {
        let input = r#"
scene "multi-patch" {
    rack {
        module "a" hp=8
        module "b" hp=8
    }
    patch {
        connect from="a" to="b" port="audio"
    }
    patch {
        connect from="b" to="master" port="output"
    }
}
"#;
        let story = parse_story(input).unwrap();
        let graph = build_scene_graph(&story.scenes[0]);

        // 複数 patch ブロックの接続がフラット化される
        assert_eq!(graph.connections.len(), 2);
    }
}

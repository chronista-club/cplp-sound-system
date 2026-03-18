//! 最小限の .usda (USD ASCII) パーサー
//!
//! USD 全仕様ではなく、Gig Scene に必要なサブセットのみ対応:
//! - ヘッダー (#usda 1.0) + メタデータ
//! - def <Type> "<Name>" { ... } によるプリミティブ定義
//! - プロパティ: 数値、配列、タプル、文字列
//! - ネストしたプリミティブ（シーングラフ）

use std::collections::HashMap;

// ── データ構造 ──────────────────────────────────

/// USD Stage（シーン全体）
#[derive(Debug)]
pub struct Stage {
    /// メタデータ (defaultPrim, upAxis など)
    pub metadata: HashMap<String, Value>,
    /// ルートレベルのプリミティブ
    pub prims: Vec<Prim>,
}

/// USD Prim（シーングラフのノード）
#[derive(Debug)]
pub struct Prim {
    /// プリミティブ型 (Xform, Mesh, Sphere, etc.)
    pub prim_type: String,
    /// 名前
    pub name: String,
    /// プロパティ
    pub properties: HashMap<String, Property>,
    /// 子プリミティブ
    pub children: Vec<Prim>,
}

/// プロパティ（型名 + 値）
#[derive(Debug)]
pub struct Property {
    /// USD 型名 (float3[], double, token[], etc.)
    pub type_name: String,
    /// 値
    pub value: Value,
}

/// USD の値
#[derive(Debug, Clone)]
pub enum Value {
    /// 整数
    Int(i64),
    /// 浮動小数点
    Float(f64),
    /// 文字列
    String(String),
    /// タプル (1, 2, 3)
    Tuple(Vec<Value>),
    /// 配列 [1, 2, 3]
    Array(Vec<Value>),
}

impl Value {
    /// f64 として取得
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// (f64, f64, f64) タプルとして取得
    pub fn as_f64x3(&self) -> Option<[f64; 3]> {
        match self {
            Value::Tuple(v) if v.len() == 3 => {
                Some([v[0].as_f64()?, v[1].as_f64()?, v[2].as_f64()?])
            }
            _ => None,
        }
    }

    /// 文字列として取得
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

// ── トークナイザ ────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// def キーワード
    Def,
    /// 識別子（型名、プロパティ名）
    Ident(String),
    /// 文字列リテラル "..."
    StringLit(String),
    /// 整数リテラル
    IntLit(i64),
    /// 浮動小数点リテラル
    FloatLit(f64),
    /// {
    BraceOpen,
    /// }
    BraceClose,
    /// (
    ParenOpen,
    /// )
    ParenClose,
    /// [
    BracketOpen,
    /// ]
    BracketClose,
    /// =
    Eq,
    /// ,
    Comma,
}

struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        Some(ch)
    }

    fn skip_whitespace_and_comments(&mut self) {
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch.is_whitespace() {
                self.pos += 1;
            } else if ch == '#' {
                // 行コメント（ただしヘッダー行 #usda は別扱い）
                while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn tokenize(&mut self) -> anyhow::Result<Vec<Token>> {
        let mut tokens = Vec::new();

        // ヘッダー行をスキップ (#usda 1.0)
        if self.chars.starts_with(&['#', 'u', 's', 'd', 'a']) {
            while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                self.pos += 1;
            }
        }

        loop {
            self.skip_whitespace_and_comments();
            let Some(ch) = self.peek() else { break };

            match ch {
                '{' => {
                    self.advance();
                    tokens.push(Token::BraceOpen);
                }
                '}' => {
                    self.advance();
                    tokens.push(Token::BraceClose);
                }
                '(' => {
                    self.advance();
                    tokens.push(Token::ParenOpen);
                }
                ')' => {
                    self.advance();
                    tokens.push(Token::ParenClose);
                }
                '[' => {
                    self.advance();
                    tokens.push(Token::BracketOpen);
                }
                ']' => {
                    self.advance();
                    tokens.push(Token::BracketClose);
                }
                '=' => {
                    self.advance();
                    tokens.push(Token::Eq);
                }
                ',' => {
                    self.advance();
                    tokens.push(Token::Comma);
                }
                '"' => {
                    tokens.push(self.read_string()?);
                }
                c if c == '-' || c.is_ascii_digit() => {
                    tokens.push(self.read_number()?);
                }
                c if c.is_alphabetic() || c == '_' => {
                    tokens.push(self.read_ident());
                }
                _ => {
                    self.advance();
                }
            }
        }

        Ok(tokens)
    }

    fn read_string(&mut self) -> anyhow::Result<Token> {
        self.advance(); // skip opening "
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => break,
                Some(ch) => s.push(ch),
                None => anyhow::bail!("unterminated string"),
            }
        }
        Ok(Token::StringLit(s))
    }

    fn read_number(&mut self) -> anyhow::Result<Token> {
        let mut s = String::new();
        let mut is_float = false;

        if self.peek() == Some('-') {
            s.push('-');
            self.advance();
        }

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else if ch == '.' && !is_float {
                is_float = true;
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if is_float {
            Ok(Token::FloatLit(s.parse()?))
        } else {
            Ok(Token::IntLit(s.parse()?))
        }
    }

    fn read_ident(&mut self) -> Token {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' || ch == ':' || ch == '.' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        // 型名に [] が付く場合 (float3[], int[] etc.)
        if self.peek() == Some('[') && self.chars.get(self.pos + 1) == Some(&']') {
            s.push('[');
            s.push(']');
            self.advance();
            self.advance();
        }

        if s == "def" {
            Token::Def
        } else {
            Token::Ident(s)
        }
    }
}

// ── パーサー ────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos)?;
        self.pos += 1;
        Some(tok)
    }

    fn expect(&mut self, expected: &Token) -> anyhow::Result<()> {
        let tok = self
            .advance()
            .ok_or_else(|| anyhow::anyhow!("unexpected EOF, expected {:?}", expected))?;
        if tok != expected {
            anyhow::bail!("expected {:?}, got {:?}", expected, tok);
        }
        Ok(())
    }

    fn parse_stage(&mut self) -> anyhow::Result<Stage> {
        let mut metadata = HashMap::new();
        let mut prims = Vec::new();

        // メタデータブロック ( ... ) がある場合
        if self.peek() == Some(&Token::ParenOpen) {
            metadata = self.parse_metadata_block()?;
        }

        // ルートレベルのプリミティブ
        while self.peek().is_some() {
            if self.peek() == Some(&Token::Def) {
                prims.push(self.parse_prim()?);
            } else {
                self.advance();
            }
        }

        Ok(Stage { metadata, prims })
    }

    fn parse_metadata_block(&mut self) -> anyhow::Result<HashMap<String, Value>> {
        self.expect(&Token::ParenOpen)?;
        let mut meta = HashMap::new();
        while self.peek() != Some(&Token::ParenClose) {
            if let Some(Token::Ident(name)) = self.peek().cloned() {
                self.advance();
                self.expect(&Token::Eq)?;
                let value = self.parse_value()?;
                meta.insert(name, value);
            } else {
                self.advance();
            }
        }
        self.expect(&Token::ParenClose)?;
        Ok(meta)
    }

    fn parse_prim(&mut self) -> anyhow::Result<Prim> {
        self.expect(&Token::Def)?;

        let prim_type = match self.advance() {
            Some(Token::Ident(s)) => s.clone(),
            other => anyhow::bail!("expected prim type, got {:?}", other),
        };

        let name = match self.advance() {
            Some(Token::StringLit(s)) => s.clone(),
            other => anyhow::bail!("expected prim name, got {:?}", other),
        };

        self.expect(&Token::BraceOpen)?;

        let mut properties = HashMap::new();
        let mut children = Vec::new();

        while self.peek() != Some(&Token::BraceClose) {
            match self.peek() {
                Some(Token::Def) => {
                    children.push(self.parse_prim()?);
                }
                Some(Token::Ident(_)) => {
                    let (prop_name, prop) = self.parse_property()?;
                    properties.insert(prop_name, prop);
                }
                None => anyhow::bail!("unexpected EOF in prim '{}'", name),
                _ => {
                    self.advance();
                }
            }
        }

        self.expect(&Token::BraceClose)?;

        Ok(Prim {
            prim_type,
            name,
            properties,
            children,
        })
    }

    fn parse_property(&mut self) -> anyhow::Result<(String, Property)> {
        // 型名を読む (float3[], double, token[] etc.)
        let type_name = match self.advance() {
            Some(Token::Ident(s)) => s.clone(),
            other => anyhow::bail!("expected type name, got {:?}", other),
        };

        // プロパティ名を読む
        let prop_name = match self.advance() {
            Some(Token::Ident(s)) => s.clone(),
            other => anyhow::bail!("expected property name, got {:?}", other),
        };

        self.expect(&Token::Eq)?;
        let value = self.parse_value()?;

        Ok((prop_name, Property { type_name, value }))
    }

    fn parse_value(&mut self) -> anyhow::Result<Value> {
        match self.peek() {
            Some(Token::IntLit(_)) => {
                let Token::IntLit(n) = self.advance().unwrap().clone() else {
                    unreachable!()
                };
                Ok(Value::Int(n))
            }
            Some(Token::FloatLit(_)) => {
                let Token::FloatLit(f) = self.advance().unwrap().clone() else {
                    unreachable!()
                };
                Ok(Value::Float(f))
            }
            Some(Token::StringLit(_)) => {
                let Token::StringLit(s) = self.advance().unwrap().clone() else {
                    unreachable!()
                };
                Ok(Value::String(s))
            }
            Some(Token::ParenOpen) => self.parse_tuple(),
            Some(Token::BracketOpen) => self.parse_array(),
            other => anyhow::bail!("unexpected token in value: {:?}", other),
        }
    }

    fn parse_tuple(&mut self) -> anyhow::Result<Value> {
        self.expect(&Token::ParenOpen)?;
        let mut items = Vec::new();
        while self.peek() != Some(&Token::ParenClose) {
            items.push(self.parse_value()?);
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            }
        }
        self.expect(&Token::ParenClose)?;
        Ok(Value::Tuple(items))
    }

    fn parse_array(&mut self) -> anyhow::Result<Value> {
        self.expect(&Token::BracketOpen)?;
        let mut items = Vec::new();
        while self.peek() != Some(&Token::BracketClose) {
            items.push(self.parse_value()?);
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            }
        }
        self.expect(&Token::BracketClose)?;
        Ok(Value::Array(items))
    }
}

// ── 公開 API ────────────────────────────────────

/// .usda テキストをパースして Stage を返す
pub fn parse(input: &str) -> anyhow::Result<Stage> {
    let mut tokenizer = Tokenizer::new(input);
    let tokens = tokenizer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse_stage()
}

/// .usda ファイルを読み込んでパース
pub fn load(path: &std::path::Path) -> anyhow::Result<Stage> {
    let content = std::fs::read_to_string(path)?;
    parse(&content)
}

// ── 表示用 ──────────────────────────────────────

impl Stage {
    /// シーングラフをテキストで表示
    pub fn dump(&self) {
        if !self.metadata.is_empty() {
            println!("Metadata:");
            for (k, v) in &self.metadata {
                println!("  {} = {:?}", k, v);
            }
        }
        for prim in &self.prims {
            prim.dump(0);
        }
    }
}

impl Prim {
    fn dump(&self, indent: usize) {
        let pad = "  ".repeat(indent);
        println!("{}def {} \"{}\"", pad, self.prim_type, self.name);
        for (name, prop) in &self.properties {
            println!("{}  {} {} = {:?}", pad, prop.type_name, name, prop.value);
        }
        for child in &self.children {
            child.dump(indent + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_stage() {
        let input = r#"#usda 1.0

def Xform "Root" {
    def Sphere "Ball" {
        double radius = 1.0
    }
}
"#;
        let stage = parse(input).unwrap();
        assert_eq!(stage.prims.len(), 1);

        let root = &stage.prims[0];
        assert_eq!(root.prim_type, "Xform");
        assert_eq!(root.name, "Root");
        assert_eq!(root.children.len(), 1);

        let ball = &root.children[0];
        assert_eq!(ball.prim_type, "Sphere");
        assert_eq!(ball.name, "Ball");
        assert_eq!(ball.properties["radius"].value.as_f64(), Some(1.0));
    }

    #[test]
    fn parse_metadata() {
        let input = r#"#usda 1.0
(
    defaultPrim = "Stage"
    upAxis = "Y"
)

def Xform "Stage" {
}
"#;
        let stage = parse(input).unwrap();
        assert_eq!(
            stage.metadata["defaultPrim"].as_str(),
            Some("Stage")
        );
        assert_eq!(stage.metadata["upAxis"].as_str(), Some("Y"));
    }

    #[test]
    fn parse_arrays_and_tuples() {
        let input = r#"#usda 1.0

def Mesh "Floor" {
    float3[] points = [(-5, 0, -5), (5, 0, -5)]
    int[] faceVertexCounts = [4]
    color3f[] primvars:displayColor = [(0.2, 0.2, 0.25)]
}
"#;
        let stage = parse(input).unwrap();
        let floor = &stage.prims[0];
        assert_eq!(floor.name, "Floor");

        let points = &floor.properties["points"];
        assert_eq!(points.type_name, "float3[]");
        if let Value::Array(arr) = &points.value {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0].as_f64x3(), Some([-5.0, 0.0, -5.0]));
        } else {
            panic!("expected array");
        }
    }
}

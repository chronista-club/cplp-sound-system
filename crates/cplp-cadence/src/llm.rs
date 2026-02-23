use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::midi_types::MidiSequence;

/// LLM プロバイダトレイト
///
/// 自然言語プロンプトから MidiSequence を生成する。
/// 現在は ClaudeProvider のみ実装。将来的に他の LLM を追加可能。
pub trait LlmProvider: Send + Sync {
    /// 自然言語プロンプトを MidiSequence に変換する
    async fn generate_sequence(&self, prompt: &str) -> anyhow::Result<MidiSequence>;
}

const SYSTEM_PROMPT: &str = r#"あなたは MIDI シーケンス生成エンジンです。
ユーザーの自然言語リクエスト（日本語または英語）を MidiSequence JSON に変換してください。

# 出力フォーマット

必ず以下の JSON スキーマで ```json ブロック内に出力してください:

```json
{
  "tempo_bpm": <テンポ(BPM)>,
  "events": [
    { "tick": <開始ティック>, "note": <MIDIノート番号>, "velocity": <ベロシティ>, "duration_ticks": <ノート長> }
  ]
}
```

# ティック解像度

- 4分音符 = 480 ticks
- 8分音符 = 240 ticks
- 16分音符 = 120 ticks
- 全音符 = 1920 ticks
- 2分音符 = 960 ticks
- 付点4分音符 = 720 ticks
- 3連符(4分音符) = 320 ticks

# MIDI ノート番号

- C4 = 60, D4 = 62, E4 = 64, F4 = 65, G4 = 67, A4 = 69, B4 = 71
- 1オクターブ = 12半音 (C5 = 72, C3 = 48)
- シャープ: +1 (C#4 = 61), フラット: -1 (Bb4 = 70)

# 音楽理論リファレンス

スケール（半音間隔）:
- メジャー: 2-2-1-2-2-2-1
- マイナー(ナチュラル): 2-1-2-2-1-2-2
- ペンタトニック(メジャー): 2-2-3-2-3
- ブルース: 3-2-1-1-3-2
- ドリアン: 2-1-2-2-2-1-2
- ミクソリディアン: 2-2-1-2-2-1-2

コード構成:
- メジャートライアド: ルート, +4, +7
- マイナートライアド: ルート, +3, +7
- セブンス: トライアド + +10(dom7) / +11(maj7)
- ディミニッシュ: ルート, +3, +6

# ガイドライン

- velocity は 60〜120 の範囲で、アクセントやダイナミクスを表現する
- シーケンスはループ再生される前提で作成する（最後のノートの終了後にシームレスにループ）
- テンポが指定されない場合はジャンルに適したBPMを選ぶ（ブルース: 80-100, ロック: 120-140, ジャズ: 100-160, ボサノバ: 130-150）
- コード進行が指定された場合、各コードを4分音符単位で区切り、アルペジオまたはブロックコードで配置する
- 「バッキング」と指示された場合はコードトーンを中心に伴奏パターンを生成する
- 日本語の音名表記（ド=C, レ=D, ミ=E, ファ=F, ソ=G, ラ=A, シ=B）にも対応する
- JSON 以外のテキスト（説明等）は最小限にする"#;

/// 認証方式
pub enum AuthMethod {
    /// x-api-key ヘッダー（ANTHROPIC_API_KEY）
    ApiKey(String),
    /// Authorization: Bearer（OAuth トークン）
    Bearer(String),
}

/// Anthropic Claude API 実装
pub struct ClaudeProvider {
    client: reqwest::Client,
    auth: AuthMethod,
    model: String,
}

impl ClaudeProvider {
    pub fn new(auth: AuthMethod) -> Self {
        Self {
            client: reqwest::Client::new(),
            auth,
            model: "claude-sonnet-4-20250514".to_string(),
        }
    }

    /// Claude API レスポンスのテキストから JSON ブロックを抽出
    fn extract_json(text: &str) -> anyhow::Result<MidiSequence> {
        // ```json ... ``` ブロックを探す
        if let Some(start) = text.find("```json") {
            let json_start = start + "```json".len();
            if let Some(end) = text[json_start..].find("```") {
                let json_str = text[json_start..json_start + end].trim();
                return serde_json::from_str(json_str).context("JSON ブロックのパースに失敗");
            }
        }

        // ``` ブロックがない場合、レスポンス全体を JSON としてパース
        // (LLM が JSON のみを返した場合)
        if let Ok(seq) = serde_json::from_str::<MidiSequence>(text.trim()) {
            return Ok(seq);
        }

        // { ... } を探す
        if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                let json_str = &text[start..=end];
                return serde_json::from_str(json_str).context("埋め込み JSON のパースに失敗");
            }
        }

        bail!("レスポンスから JSON を抽出できませんでした")
    }
}

// ── Anthropic Messages API リクエスト/レスポンス型 ────────────

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

// ── LlmProvider 実装 ─────────────────────────────────────────

impl LlmProvider for ClaudeProvider {
    async fn generate_sequence(&self, prompt: &str) -> anyhow::Result<MidiSequence> {
        debug!("LLM リクエスト: {}", prompt);

        let request = MessagesRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            system: SYSTEM_PROMPT.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");

        req = match &self.auth {
            AuthMethod::ApiKey(key) => req.header("x-api-key", key),
            AuthMethod::Bearer(token) => req.header("authorization", format!("Bearer {token}")),
        };

        let response = req
            .json(&request)
            .send()
            .await
            .context("Claude API へのリクエストに失敗")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("Claude API エラー ({}): {}", status, body);
            bail!("Claude API エラー: {} - {}", status, body);
        }

        let api_response: MessagesResponse = response
            .json()
            .await
            .context("Claude API レスポンスのパースに失敗")?;

        let text = api_response
            .content
            .iter()
            .find(|b| b.content_type == "text")
            .and_then(|b| b.text.as_deref())
            .context("Claude API レスポンスにテキストが含まれていません")?;

        debug!("LLM レスポンス: {}", text);

        let sequence =
            Self::extract_json(text).context("LLM レスポンスから MidiSequence の抽出に失敗")?;

        // バリデーション
        if sequence.tempo_bpm <= 0.0 || sequence.tempo_bpm > 300.0 {
            bail!(
                "無効なテンポ: {} BPM (1-300 の範囲で指定してください)",
                sequence.tempo_bpm
            );
        }
        for event in &sequence.events {
            if event.note > 127 {
                bail!("無効なノート番号: {} (0-127)", event.note);
            }
            if event.velocity > 127 {
                bail!("無効なベロシティ: {} (0-127)", event.velocity);
            }
        }

        Ok(sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_from_code_block() {
        let text = r#"Here's a blues sequence:
```json
{"tempo_bpm": 120.0, "events": [{"tick": 0, "note": 60, "velocity": 100, "duration_ticks": 480}]}
```
This plays a C note."#;

        let seq = ClaudeProvider::extract_json(text).unwrap();
        assert!((seq.tempo_bpm - 120.0).abs() < f32::EPSILON);
        assert_eq!(seq.events.len(), 1);
        assert_eq!(seq.events[0].note, 60);
    }

    #[test]
    fn extract_json_raw() {
        let text = r#"{"tempo_bpm": 90.0, "events": [{"tick": 0, "note": 69, "velocity": 80, "duration_ticks": 240}]}"#;

        let seq = ClaudeProvider::extract_json(text).unwrap();
        assert!((seq.tempo_bpm - 90.0).abs() < f32::EPSILON);
        assert_eq!(seq.events[0].note, 69);
    }

    #[test]
    fn extract_json_embedded() {
        let text = r#"Sure! Here's the sequence: {"tempo_bpm": 100.0, "events": [{"tick": 0, "note": 62, "velocity": 90, "duration_ticks": 960}]} Let me know if you want changes."#;

        let seq = ClaudeProvider::extract_json(text).unwrap();
        assert!((seq.tempo_bpm - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn extract_json_no_json() {
        let text = "I can't generate music right now.";
        assert!(ClaudeProvider::extract_json(text).is_err());
    }

    #[test]
    fn extract_json_multiple_events() {
        let text = r#"```json
{
  "tempo_bpm": 120.0,
  "events": [
    {"tick": 0, "note": 60, "velocity": 100, "duration_ticks": 480},
    {"tick": 480, "note": 64, "velocity": 95, "duration_ticks": 480},
    {"tick": 960, "note": 67, "velocity": 90, "duration_ticks": 480},
    {"tick": 1440, "note": 72, "velocity": 100, "duration_ticks": 960}
  ]
}
```"#;

        let seq = ClaudeProvider::extract_json(text).unwrap();
        assert_eq!(seq.events.len(), 4);
        assert_eq!(seq.events[0].note, 60); // C4
        assert_eq!(seq.events[1].note, 64); // E4
        assert_eq!(seq.events[2].note, 67); // G4
        assert_eq!(seq.events[3].note, 72); // C5
    }
}

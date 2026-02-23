/// ネイティブ Rust モジュールの共通インターフェース
///
/// CLAP プラグインと同列に AudioEngine で扱えるようにする。
/// process() でオーディオ生成、handle_midi() で MIDI 受信。
pub trait AudioModule: Send {
    /// オーディオ生成（シンセ・ドラムマシン等）
    /// output バッファにサンプルを書き込む
    fn process(&mut self, output: &mut [f32]);

    /// エフェクト処理（入力→出力）
    /// デフォルト実装: input をそのまま output にコピー（パススルー）
    fn process_replacing(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());
        output[..len].copy_from_slice(&input[..len]);
    }

    /// MIDI イベント受信
    fn handle_midi(&mut self, event: MidiEvent);

    /// パラメータ設定
    fn set_param(&mut self, id: u32, value: f32);

    /// モジュール情報
    fn info(&self) -> ModuleInfo;
}

/// MIDI イベント（ノートオン/オフ + CC）
#[derive(Debug, Clone)]
pub enum MidiEvent {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    ControlChange { cc: u8, value: u8 },
}

/// モジュールのメタ情報
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub category: ModuleCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleCategory {
    Instrument,
    Effect,
    Utility,
}

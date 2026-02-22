use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer, Viewport,
};

/// テキスト描画リクエスト
pub struct TextEntry {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: [f32; 4], // RGBA (0.0–1.0)
}

/// glyphon ベースのテキスト描画エンジン
pub struct TextEngine {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: GlyphonTextRenderer,
    buffers: Vec<Buffer>,
}

impl TextEngine {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer = GlyphonTextRenderer::new(
            &mut atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );

        Self {
            font_system,
            swash_cache,
            viewport,
            atlas,
            renderer,
            buffers: Vec::new(),
        }
    }

    /// TextEntry 群からテキスト描画を準備する
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        entries: &[TextEntry],
        width: u32,
        height: u32,
    ) {
        self.viewport.update(queue, Resolution { width, height });

        // エントリごとに Buffer を作成
        self.buffers.clear();
        for entry in entries {
            let line_height = entry.size * 1.4;
            let mut buffer =
                Buffer::new(&mut self.font_system, Metrics::new(entry.size, line_height));
            buffer.set_size(
                &mut self.font_system,
                Some(width as f32),
                Some(height as f32),
            );
            buffer.set_text(
                &mut self.font_system,
                &entry.text,
                &Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(&mut self.font_system, false);
            self.buffers.push(buffer);
        }

        // TextArea を組み立てて prepare
        let text_areas: Vec<TextArea> = entries
            .iter()
            .zip(self.buffers.iter())
            .map(|(entry, buffer)| {
                let [r, g, b, a] = entry.color;
                TextArea {
                    buffer,
                    left: entry.x,
                    top: entry.y,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: width as i32,
                        bottom: height as i32,
                    },
                    default_color: GlyphonColor::rgba(
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (b * 255.0) as u8,
                        (a * 255.0) as u8,
                    ),
                    custom_glyphs: &[],
                }
            })
            .collect();

        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .expect("failed to prepare text rendering");
    }

    /// レンダーパスにテキストを描画する
    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        self.renderer
            .render(&self.atlas, &self.viewport, pass)
            .expect("failed to render text");
    }

    /// 未使用のアトラスエントリを解放する
    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}

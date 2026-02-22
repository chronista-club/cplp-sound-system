// ガウシアンブラーシェーダー
// フルスクリーン三角形 + 2パス（水平/垂直）ブラー

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// フルスクリーン三角形（頂点バッファ不要）
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var out: VertexOutput;
    // 大きな三角形でスクリーン全体をカバー
    let x = f32(i32(idx) / 2) * 4.0 - 1.0;
    let y = f32(i32(idx) % 2) * 4.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) / 2.0, (1.0 - y) / 2.0);
    return out;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;
@group(0) @binding(2) var<uniform> direction: vec2<f32>;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let weights = array<f32, 5>(0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);
    var result = textureSample(input_texture, tex_sampler, in.uv) * weights[0];
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    for (var i = 1; i < 5; i++) {
        let offset = direction * f32(i) / tex_size;
        result += textureSample(input_texture, tex_sampler, in.uv + offset) * weights[i];
        result += textureSample(input_texture, tex_sampler, in.uv - offset) * weights[i];
    }
    return result;
}

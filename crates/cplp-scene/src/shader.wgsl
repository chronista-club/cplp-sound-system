// Gig Scene 3D シェーダー — Phong ライティング

struct Camera {
    view_proj: mat4x4<f32>,
    eye_pos: vec4<f32>,
}

struct Model {
    transform: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
}

struct Light {
    direction: vec4<f32>,       // xyz = 方向（正規化）, w = ambient 強度
    color: vec4<f32>,           // xyz = ライト色, w = specular 強度
    params: vec4<f32>,          // x = shininess, y = rim 強度, z = unused, w = unused
}

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var<uniform> model: Model;

@group(2) @binding(0)
var<uniform> light: Light;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) normal: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) world_pos: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model.transform * vec4<f32>(in.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    out.color = in.color;
    out.world_pos = world_pos.xyz;
    // normal_matrix で法線を変換（非一様スケール対応）
    out.world_normal = normalize((model.normal_matrix * vec4<f32>(in.normal, 0.0)).xyz);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let N = normalize(in.world_normal);
    let L = normalize(-light.direction.xyz);
    let V = normalize(camera.eye_pos.xyz - in.world_pos);
    let H = normalize(L + V);

    // Ambient
    let ambient_strength = light.direction.w;
    let ambient = ambient_strength * in.color;

    // Diffuse (half-Lambert for softer look)
    let NdotL = dot(N, L);
    let half_lambert = NdotL * 0.5 + 0.5;
    let diffuse = half_lambert * in.color * light.color.xyz;

    // Specular (Blinn-Phong)
    let shininess = light.params.x;
    let spec_strength = light.color.w;
    let NdotH = max(dot(N, H), 0.0);
    let specular = spec_strength * pow(NdotH, shininess) * light.color.xyz;

    // Rim light（エッジを明るくして立体感を出す）
    let rim_strength = light.params.y;
    let rim = rim_strength * pow(1.0 - max(dot(N, V), 0.0), 3.0);
    let rim_color = rim * vec3<f32>(0.3, 0.4, 0.6);

    let result = ambient + diffuse + specular + rim_color;

    // トーンマッピング（簡易 Reinhard）
    let mapped = result / (result + vec3<f32>(1.0, 1.0, 1.0));

    return vec4<f32>(mapped, 1.0);
}

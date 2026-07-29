#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_clip}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var atlas_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> effects: vec4<f32>;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) effect: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) effect: vec4<f32>,
};

fn flicker(time: f32, phase: f32) -> f32 {
    let primary = sin(time * 7.3 + phase);
    let detail = sin(time * 13.1 + phase * 1.7);
    return clamp(1.0 + primary * 0.08 + detail * 0.04, 0.88, 1.12);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var position = vertex.position;
    let kind = vertex.effect.x;
    let phase = vertex.effect.y;
    if kind > 0.5 && kind < 1.5 {
        position.x += sin(effects.x * 7.3 + phase) * 0.025;
    } else if kind >= 1.5 {
        let progress = fract(effects.x * 0.7 + phase / 6.2831853);
        position.y += progress * 0.25;
        position.x += sin(effects.x * 4.7 + phase) * 0.018 * vertex.effect.z;
    }

    var out: VertexOutput;
    out.clip_position = mesh_position_local_to_clip(
        get_world_from_local(vertex.instance_index),
        vec4(position, 1.0),
    );
    out.uv = vertex.uv;
    out.effect = vertex.effect;
    return out;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let kind = input.effect.x;
    let intensity = input.effect.w * flicker(effects.x, input.effect.y);
    if kind < 0.5 {
        let atlas = textureSample(atlas_texture, atlas_sampler, input.uv);
        if atlas.a < 0.18 {
            discard;
        }
        return vec4(atlas.rgb * 1.45, atlas.a);
    }

    let centered_x = abs(input.uv.x - 0.5);
    if kind < 1.5 {
        let taper = mix(0.10, 0.46, input.uv.y);
        if centered_x > taper {
            discard;
        }
        let core = centered_x < taper * 0.48 && input.uv.y > 0.28;
        let color = select(vec3(1.0, 0.20, 0.025), vec3(1.0, 0.78, 0.18), core);
        return vec4(color * (3.8 * intensity), 1.0);
    }

    if max(centered_x, abs(input.uv.y - 0.5)) > 0.42 {
        discard;
    }
    return vec4(vec3(1.0, 0.42, 0.05) * (3.0 * intensity), 1.0);
}

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

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = mesh_position_local_to_clip(
        get_world_from_local(vertex.instance_index),
        vec4(vertex.position, 1.0),
    );
    out.uv = vertex.uv;
    out.effect = vertex.effect;
    return out;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let atlas = textureSample(atlas_texture, atlas_sampler, input.uv);
    let emissive = input.effect.x >= 0.75;
    let color = select(
        atlas.rgb,
        atlas.rgb * vec3(3.0, 1.4, 0.35) * effects.x,
        emissive,
    );
    return vec4(color, atlas.a);
}

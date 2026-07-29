#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var atlas_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var lightmap_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<uniform> haze_color: vec4<f32>;

fn depth_style(depth: u32) -> vec2<f32> {
    switch depth {
        case 1u: {
            return vec2(0.92, 0.12);
        }
        case 2u: {
            return vec2(0.84, 0.25);
        }
        case 3u: {
            return vec2(0.76, 0.38);
        }
        default: {
            return vec2(1.0, 0.0);
        }
    }
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let atlas = textureSample(atlas_texture, atlas_sampler, mesh.uv);
    if atlas.a < 0.38 {
        discard;
    }

    let sky = clamp(mesh.color.r * 15.0, 0.0, 15.0);
    let block = clamp(mesh.color.g * 15.0, 0.0, 15.0);
    let light_uv = (vec2(block, sky) + vec2(0.5)) / 16.0;
    let light = textureSample(lightmap_texture, lightmap_sampler, light_uv).rgb;
    let style = depth_style(u32(round(mesh.color.a * 3.0)));
    let lit = atlas.rgb * light * mesh.color.b;
    let hazed = mix(lit, haze_color.rgb, style.y) * style.x;
    return vec4(hazed, atlas.a);
}

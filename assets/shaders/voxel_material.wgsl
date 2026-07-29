#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var atlas_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var lightmap_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<uniform> haze_color: vec4<f32>;

fn depth_haze(depth: u32) -> f32 {
    switch depth {
        case 1u: {
            return 0.12;
        }
        case 2u: {
            return 0.25;
        }
        default: {
            return select(0.0, 0.38, depth >= 3u);
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
    let haze = depth_haze(u32(round(mesh.color.a)));
    let light_peak = max(max(light.r, light.g), light.b);
    let haze_peak = max(max(haze_color.r, haze_color.g), max(haze_color.b, 0.001));
    let haze_light = haze_color.rgb * (light_peak / haze_peak);
    let detailed_light = mix(light, haze_light, haze);
    return vec4(atlas.rgb * detailed_light * mesh.color.b, atlas.a);
}

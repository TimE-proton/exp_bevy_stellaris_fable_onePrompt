// Stadtlichter einer Welt in der Orbitansicht.
//
// Gezeichnet wird eine Hülle knapp über der Oberfläche, additiv. Die Lichter leuchten
// nur, wo die Sonne nicht hinkommt; an der Tag-Nacht-Grenze gehen sie langsam an.

import bevy_pbr::render::forward_io::VertexOutput;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var lights: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var lights_sampler: sampler;
// xyz: Richtung zur Sonne, w: Helligkeit.
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> sun: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(in.world_normal);
    let light = dot(normal, normalize(sun.xyz));
    let night = 1.0 - smoothstep(-0.12, 0.08, light);
    let towns = textureSample(lights, lights_sampler, in.uv).rgb;
    return vec4<f32>(towns * sun.w * night, 0.0);
}

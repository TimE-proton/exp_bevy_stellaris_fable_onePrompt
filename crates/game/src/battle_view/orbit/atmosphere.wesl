// Saum der Atmosphäre einer Welt in der Orbitansicht.
//
// Gezeichnet wird die Vorderseite einer Hülle etwas größer als der Planet, additiv. Für
// jeden Bildpunkt rechnet der Shader aus, wie nah der Sehstrahl dem Mittelpunkt kommt:
// Neben der Scheibe fällt der Dunst mit der Höhe ab, über der Scheibe verdichtet er sich
// zum Rand hin. Die Nachtseite bleibt dunkel; an der Tag-Nacht-Grenze färbt er sich warm.

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

// rgb: Farbe (linear), a: Stärke.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> haze: vec4<f32>;
// xyz: Richtung zur Sonne.
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> sun: vec4<f32>;
// xyz: Mittelpunkt, w: Halbmesser des Planeten.
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> body: vec4<f32>;
// x: Halbmesser der Hülle, y: Höhe, auf die der Dunst abfällt.
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> shape: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let eye = view.world_position;
    let ray = normalize(in.world_position.xyz - eye);
    let center = body.xyz;
    let radius = body.w;
    let along = dot(center - eye, ray);
    let offset = eye + ray * along - center;
    let distance = length(offset);
    let scale = shape.y;
    var glow = 0.0;
    var normal = offset / max(distance, 0.0001);
    if distance < radius {
        // Der Strahl trifft die Welt: Dunst über der Oberfläche, am Rand dicht.
        let back = sqrt(radius * radius - distance * distance);
        normal = normalize(eye + ray * (along - back) - center);
        let facing = clamp(dot(normal, -ray), 0.0, 1.0);
        let rim = 1.0 - facing;
        glow = 0.05 + 0.95 * rim * rim * rim * rim;
    } else {
        let above = (distance - radius) / scale;
        let fade = 1.0 - smoothstep(shape.x - scale * 1.5, shape.x, distance);
        glow = exp(-above) * fade;
    }
    let light = dot(normal, normalize(sun.xyz));
    let day = smoothstep(-0.28, 0.4, light);
    // Warm an der Grenze zur Nacht, wo das Licht den längsten Weg durch die Luft nimmt.
    let dusk = smoothstep(-0.3, 0.0, light) * (1.0 - smoothstep(0.0, 0.35, light));
    let color = mix(haze.rgb, vec3<f32>(1.0, 0.5, 0.25), dusk * 0.55);
    return vec4<f32>(color * haze.a * glow * day, 0.0);
}

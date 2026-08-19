// One pipeline for every shape and every glyph. Solid instances take the rounded-box SDF path,
// textured instances sample the glyph atlas. Output is premultiplied — the layer surface is
// composited with PreMultiplied alpha.

struct Globals {
    screen: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_tex: texture_2d<f32>;
@group(0) @binding(2) var atlas_smp: sampler;

struct Inst {
    @location(0) xywh: vec4<f32>,   // centre.xy, half-extent.xy
    @location(1) color: vec4<f32>,
    @location(2) uv: vec4<f32>,     // u0 v0 u1 v1; u0 < 0 means solid
    @location(3) misc: vec4<f32>,   // rot, radius, pad, pad
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_extent: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) radius: f32,
    @location(5) textured: f32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
    let sx = f32(vi & 1u) * 2.0 - 1.0;
    let sy = f32(vi >> 1u) * 2.0 - 1.0;

    // One pixel of slack so the SDF has room to antialias inside the quad.
    let half_extent = inst.xywh.zw;
    let padded = half_extent + vec2<f32>(1.0, 1.0);
    let local = vec2<f32>(sx, sy) * padded;

    let rot = inst.misc.x;
    let c = cos(rot);
    let s = sin(rot);
    let rotated = vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    let screen_pos = inst.xywh.xy + rotated;

    var out: VOut;
    out.clip = vec4<f32>(
        screen_pos.x / globals.screen.x * 2.0 - 1.0,
        1.0 - screen_pos.y / globals.screen.y * 2.0,
        0.0,
        1.0,
    );
    out.local = local;
    out.half_extent = half_extent;
    out.color = inst.color;
    out.uv = mix(inst.uv.xy, inst.uv.zw, vec2<f32>(f32(vi & 1u), f32(vi >> 1u)));
    out.radius = inst.misc.y;
    out.textured = select(0.0, 1.0, inst.uv.x >= 0.0);
    return out;
}

fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    var a = in.color.a;
    if in.textured > 0.5 {
        a = a * textureSample(atlas_tex, atlas_smp, in.uv).r;
    } else {
        let r = min(in.radius, min(in.half_extent.x, in.half_extent.y));
        let d = sd_round_box(in.local, in.half_extent, r);
        a = a * clamp(0.5 - d, 0.0, 1.0);
    }
    return vec4<f32>(in.color.rgb * a, a);
}

// STR effect layers: the sprite vertex path (screen-aligned quads around an anchor), with
// per-quad blending: colour alpha < 0 marks an additive layer (rendered with alpha 0, so
// premultiplied blending adds it), otherwise normal alpha blending.

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    // x: seconds, y/z: clip units per sprite pixel (x, y) at w = 1, w: 1 to snap to pixels
    params: vec4<f32>,
    light_vp: mat4x4<f32>,
    shadow: vec4<f32>,
    // w, h, 1/w, 1/h
    screen: vec4<f32>,
    // x: sprite lighting, y: rim, z: point light intensity, w: light count
    sprite_light: vec4<f32>,
    lights: array<vec4<f32>, 64>,
    // x: 1 when indoors (room mask active), y/z: mask size in cells, w: world units per cell
    room: vec4<f32>,
};

/// World units (about two cells) a sprite's depth is pulled toward the camera.
const DEPTH_PULL: f32 = 10.0;

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(3) var room_mask: texture_2d<f32>;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsIn {
    @location(0) anchor: vec3<f32>,
    @location(1) offset: vec2<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) anchor: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var clip = g.view_proj * vec4(v.anchor, 1.0);
    clip.x += v.offset.x * g.params.y;
    clip.y -= v.offset.y * g.params.z;
    // Depth comes from a point pulled toward the camera, so the shadow and feet are not
    // hidden by the ground just in front of where the actor stands.
    let toward_eye = normalize(g.eye.xyz - v.anchor) * DEPTH_PULL;
    let d = g.view_proj * vec4(v.anchor + toward_eye, 1.0);
    clip.z = d.z / d.w * clip.w;
    var o: VsOut;
    o.pos = clip;
    o.uv = v.uv;
    o.color = v.color;
    o.anchor = v.anchor;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if !in_room(in.anchor) {
        discard;
    }
    let s = textureSample(tex, samp, in.uv);
    let additive = in.color.a < 0.0;
    let a = abs(in.color.a);
    let rgb = srgb_to_linear(s.rgb) * in.color.rgb * a;
    if additive {
        // Black texels add nothing; colour is weighted by the texel's own alpha too.
        return vec4(rgb * s.a, 0.0);
    }
    if s.a * a < 0.02 {
        discard;
    }
    return vec4(rgb * s.a, s.a * a);
}
/// Indoors, only the room the player is in is drawn (other rooms share the map).
fn in_room(world: vec3<f32>) -> bool {
    if g.room.x < 0.5 {
        return true;
    }
    let cell = vec2<i32>(floor(world.xz / g.room.w));
    if cell.x < 0 || cell.y < 0 || cell.x >= i32(g.room.y) || cell.y >= i32(g.room.z) {
        return false;
    }
    return textureLoad(room_mask, cell, 0).r > 0.5;
}

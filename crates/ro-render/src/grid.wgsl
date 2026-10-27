// Ground grid on y = 0 in RO cell units, anti-aliased with screen-space derivatives.
// Stands in for GND terrain until map data is loaded.

struct Globals {
    view_proj: mat4x4<f32>,
    // xz: highlighted cell center, y: cell size, w: fade radius
    focus: vec4<f32>,
    eye: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // Two triangles covering a square around the focus point.
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    let c = corners[i] * g.focus.w;
    let world = vec3(g.focus.x + c.x, 0.0, g.focus.z + c.y);
    var o: VsOut;
    o.pos = g.view_proj * vec4(world, 1.0);
    o.world = world;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

fn grid_line(coord: vec2<f32>) -> f32 {
    let d = fwidth(coord);
    let g = abs(fract(coord - 0.5) - 0.5) / d;
    return 1.0 - min(min(g.x, g.y), 1.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let cell = g.focus.y;
    let uv = in.world.xz / cell;

    var color = vec3(0.17, 0.22, 0.16);                        // grass-ish base
    color = mix(color, vec3(0.30, 0.38, 0.28), grid_line(uv) * 0.6);         // cells
    color = mix(color, vec3(0.55, 0.62, 0.50), grid_line(uv / 10.0) * 0.8);  // every 10 cells

    // Highlight the focus cell (player spawn).
    let focus_cell = floor(g.focus.xz / cell);
    if all(floor(uv) == focus_cell) {
        color = mix(color, vec3(0.95, 0.75, 0.30), 0.55);
    }

    // Fade to the clear color toward the edge of the plane.
    let r = length(in.world.xz - g.focus.xz) / g.focus.w;
    let fade = smoothstep(0.55, 1.0, r);
    // Colors above are authored in sRGB; the target is an sRGB surface, so output linear.
    return vec4(srgb_to_linear(mix(color, vec3(0.05, 0.06, 0.09), fade)), 1.0);
}

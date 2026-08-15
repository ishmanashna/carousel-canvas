//! GPU polaroid instant-film material (port of `polaroid_card.py` + composer shadows).

use core::{
    paper_base_rgb, polaroid_corner_radius, polaroid_inner_dims, polaroid_margins, SceneCard,
};

const POLAROID_SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    card_size: vec2<f32>,
    inner_origin: vec2<f32>,
    inner_size: vec2<f32>,
    paper_rgb: vec3<f32>,
    corner_radius: f32,
    slot_seed: u32,
    mode: u32,
    shadow_offset: vec2<f32>,
    _pad: vec2<f32>,
}
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var photo_tex: texture_2d<f32>;
@group(1) @binding(1) var photo_sampler: sampler;

struct VertexInput { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, }
struct VertexOutput { @builtin(position) clip_position: vec4<f32>, @location(0) uv: vec2<f32>, }

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = u.view_proj * vec4<f32>(vertex.pos, 0.0, 1.0);
    out.uv = vertex.uv;
    return out;
}

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let a = hash21(i);
    let b = hash21(i + vec2(1.0, 0.0));
    let c = hash21(i + vec2(0.0, 1.0));
    let d = hash21(i + vec2(1.0, 1.0));
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var pp = p;
    for (var i = 0; i < 4; i++) {
        v += a * noise2(pp);
        pp = pp * 2.03 + vec2(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}

fn rounded_rect_alpha(p: vec2<f32>, size: vec2<f32>, r: f32) -> f32 {
    let rr = min(r, min(size.x, size.y) * 0.5);
    let q = abs(p - size * 0.5) - (size * 0.5 - vec2(rr));
    let d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - rr;
    return 1.0 - smoothstep(-1.0, 1.0, d);
}

fn paper_grain(px: vec2<f32>, seed: f32) -> f32 {
    let s = seed * 0.013;
    let g1 = fbm(px * 0.004 + vec2(s, s * 1.3));
    let g2 = fbm(px * 0.018 + vec2(s * 2.1, s * 0.7));
    let g3 = noise2(px * 0.09 + vec2(s * 5.3, s * 3.1));
    return g1 * 0.20 + g2 * 0.17 + g3 * 0.12;
}

fn border_tone(px: vec2<f32>, size: vec2<f32>, border_mask: f32, seed: f32) -> vec3<f32> {
    let gx = px.x / max(1.0, size.x);
    let gy = px.y / max(1.0, size.y);
    let comb = (gx + gy) * 0.5;
    let warm = vec3(0.99, 0.93, 0.89);
    let cool = vec3(0.93, 0.94, 0.97);
    let vig = smoothstep(0.0, 0.22, min(min(gx, 1.0 - gx), min(gy, 1.0 - gy)));
    var tint = mix(warm, cool, comb) * (0.22 + hash21(vec2(seed, 1.7)) * 0.10);
    tint = mix(vec3(0.82, 0.78, 0.74), tint, 1.0 - vig);
    return tint * border_mask;
}

fn inner_feather_alpha(local: vec2<f32>, inner_origin: vec2<f32>, inner_size: vec2<f32>, feather: f32) -> f32 {
    let rel = local - inner_origin;
    let edge_x = min(rel.x, inner_size.x - rel.x);
    let edge_y = min(rel.y, inner_size.y - rel.y);
    let edge = min(edge_x, edge_y);
    return smoothstep(0.0, feather, edge);
}

fn photo_finish(col: vec3<f32>, px: vec2<f32>, seed: f32) -> vec3<f32> {
    let warm = vec3(1.0, 0.969, 0.918);
    var out = mix(col, warm, 0.038 + hash21(vec2(seed, 4.2)) * 0.018);
    let n = noise2(px * 0.05 + vec2(seed * 0.3, seed * 0.11));
    let grain = vec3(n);
    out = mix(out, grain, 0.022 + hash21(vec2(seed, 8.8)) * 0.012);
    return out;
}

fn polaroid_card_rgba(local: vec2<f32>, size: vec2<f32>, flip_y: bool) -> vec4<f32> {
    let seed = f32(u.slot_seed);
    let paper = u.paper_rgb;
    let inner_origin = u.inner_origin;
    let inner_size = u.inner_size;
    let feather = 2.5;

    let in_inner = local.x >= inner_origin.x && local.x <= inner_origin.x + inner_size.x
        && local.y >= inner_origin.y && local.y <= inner_origin.y + inner_size.y;

    var rgb = paper;
    let grain = paper_grain(local, seed + 11.0);
    rgb = mix(rgb, vec3(grain), 0.42);

    let border_mask = select(1.0, 0.0, in_inner);
    rgb += border_tone(local, size, border_mask, seed);

    if (in_inner) {
        var rel = local - inner_origin;
        if (flip_y) {
            rel.y = inner_size.y - rel.y;
        }
        let uv = (rel + vec2(0.5)) / inner_size;
        var photo = textureSample(photo_tex, photo_sampler, uv).rgb;
        photo = photo_finish(photo, local, seed);
        let alpha = inner_feather_alpha(local, inner_origin, inner_size, feather);
        rgb = mix(rgb, photo, alpha);
    }

    if (in_inner) {
        let rel = local - inner_origin;
        let edge_x = min(rel.x, inner_size.x - rel.x);
        let edge_y = min(rel.y, inner_size.y - rel.y);
        let edge = min(edge_x, edge_y);
        let inset = smoothstep(3.5, 0.0, edge);
        rgb = mix(rgb, vec3(0.07, 0.055, 0.047), inset * 0.38);
    }

    let gx = local.x / max(1.0, size.x);
    let gy = local.y / max(1.0, size.y);
    let top_left = (1.0 - gy) * (1.0 - gx);
    let bot_right = gy * gx;
    rgb += vec3(0.04) * top_left;
    rgb -= vec3(0.03) * bot_right;

    let alpha = rounded_rect_alpha(local, size, u.corner_radius);
    return vec4<f32>(clamp(rgb, vec3(0.0), vec3(1.0)), alpha);
}

fn shadow_rgba(local: vec2<f32>, size: vec2<f32>) -> vec4<f32> {
    let shifted = local - u.shadow_offset;
    let alpha = rounded_rect_alpha(shifted, size, u.corner_radius);
    let a = alpha * 0.43;
    return vec4<f32>(24.0 / 255.0, 20.0 / 255.0, 14.0 / 255.0, a);
}

fn reflection_rgba(local: vec2<f32>, size: vec2<f32>) -> vec4<f32> {
    let card_h = u.card_size.y;
    let src_y = card_h - local.y;
    if (src_y < 0.0 || src_y > card_h) {
        return vec4<f32>(0.0);
    }
    let src_local = vec2(local.x, src_y);
    var col = polaroid_card_rgba(src_local, u.card_size, true);
    let t = local.y / max(1.0, size.y);
    let fade = 0.08 * (1.0 - t / 0.15);
    col.a *= max(0.0, fade);
    return col;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = u.card_size;
    let local = in.uv * size;
    if (u.mode == 1u) {
        return shadow_rgba(local, size);
    }
    if (u.mode == 2u) {
        return reflection_rgba(local, size);
    }
    return polaroid_card_rgba(local, size, false);
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PolaroidUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub card_size: [f32; 2],
    pub inner_origin: [f32; 2],
    pub inner_size: [f32; 2],
    pub _align_paper: [f32; 2],
    pub paper_rgb: [f32; 3],
    pub corner_radius: f32,
    pub slot_seed: u32,
    pub mode: u32,
    pub shadow_offset: [f32; 2],
    pub _pad: [f32; 2],
    pub _struct_pad: [f32; 2],
}

const _: () = assert!(std::mem::size_of::<PolaroidUniforms>() == 144);

unsafe impl bytemuck::Pod for PolaroidUniforms {}
unsafe impl bytemuck::Zeroable for PolaroidUniforms {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PolaroidDrawMode {
    Card = 0,
    Shadow = 1,
    Reflection = 2,
}

pub(crate) fn polaroid_shader_source() -> &'static str {
    POLAROID_SHADER
}

pub(crate) fn polaroid_uniforms(
    view_proj: [[f32; 4]; 4],
    card: &SceneCard,
    mode: PolaroidDrawMode,
) -> PolaroidUniforms {
    let sw = card.dest.w.max(1);
    let sh = card.dest.h.max(1);
    let (m_side, m_top, _m_bot) = polaroid_margins(sw, sh);
    let (iw, ih) = polaroid_inner_dims(sw, sh);
    let paper = paper_base_rgb(card.slot_seed.wrapping_add(11));
    PolaroidUniforms {
        view_proj,
        card_size: [sw as f32, sh as f32],
        inner_origin: [m_side as f32, m_top as f32],
        inner_size: [iw as f32, ih as f32],
        _align_paper: [0.0, 0.0],
        paper_rgb: [
            paper[0] as f32 / 255.0,
            paper[1] as f32 / 255.0,
            paper[2] as f32 / 255.0,
        ],
        corner_radius: polaroid_corner_radius(sw, sh) as f32,
        slot_seed: card.slot_seed,
        mode: mode as u32,
        shadow_offset: [5.0, 8.0],
        _pad: [0.0, 0.0],
        _struct_pad: [0.0, 0.0],
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
}

unsafe impl bytemuck::Pod for Vertex {}
unsafe impl bytemuck::Zeroable for Vertex {}

fn rotate_point(x: f32, y: f32, cx: f32, cy: f32, deg: f32) -> [f32; 2] {
    let rad = deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    let dx = x - cx;
    let dy = y - cy;
    [cx + dx * cos - dy * sin, cy + dx * sin + dy * cos]
}

pub(crate) fn polaroid_vertices(card: &SceneCard, mode: PolaroidDrawMode) -> [Vertex; 6] {
    let x = card.dest.x as f32;
    let y = card.dest.y as f32;
    let w = card.dest.w.max(1) as f32;
    let h = card.dest.h.max(1) as f32;
    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    let rot = card.rotation_deg as f32;

    let (qx, qy, qw, qh) = match mode {
        PolaroidDrawMode::Card | PolaroidDrawMode::Shadow => (x, y, w, h),
        PolaroidDrawMode::Reflection => {
            let rh = (h * 0.32).max(8.0);
            (x, y + h - 2.0, w, rh)
        }
    };

    let corners = [
        rotate_point(qx, qy, cx, cy, rot),
        rotate_point(qx + qw, qy, cx, cy, rot),
        rotate_point(qx + qw, qy + qh, cx, cy, rot),
        rotate_point(qx, qy + qh, cx, cy, rot),
    ];

    let uv_scale = match mode {
        PolaroidDrawMode::Reflection => [w, h],
        _ => [qw, qh],
    };

    [
        Vertex {
            pos: corners[0],
            uv: [0.0, 0.0],
        },
        Vertex {
            pos: corners[1],
            uv: [uv_scale[0] / w, 0.0],
        },
        Vertex {
            pos: corners[2],
            uv: [uv_scale[0] / w, uv_scale[1] / h],
        },
        Vertex {
            pos: corners[0],
            uv: [0.0, 0.0],
        },
        Vertex {
            pos: corners[2],
            uv: [uv_scale[0] / w, uv_scale[1] / h],
        },
        Vertex {
            pos: corners[3],
            uv: [0.0, uv_scale[1] / h],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::Rect;
    use std::path::PathBuf;

    #[test]
    fn reflection_quad_sits_below_card() {
        let card = SceneCard {
            photo_path: PathBuf::from("x.jpg"),
            dest: Rect {
                x: 100,
                y: 200,
                w: 740,
                h: 920,
            },
            z: 0,
            rotation_deg: 0.0,
            fit: core::Fit::Cover,
            pan_x: 0.0,
            pan_y: 0.0,
            flip_h: false,
            source_trim_left_frac: None,
            horizontal_center_band_frac: None,
            cover_height_first: false,
            polaroid: true,
            slot_seed: 42,
        };
        let verts = polaroid_vertices(&card, PolaroidDrawMode::Reflection);
        let min_y = verts
            .iter()
            .map(|v| v.pos[1])
            .fold(f32::INFINITY, f32::min);
        assert!(min_y >= 200.0 + 920.0 - 4.0);
    }
}

// The Anti-aliasing setting's FXAA post pass (render/fxaa.rs; shared/render/antialias.json). The
// frame drew into `picture`; this draws it onto the next target with its edges smoothed: Timothy
// Lottes's FXAA 3.11 "quality" algorithm (a luma edge test, the edge's direction, a short search
// along it for its ends, and a blend across it), written afresh in WGSL. The picture holds the
// encoded (display) colour, which is what FXAA's luma wants.

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

@vertex
fn fxaa_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}

// Contrast below max(EDGE_MIN, EDGE * the local max) is left alone.
const EDGE: f32 = 0.125;
const EDGE_MIN: f32 = 0.0312;
// Steps of the search along an edge, and the sub-pixel blend's strength.
const STEPS: i32 = 8;
const SUBPIX: f32 = 0.75;

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

fn lum_at(uv: vec2<f32>) -> f32 {
    return luma(textureSampleLevel(picture, linear_sampler, uv, 0.0).rgb);
}

@fragment
fn fxaa_fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(picture));
    let px = 1.0 / size;
    let uv = frag.xy * px;
    let centre = textureSampleLevel(picture, linear_sampler, uv, 0.0);
    let m = luma(centre.rgb);
    let n = lum_at(uv + vec2<f32>(0.0, -px.y));
    let s = lum_at(uv + vec2<f32>(0.0, px.y));
    let e = lum_at(uv + vec2<f32>(px.x, 0.0));
    let w = lum_at(uv + vec2<f32>(-px.x, 0.0));
    let hi = max(m, max(max(n, s), max(e, w)));
    let lo = min(m, min(min(n, s), min(e, w)));
    let range = hi - lo;
    if (range < max(EDGE_MIN, hi * EDGE)) {
        return centre;
    }
    let nw = lum_at(uv + vec2<f32>(-px.x, -px.y));
    let ne = lum_at(uv + vec2<f32>(px.x, -px.y));
    let sw = lum_at(uv + vec2<f32>(-px.x, px.y));
    let se = lum_at(uv + vec2<f32>(px.x, px.y));
    // Sub-pixel aliasing: how far the centre stands out from its neighbourhood's average.
    let avg = (2.0 * (n + s + e + w) + nw + ne + sw + se) / 12.0;
    let sub = clamp(abs(avg - m) / range, 0.0, 1.0);
    let sub_blend = smoothstep(0.0, 1.0, sub) * smoothstep(0.0, 1.0, sub) * SUBPIX;
    // The edge's direction: horizontal when the vertical change dominates.
    let horz = abs(nw + sw - 2.0 * w) + 2.0 * abs(n + s - 2.0 * m) + abs(ne + se - 2.0 * e);
    let vert = abs(nw + ne - 2.0 * n) + 2.0 * abs(w + e - 2.0 * m) + abs(sw + se - 2.0 * s);
    let horizontal = horz >= vert;
    let l1 = select(w, n, horizontal);
    let l2 = select(e, s, horizontal);
    let g1 = abs(l1 - m);
    let g2 = abs(l2 - m);
    var step_len = select(px.x, px.y, horizontal);
    var edge_lum: f32;
    var grad: f32;
    if (g1 >= g2) {
        step_len = -step_len;
        edge_lum = 0.5 * (l1 + m);
        grad = g1;
    } else {
        edge_lum = 0.5 * (l2 + m);
        grad = g2;
    }
    // Half a pixel across, onto the edge itself; then search both ways along it.
    var edge_uv = uv;
    if (horizontal) {
        edge_uv.y += 0.5 * step_len;
    } else {
        edge_uv.x += 0.5 * step_len;
    }
    let along = select(vec2<f32>(0.0, px.y), vec2<f32>(px.x, 0.0), horizontal);
    let scaled = 0.25 * grad;
    var p1 = edge_uv - along;
    var p2 = edge_uv + along;
    var d1 = lum_at(p1) - edge_lum;
    var d2 = lum_at(p2) - edge_lum;
    var done1 = abs(d1) >= scaled;
    var done2 = abs(d2) >= scaled;
    for (var k = 0; k < STEPS; k++) {
        if (done1 && done2) {
            break;
        }
        let stride = select(1.0, 2.0, k > 2);
        if (!done1) {
            p1 -= along * stride;
            d1 = lum_at(p1) - edge_lum;
            done1 = abs(d1) >= scaled;
        }
        if (!done2) {
            p2 += along * stride;
            d2 = lum_at(p2) - edge_lum;
            done2 = abs(d2) >= scaled;
        }
    }
    let dist1 = select(uv.y - p1.y, uv.x - p1.x, horizontal);
    let dist2 = select(p2.y - uv.y, p2.x - uv.x, horizontal);
    let nearer1 = dist1 < dist2;
    let dist = min(dist1, dist2);
    let span = dist1 + dist2;
    // Blend only when the centre is on the side of the edge the nearer end ends on.
    let centre_smaller = m < edge_lum;
    let ends_right = (select(d2, d1, nearer1) < 0.0) != centre_smaller;
    let edge_blend = select(0.0, 0.5 - dist / max(span, 1e-6), ends_right);
    let blend = max(edge_blend, sub_blend);
    var out_uv = uv;
    if (horizontal) {
        out_uv.y += blend * step_len;
    } else {
        out_uv.x += blend * step_len;
    }
    return textureSampleLevel(picture, linear_sampler, out_uv, 0.0);
}

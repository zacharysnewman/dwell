// The sky (dwell-look `sky_color` is `dwell_sky`'s twin). Needs the `DaySky` struct and the
// constants `sky_wgsl()` writes before this.

const SKY_TAU: f32 = 6.283185307179586;

// A light's halo toward x = cos(angle from it): a tight bloom and a broad glow (Gaussians).
fn dwell_halo(x: f32, bloom: f32, bloom_k: f32, glow: f32, glow_k: f32) -> f32 {
    return bloom * exp((x - 1.0) * bloom_k) + glow * exp((x - 1.0) * glow_k);
}

fn dwell_luma(c: vec3f) -> f32 {
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

// The sky's colour (output sRGB) toward the unit direction `dir` for a viewer on `face` (±1): the
// gradient, the warm horizon band toward the sun, the sun's and the moon's halos. Below the viewer's
// horizon, the horizon's colour in that azimuth (what far terrain's fog fades to).
fn dwell_sky(dir: vec3f, face: f32, day: DaySky) -> vec3f {
    let up = vec3f(0.0, face, 0.0);
    let above = dot(dir, up);
    var d = dir;
    if (above <= 0.0) {
        let level = dir - above * up;
        let len = length(level);
        if (len >= 1e-6) {
            d = level / len;
        }
    }
    let e = clamp(above, 0.0, 1.0);
    var c = mix(day.horizon.rgb, day.zenith.rgb, pow(max(e, 1e-6), GRADIENT_POWER));
    let sun = day.sun.xyz;
    let sh = sun - dot(sun, up) * up;
    let dh = d - dot(d, up) * up;
    let lens = length(sh) * length(dh);
    let toward = select(0.5, 0.5 + 0.5 * dot(dh, sh) / max(lens, 1e-6), lens > 1e-6);
    let band = day.band.a * pow(max(toward, 1e-6), BAND_SPREAD) * (1.0 - smoothstep(0.0, BAND_HEIGHT, e));
    c = mix(c, day.band.rgb, clamp(band, 0.0, 1.0));
    let g = day.glow.a * dwell_halo(dot(d, sun), SUN_BLOOM, SUN_BLOOM_K, SUN_GLOW, SUN_GLOW_K);
    // Capped, so the sun's disc stays brighter than the sky around it.
    c = mix(c, day.glow.rgb, clamp(g, 0.0, GLOW_CAP));
    let m = day.disc.a * dwell_halo(dot(d, day.moon.xyz), MOON_BLOOM, MOON_BLOOM_K, MOON_GLOW, MOON_GLOW_K);
    return mix(c, MOON_GLOW_COLOR, clamp(m, 0.0, 1.0));
}

// `v` turned by `angle` about the unit axis `k` (Rodrigues).
fn dwell_turn(v: vec3f, k: vec3f, angle: f32) -> vec3f {
    let c = cos(angle);
    let s = sin(angle);
    return v * c + cross(k, v) * s + k * dot(k, v) * (1.0 - c);
}

// Three hashes in [0, 1) of an integer lattice point (pcg3d).
fn dwell_hash3(p: vec3f) -> vec3f {
    var v = bitcast<vec3u>(vec3i(p)) * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v ^= v >> vec3u(16u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return vec3f(v) * (1.0 / 4294967296.0);
}

// The stars toward `dir` (added light): fixed to the sky, which turns about its axis with the sun,
// on a grid over the faces of a cube; at most one star per cell, kept clear of the cell's edges.
fn dwell_stars(dir: vec3f, day: DaySky) -> vec3f {
    let q = dwell_turn(dir, day.axis.xyz, -day.axis.w);
    let a = abs(q);
    var uv: vec2f;
    var side: f32;
    var m: f32;
    if (a.x >= a.y && a.x >= a.z) {
        m = a.x;
        uv = q.yz / a.x;
        side = select(1.0, 0.0, q.x > 0.0);
    } else if (a.y >= a.z) {
        m = a.y;
        uv = q.xz / a.y;
        side = select(3.0, 2.0, q.y > 0.0);
    } else {
        m = a.z;
        uv = q.xy / a.z;
        side = select(5.0, 4.0, q.z > 0.0);
    }
    let g = (uv * 0.5 + 0.5) * STAR_CELLS;
    let cell = floor(g);
    let h = dwell_hash3(vec3f(cell, side));
    if (h.x > STAR_DENSITY) {
        return vec3f(0.0);
    }
    let h2 = dwell_hash3(vec3f(cell + vec2f(17.0, 31.0), side + 7.0));
    let star = cell + 0.3 + 0.4 * h2.xy;
    // A pixel in cells: the cube's projection stretches toward its corners.
    let px_cells = max(day.stars.z * STAR_CELLS * 0.5 / pow(m, 1.5), 1e-6);
    let dist = length(g - star) / px_cells;
    let bright = pow(h.y, 6.0);
    let radius = STAR_SIZE * (0.8 + 0.7 * bright);
    let core = exp(-1.5 * dist * dist / (radius * radius));
    let twinkle = 1.0 - STAR_TWINKLE * (0.5 + 0.5 * sin(day.stars.y * SKY_TAU * STAR_TWINKLE_HZ * (0.6 + 0.8 * h2.z) + h2.x * SKY_TAU));
    let tint = mix(vec3f(0.78, 0.86, 1.0), vec3f(1.0, 0.92, 0.8), h.z);
    return tint * STAR_BRIGHTNESS * (0.1 + 0.9 * bright) * core * twinkle;
}

// Value noise in 2D (0–1).
fn dwell_noise2(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = dwell_hash3(vec3f(i, 91.0)).x;
    let b = dwell_hash3(vec3f(i + vec2f(1.0, 0.0), 91.0)).x;
    let c = dwell_hash3(vec3f(i + vec2f(0.0, 1.0), 91.0)).x;
    let d = dwell_hash3(vec3f(i + vec2f(1.0, 1.0), 91.0)).x;
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The moon's dark seas over its disc (p in the unit disc): 0 highland, 1 mare.
fn dwell_maria(p: vec2f) -> f32 {
    var f = 0.0;
    var amp = 0.5;
    var q = p * 2.3 + vec2f(3.1, 1.7);
    for (var i = 0; i < 4; i++) {
        f += amp * dwell_noise2(q);
        q = q * 2.07 + vec2f(1.3, 2.9);
        amp *= 0.5;
    }
    return smoothstep(0.46, 0.64, f);
}

// The moon's light toward `dir` (added) and how much of the pixel its disc covers (w). Its frame is
// built from world vectors (its direction and the sky's axis), so it keeps its face through the
// camera's turn-over. Lit from the sun's side by its phase: full faces the viewer, new faces away.
fn dwell_moon(dir: vec3f, day: DaySky, px: f32) -> vec4f {
    let z = day.moon.xyz;
    let x = normalize(cross(day.axis.xyz, z));
    let y = cross(z, x);
    let p = vec2f(dot(dir, x), dot(dir, y)) / sin(MOON_RADIUS);
    let r = length(p);
    let edge = 1.0 - smoothstep(1.0 - px / MOON_RADIUS, 1.0 + px / MOON_RADIUS, r);
    if (edge <= 0.0 || dot(dir, z) <= 0.0) {
        return vec4f(0.0);
    }
    let n = vec3f(p, sqrt(max(1.0 - r * r, 0.0)));
    let a = SKY_TAU * day.moon.w;
    let l = vec3f(sin(a), 0.0, -cos(a));
    let lit = smoothstep(-0.05, 0.09, dot(n, l));
    let albedo = MOON_COLOR * (1.0 - MOON_MARIA * dwell_maria(p));
    let limb = 0.7 + 0.3 * n.z;
    return vec4f(albedo * limb * (lit + MOON_EARTHSHINE), edge);
}

// The sun's disc toward `dir` (rgb) and its coverage (w): limb-darkened, larger and in the
// keyframes' warmer colour near the horizon.
fn dwell_sun_disc(dir: vec3f, up: vec3f, day: DaySky, px: f32) -> vec4f {
    let sun = day.sun.xyz;
    let radius = SUN_RADIUS * mix(SUN_HORIZON_SCALE, 1.0, smoothstep(0.0, 0.35, abs(dot(sun, up))));
    if (dot(dir, sun) < cos(radius + 3.0 * px)) {
        return vec4f(0.0);
    }
    let r = asin(min(length(cross(dir, sun)), 1.0)) / radius;
    let mu = sqrt(max(1.0 - r * r, 0.0));
    let limb = 1.0 - SUN_LIMB * (1.0 - mu);
    let edge = 1.0 - smoothstep(1.0 - px / radius, 1.0 + px / radius, r);
    let core = mix(day.disc.rgb, vec3f(1.0), 0.45 + 0.5 * mu * mu);
    return vec4f(core * limb, edge);
}

// The sky behind the world: `base` (its `dwell_sky`) with the stars, the moon and the sun, cut
// at the viewer's horizon.
fn dwell_sky_bodies(dir: vec3f, face: f32, day: DaySky, base: vec3f) -> vec3f {
    let up = vec3f(0.0, face, 0.0);
    let e = dot(dir, up);
    let px = max(day.stars.z, 1e-5);
    let shown = smoothstep(-px, px, e);
    if (shown <= 0.0) {
        return base;
    }
    // The moon's lit part hides its own glow behind it; its dark part shows the sky (and hides
    // the stars).
    let moon = dwell_moon(dir, day, px);
    var c = mix(base, max(base, base * 0.4 + moon.rgb), moon.w);
    let vis = day.stars.x * smoothstep(0.0, STAR_FADE, e) * (1.0 - smoothstep(0.08, 0.4, dwell_luma(base)));
    if (vis > 0.001) {
        c += dwell_stars(dir, day) * vis * (1.0 - moon.w);
    }
    // Never darker than the sky behind it.
    let sun = dwell_sun_disc(dir, up, day, px);
    c = mix(c, max(c, sun.rgb), sun.w);
    return mix(base, min(c, vec3f(1.0)), shown);
}

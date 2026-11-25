const PI: f32 = 3.14159265359;
const INV_PI: f32 = 0.31830988618;
const TWO_PI: f32 = 6.28318530718;

const BENCH_Y: f32 = -0.003;
const EPS_T: f32 = 1.5e-4;
const EPS_OFFSET: f32 = 2.0e-4;

const MED_AIR: u32 = 0u;
const MED_GLASS: u32 = 1u;
const MED_LIQUID: u32 = 2u;

const SURF_NONE: u32 = 0u;
const SURF_OUTER: u32 = 1u;
const SURF_INNER: u32 = 2u;
const SURF_MENISCUS: u32 = 3u;
const SURF_STOPPER: u32 = 4u;
const SURF_BENCH: u32 = 5u;
const SURF_CARD: u32 = 6u;
const SURF_KEY_LIGHT: u32 = 7u;
const SURF_FILL_LIGHT: u32 = 8u;

struct SpectralEntry {
    data0: vec4<f32>, // sigma_a_liquid, illuminant_spd, cmf_x, cmf_y
    data1: vec4<f32>, // cmf_z, sigma_a_glass, wl_icdf, wl_pdf
};

struct ProfileSegment {
    bounds: vec4<f32>, // y0, y1, r_min, r_max
    coeffs: vec4<f32>, // c0, c1, c2, c3
};

struct SceneUniform {
    cam_pos: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    cam_forward: vec4<f32>,

    bradford_row0: vec4<f32>,
    bradford_row1: vec4<f32>,
    bradford_row2: vec4<f32>,

    dims: vec4<u32>,         // width, height, frame_index, spp_per_frame
    counts: vec4<u32>,       // outer_segs, inner_segs, stopper_segs, total_caustic_photons
    flags: vec4<u32>,        // fill_active, stopper_type, enable_markings, enable_dispersion
    render_flags: vec4<u32>, // enable_caustics, lighting_preset, enable_white_card, tone_mapper
    render_flags2: vec4<u32>,// white_balance, caustic_map_size, photons_per_frame, show_grid

    flask0: vec4<f32>,       // y_mark, y_bot, y_contact, r_wall
    flask1: vec4<f32>,       // h_men, l_c, flask_h, base_t
    flask2: vec4<f32>,       // r_base_flat, r_inner_flat, r_rim_in, r_rim_out
    flask3: vec4<f32>,       // r_bulb_max, label_y_min, label_y_max, total_h_with_stopper
    optics: vec4<f32>,       // solvent_a, solvent_b, glass_a, glass_b
    scene_params0: vec4<f32>,// glass_tint_strength, light_azimuth, card_distance, bench_albedo
    scene_params1: vec4<f32>,// exposure, caustic_half_extent, d_liq_max, d_glass_max
};

@group(0) @binding(0) var<uniform> u: SceneUniform;
@group(0) @binding(1) var<storage, read> spectral_buf: array<SpectralEntry>;
@group(0) @binding(2) var<storage, read> outer_segs: array<ProfileSegment>;
@group(0) @binding(3) var<storage, read> inner_segs: array<ProfileSegment>;
@group(0) @binding(4) var<storage, read> stopper_segs: array<ProfileSegment>;
@group(0) @binding(5) var label_tex: texture_2d<f32>;
@group(0) @binding(6) var label_sampler: sampler;
@group(0) @binding(7) var<storage, read> beer_lut: array<vec4<f32>>;

// ---- PCG32 PRNG ----
fn pcg_next(state: ptr<function, u32>) -> f32 {
    let old = *state;
    *state = old * 747796405u + 2891336453u;
    let word = ((old >> ((old >> 28u) + 4u)) ^ old) * 277803737u;
    let res = (word >> 22u) ^ word;
    return f32(res) * (1.0 / 4294967296.0);
}

fn hash_u32(x_in: u32) -> u32 {
    var x = x_in;
    x = ((x >> 16u) ^ x) * 0x45d9f3bu;
    x = ((x >> 16u) ^ x) * 0x45d9f3bu;
    x = (x >> 16u) ^ x;
    return x;
}

// ---- Spectral Lookup ----
struct SpectralSample {
    sigma_liquid: f32,
    spd: f32,
    cmf: vec3<f32>,
    sigma_glass: f32,
};

fn eval_spectral(lambda_nm: f32) -> SpectralSample {
    let x = clamp(lambda_nm - 380.0, 0.0, 400.0);
    let i0 = min(u32(floor(x)), 399u);
    let t = x - f32(i0);
    let e0 = spectral_buf[i0];
    let e1 = spectral_buf[i0 + 1u];
    let d0 = mix(e0.data0, e1.data0, t);
    let d1 = mix(e0.data1, e1.data1, t);
    return SpectralSample(
        d0.x,
        d0.y,
        vec3<f32>(d0.z, d0.w, d1.x),
        d1.y * u.scene_params0.x
    );
}

fn cauchy_ior(a: f32, b: f32, lambda_nm: f32) -> f32 {
    let l_eff = select(589.3, lambda_nm, u.flags.w != 0u);
    let l_um = l_eff * 0.001;
    return a + b / (l_um * l_um);
}

fn ior_glass(lambda_nm: f32) -> f32 {
    return cauchy_ior(u.optics.z, u.optics.w, lambda_nm);
}

fn ior_liquid(lambda_nm: f32) -> f32 {
    return cauchy_ior(u.optics.x, u.optics.y, lambda_nm);
}

fn eval_beer_lut(d_liq: f32, d_glass: f32) -> vec3<f32> {
    let d_liq_max = max(u.scene_params1.z, 1e-3);
    let d_glass_max = max(u.scene_params1.w, 1e-3);
    let uc = sqrt(clamp(d_liq / d_liq_max, 0.0, 1.0)) * 511.0;
    let vc = sqrt(clamp(d_glass / d_glass_max, 0.0, 1.0)) * 15.0;
    let i0 = min(u32(floor(uc)), 510u);
    let j0 = min(u32(floor(vc)), 14u);
    let fx = clamp(uc - f32(i0), 0.0, 1.0);
    let fy = clamp(vc - f32(j0), 0.0, 1.0);
    let row0 = j0 * 512u;
    let row1 = (j0 + 1u) * 512u;
    let c00 = beer_lut[row0 + i0].xyz;
    let c10 = beer_lut[row0 + i0 + 1u].xyz;
    let c01 = beer_lut[row1 + i0].xyz;
    let c11 = beer_lut[row1 + i0 + 1u].xyz;
    return mix(mix(c00, c10, fx), mix(c01, c11, fx), fy);
}

fn sample_importance_lambda(u_in: f32) -> vec2<f32> {
    let x = clamp(u_in, 0.0, 0.999999) * 400.0;
    let k0 = min(u32(floor(x)), 399u);
    let tf = x - f32(k0);
    let lambda_nm = mix(spectral_buf[k0].data1.z, spectral_buf[k0 + 1u].data1.z, tf);

    let lx = clamp(lambda_nm - 380.0, 0.0, 400.0);
    let l0 = min(u32(floor(lx)), 399u);
    let lf = lx - f32(l0);
    let pdf = mix(spectral_buf[l0].data1.w, spectral_buf[l0 + 1u].data1.w, lf);
    return vec2<f32>(lambda_nm, 1.0 / max(pdf, 1e-6));
}

// ---- Fresnel & Snell ----
fn fresnel_dielectric(cos_i_in: f32, eta_i: f32, eta_t: f32) -> f32 {
    let cos_i = clamp(abs(cos_i_in), 0.0, 1.0);
    let sin2_t = (eta_i / eta_t) * (eta_i / eta_t) * max(0.0, 1.0 - cos_i * cos_i);
    if (sin2_t >= 1.0) {
        return 1.0;
    }
    let cos_t = sqrt(max(0.0, 1.0 - sin2_t));
    let rs = (eta_i * cos_i - eta_t * cos_t) / max(1e-7, eta_i * cos_i + eta_t * cos_t);
    let rp = (eta_t * cos_i - eta_i * cos_t) / max(1e-7, eta_t * cos_i + eta_i * cos_t);
    return clamp(0.5 * (rs * rs + rp * rp), 0.0, 1.0);
}

fn refract_exact(wi: vec3<f32>, n_facing: vec3<f32>, eta: f32) -> vec4<f32> {
    // Returns (dir.xyz, valid 1.0/0.0) where wi points AWAY from surface or INTO surface?
    // Convention here: incident ray direction rd points INTO hit point (dot(rd, n_facing) < 0).
    let cos_i = -dot(wi, n_facing);
    let k = 1.0 - eta * eta * max(0.0, 1.0 - cos_i * cos_i);
    if (k < 0.0) {
        return vec4<f32>(reflect(wi, n_facing), 0.0);
    }
    let dir = normalize(eta * wi + (eta * cos_i - sqrt(k)) * n_facing);
    return vec4<f32>(dir, 1.0);
}

// ---- Overflow-Free Young-Laplace Capillary Meniscus ----
fn eval_meniscus_y(r_in: f32) -> f32 {
    let y_bot = u.flask0.y;
    let r_wall = max(u.flask0.w, 1e-4);
    let h_men = u.flask1.x;
    let l_c = max(u.flask1.y, 1e-4);
    let r = min(r_in, r_wall);

    let e_pos = exp((r - r_wall) / l_c);
    let e_neg = exp(-(r + r_wall) / l_c);
    let e_wall = exp(-r_wall / l_c);
    let e_2w = exp(-2.0 * r_wall / l_c);
    let denom = max(1e-7, 1.0 + e_2w - 2.0 * e_wall);
    return y_bot + h_men * (e_pos + e_neg - 2.0 * e_wall) / denom;
}

fn eval_meniscus_dy_dr(r_in: f32) -> f32 {
    let r_wall = max(u.flask0.w, 1e-4);
    let h_men = u.flask1.x;
    let l_c = max(u.flask1.y, 1e-4);
    let r = min(r_in, r_wall);

    let e_pos = exp((r - r_wall) / l_c);
    let e_neg = exp(-(r + r_wall) / l_c);
    let e_wall = exp(-r_wall / l_c);
    let e_2w = exp(-2.0 * r_wall / l_c);
    let denom = max(1e-7, 1.0 + e_2w - 2.0 * e_wall);
    return (h_men / (l_c * denom)) * (e_pos - e_neg);
}

// ---- Smooth Profile Segment Intersector ----
fn eval_seg_r_and_dr(seg: ProfileSegment, y: f32) -> vec2<f32> {
    let dy = max(seg.bounds.y - seg.bounds.x, 1e-6);
    let u_val = clamp((y - seg.bounds.x) / dy, 0.0, 1.0);
    let c = seg.coeffs;
    let r = c.x + u_val * (c.y + u_val * (c.z + u_val * c.w));
    let dr_dy = (c.y + u_val * (2.0 * c.z + 3.0 * c.w * u_val)) / dy;
    return vec2<f32>(r, dr_dy);
}

fn eval_seg_f_and_df(seg: ProfileSegment, ro: vec3<f32>, rd: vec3<f32>, t: f32) -> vec2<f32> {
    let p = ro + t * rd;
    let rd_r = eval_seg_r_and_dr(seg, p.y);
    let f = (p.x * p.x + p.z * p.z) - rd_r.x * rd_r.x;
    let df = 2.0 * (p.x * rd.x + p.z * rd.z - rd_r.x * rd_r.y * rd.y);
    return vec2<f32>(f, df);
}

fn solve_bracket_newton(seg: ProfileSegment, ro: vec3<f32>, rd: vec3<f32>, t_a_in: f32, t_b_in: f32, fa_in: f32) -> f32 {
    var lo = t_a_in;
    var hi = t_b_in;
    var flo = fa_in;
    var t = 0.5 * (lo + hi);
    for (var it = 0u; it < 12u; it = it + 1u) {
        let fd = eval_seg_f_and_df(seg, ro, rd, t);
        if (abs(fd.x) < 1e-7) {
            return t;
        }
        if ((flo <= 0.0 && fd.x <= 0.0) || (flo >= 0.0 && fd.x >= 0.0)) {
            lo = t;
            flo = fd.x;
        } else {
            hi = t;
        }
        var t_next = 0.5 * (lo + hi);
        if (abs(fd.y) > 1e-7) {
            let t_newton = t - fd.x / fd.y;
            if (t_newton > lo && t_newton < hi) {
                t_next = t_newton;
            }
        }
        t = t_next;
    }
    return t;
}

fn find_seg_hit(seg: ProfileSegment, ro: vec3<f32>, rd: vec3<f32>, t_max_limit: f32) -> f32 {
    var t_lo = EPS_T;
    var t_hi = t_max_limit;
    if (abs(rd.y) > 1e-7) {
        let ty0 = (seg.bounds.x - ro.y) / rd.y;
        let ty1 = (seg.bounds.y - ro.y) / rd.y;
        t_lo = max(t_lo, min(ty0, ty1));
        t_hi = min(t_hi, max(ty0, ty1));
    } else if (ro.y < seg.bounds.x || ro.y > seg.bounds.y) {
        return -1.0;
    }
    if (t_lo >= t_hi) {
        return -1.0;
    }

    let a_xz = rd.x * rd.x + rd.z * rd.z;
    let b_xz = 2.0 * (ro.x * rd.x + ro.z * rd.z);
    let r_bound = seg.bounds.w + 0.005;
    let c_xz = ro.x * ro.x + ro.z * ro.z - r_bound * r_bound;
    if (a_xz > 1e-10) {
        let disc = b_xz * b_xz - 4.0 * a_xz * c_xz;
        if (disc < 0.0) {
            return -1.0;
        }
        let sq = sqrt(disc);
        t_lo = max(t_lo, (-b_xz - sq) / (2.0 * a_xz));
        t_hi = min(t_hi, (-b_xz + sq) / (2.0 * a_xz));
        if (t_lo >= t_hi) {
            return -1.0;
        }
    } else if (c_xz > 0.0) {
        return -1.0;
    }

    let n_sub = 6u;
    let dt = (t_hi - t_lo) / f32(n_sub);
    var t_prev = t_lo;
    var fd_prev = eval_seg_f_and_df(seg, ro, rd, t_prev);

    for (var i = 1u; i <= n_sub; i = i + 1u) {
        let t_curr = select(t_lo + f32(i) * dt, t_hi, i == n_sub);
        let fd_curr = eval_seg_f_and_df(seg, ro, rd, t_curr);

        if ((fd_prev.x <= 0.0 && fd_curr.x >= 0.0) || (fd_prev.x >= 0.0 && fd_curr.x <= 0.0)) {
            return solve_bracket_newton(seg, ro, rd, t_prev, t_curr, fd_prev.x);
        }

        // Grazing silhouette detector: if f keeps same sign at both ends and df changes sign across an extremum
        let convex_grazing = fd_prev.x > 0.0 && fd_curr.x > 0.0 && fd_prev.y < 0.0 && fd_curr.y > 0.0;
        let concave_grazing = fd_prev.x < 0.0 && fd_curr.x < 0.0 && fd_prev.y > 0.0 && fd_curr.y < 0.0;
        if (convex_grazing || concave_grazing) {
            var m_lo = t_prev;
            var m_hi = t_curr;
            for (var k = 0u; k < 8u; k = k + 1u) {
                let tm = 0.5 * (m_lo + m_hi);
                let fdm = eval_seg_f_and_df(seg, ro, rd, tm);
                if ((fd_prev.x > 0.0 && fdm.x <= 0.0) || (fd_prev.x < 0.0 && fdm.x >= 0.0)) {
                    return solve_bracket_newton(seg, ro, rd, t_prev, tm, fd_prev.x);
                }
                if ((convex_grazing && fdm.y < 0.0) || (concave_grazing && fdm.y > 0.0)) {
                    m_lo = tm;
                } else {
                    m_hi = tm;
                }
            }
        }

        t_prev = t_curr;
        fd_prev = fd_curr;
    }
    return -1.0;
}

struct Hit {
    t: f32,
    pos: vec3<f32>,
    normal: vec3<f32>, // outward for outer/stopper/box, inward for inner, upward for meniscus/bench
    surf_id: u32,
};

fn miss_hit() -> Hit {
    return Hit(1e20, vec3<f32>(0.0), vec3<f32>(0.0, 1.0, 0.0), SURF_NONE);
}

fn seg_normal(seg: ProfileSegment, p: vec3<f32>, inward: bool) -> vec3<f32> {
    let r = sqrt(p.x * p.x + p.z * p.z);
    let rr = eval_seg_r_and_dr(seg, p.y);
    var n: vec3<f32>;
    if (r > 1e-6) {
        n = normalize(vec3<f32>(p.x / r, -rr.y, p.z / r));
    } else {
        n = vec3<f32>(1.0, 0.0, 0.0);
    }
    return select(n, -n, inward);
}

fn intersect_disk(ro: vec3<f32>, rd: vec3<f32>, y_plane: f32, r_min: f32, r_max: f32, t_best: f32) -> f32 {
    if (abs(rd.y) < 1e-7) {
        return -1.0;
    }
    let t = (y_plane - ro.y) / rd.y;
    if (t <= EPS_T || t >= t_best) {
        return -1.0;
    }
    let px = ro.x + t * rd.x;
    let pz = ro.z + t * rd.z;
    let r2 = px * px + pz * pz;
    if (r2 >= r_min * r_min && r2 <= r_max * r_max) {
        return t;
    }
    return -1.0;
}

fn eval_men_g_and_dg(ro: vec3<f32>, rd: vec3<f32>, t: f32) -> vec2<f32> {
    let p = ro + t * rd;
    let r = sqrt(p.x * p.x + p.z * p.z);
    let ym = eval_meniscus_y(r);
    let dydr = eval_meniscus_dy_dr(r);
    let dr_dt = select(0.0, (p.x * rd.x + p.z * rd.z) / r, r > 1e-6);
    return vec2<f32>(p.y - ym, rd.y - dydr * dr_dt);
}

fn intersect_meniscus(ro: vec3<f32>, rd: vec3<f32>, t_best: f32) -> Hit {
    if (u.flags.x == 0u) {
        return miss_hit();
    }
    let r_wall = u.flask0.w + 0.002;
    let y_lo = u.flask0.y - 0.002;
    let y_hi = u.flask0.z + 0.002;

    var t_lo = EPS_T;
    var t_hi = t_best;
    if (abs(rd.y) > 1e-7) {
        let ty0 = (y_lo - ro.y) / rd.y;
        let ty1 = (y_hi - ro.y) / rd.y;
        t_lo = max(t_lo, min(ty0, ty1));
        t_hi = min(t_hi, max(ty0, ty1));
    } else if (ro.y < y_lo || ro.y > y_hi) {
        return miss_hit();
    }
    if (t_lo >= t_hi) {
        return miss_hit();
    }

    let a_xz = rd.x * rd.x + rd.z * rd.z;
    let b_xz = 2.0 * (ro.x * rd.x + ro.z * rd.z);
    let c_xz = ro.x * ro.x + ro.z * ro.z - r_wall * r_wall;
    if (a_xz > 1e-10) {
        let disc = b_xz * b_xz - 4.0 * a_xz * c_xz;
        if (disc < 0.0) {
            return miss_hit();
        }
        let sq = sqrt(disc);
        t_lo = max(t_lo, (-b_xz - sq) / (2.0 * a_xz));
        t_hi = min(t_hi, (-b_xz + sq) / (2.0 * a_xz));
        if (t_lo >= t_hi) {
            return miss_hit();
        }
    } else if (c_xz > 0.0) {
        return miss_hit();
    }

    let n_sub = 10u;
    let dt = (t_hi - t_lo) / f32(n_sub);
    var t_prev = t_lo;
    var gd_prev = eval_men_g_and_dg(ro, rd, t_prev);
    for (var i = 1u; i <= n_sub; i = i + 1u) {
        let t_curr = select(t_lo + f32(i) * dt, t_hi, i == n_sub);
        let gd_curr = eval_men_g_and_dg(ro, rd, t_curr);
        if ((gd_prev.x <= 0.0 && gd_curr.x >= 0.0) || (gd_prev.x >= 0.0 && gd_curr.x <= 0.0)) {
            var lo = t_prev;
            var hi = t_curr;
            var glo = gd_prev.x;
            var t = 0.5 * (lo + hi);
            for (var it = 0u; it < 12u; it = it + 1u) {
                let gd = eval_men_g_and_dg(ro, rd, t);
                if (abs(gd.x) < 1e-7) {
                    break;
                }
                if ((glo <= 0.0 && gd.x <= 0.0) || (glo >= 0.0 && gd.x >= 0.0)) {
                    lo = t;
                    glo = gd.x;
                } else {
                    hi = t;
                }
                var tn = 0.5 * (lo + hi);
                if (abs(gd.y) > 1e-7) {
                    let cand = t - gd.x / gd.y;
                    if (cand > lo && cand < hi) {
                        tn = cand;
                    }
                }
                t = tn;
            }
            let p = ro + t * rd;
            let r = sqrt(p.x * p.x + p.z * p.z);
            if (r <= u.flask0.w + 0.001) {
                let dydr = eval_meniscus_dy_dr(r);
                let slope_over_r = select(0.0, dydr / r, r > 1e-5);
                let n = normalize(vec3<f32>(-slope_over_r * p.x, 1.0, -slope_over_r * p.z));
                return Hit(t, p, n, SURF_MENISCUS);
            }
        }
        t_prev = t_curr;
        gd_prev = gd_curr;
    }
    return miss_hit();
}

fn intersect_flask(ro: vec3<f32>, rd: vec3<f32>, t_limit: f32) -> Hit {
    var best = Hit(t_limit, vec3<f32>(0.0), vec3<f32>(0.0, 1.0, 0.0), SURF_NONE);

    // Quick bounding-cylinder + height slab reject
    let r_bound = max(u.flask3.x, 1.5) + 0.25;
    let h_bound = u.flask3.w + 0.1;
    var tb_lo = EPS_T;
    var tb_hi = best.t;
    if (abs(rd.y) > 1e-7) {
        let ty0 = (-0.01 - ro.y) / rd.y;
        let ty1 = (h_bound - ro.y) / rd.y;
        tb_lo = max(tb_lo, min(ty0, ty1));
        tb_hi = min(tb_hi, max(ty0, ty1));
    } else if (ro.y < -0.01 || ro.y > h_bound) {
        return best;
    }
    if (tb_lo >= tb_hi) {
        return best;
    }
    let a_xz = rd.x * rd.x + rd.z * rd.z;
    let b_xz = 2.0 * (ro.x * rd.x + ro.z * rd.z);
    let c_xz = ro.x * ro.x + ro.z * ro.z - r_bound * r_bound;
    if (a_xz > 1e-10) {
        let disc = b_xz * b_xz - 4.0 * a_xz * c_xz;
        if (disc < 0.0) {
            return best;
        }
        let sq = sqrt(disc);
        tb_lo = max(tb_lo, (-b_xz - sq) / (2.0 * a_xz));
        tb_hi = min(tb_hi, (-b_xz + sq) / (2.0 * a_xz));
        if (tb_lo >= tb_hi) {
            return best;
        }
    } else if (c_xz > 0.0) {
        return best;
    }

    // 1. Outer profile
    let n_outer = u.counts.x;
    for (var i = 0u; i < n_outer; i = i + 1u) {
        let seg = outer_segs[i];
        let t = find_seg_hit(seg, ro, rd, best.t);
        if (t > EPS_T && t < best.t) {
            let p = ro + t * rd;
            best = Hit(t, p, seg_normal(seg, p, false), SURF_OUTER);
        }
    }
    // Outer flat base disk (y = 0) & top rim annulus (y = flask_h)
    let t_base = intersect_disk(ro, rd, 0.0, 0.0, u.flask2.x, best.t);
    if (t_base > EPS_T && t_base < best.t) {
        best = Hit(t_base, ro + t_base * rd, vec3<f32>(0.0, -1.0, 0.0), SURF_OUTER);
    }
    let t_rim = intersect_disk(ro, rd, u.flask1.z, u.flask2.z, u.flask2.w, best.t);
    if (t_rim > EPS_T && t_rim < best.t) {
        best = Hit(t_rim, ro + t_rim * rd, vec3<f32>(0.0, 1.0, 0.0), SURF_OUTER);
    }

    // 2. Inner profile + inner flat floor (normal points from Cavity -> Glass, i.e. radially outward / -Y at floor)
    let n_inner = u.counts.y;
    for (var i = 0u; i < n_inner; i = i + 1u) {
        let seg = inner_segs[i];
        let t = find_seg_hit(seg, ro, rd, best.t);
        if (t > EPS_T && t < best.t) {
            let p = ro + t * rd;
            best = Hit(t, p, seg_normal(seg, p, false), SURF_INNER);
        }
    }
    let t_in_floor = intersect_disk(ro, rd, u.flask1.w, 0.0, u.flask2.y, best.t);
    if (t_in_floor > EPS_T && t_in_floor < best.t) {
        best = Hit(t_in_floor, ro + t_in_floor * rd, vec3<f32>(0.0, -1.0, 0.0), SURF_INNER);
    }

    // 3. Meniscus
    let m_hit = intersect_meniscus(ro, rd, best.t);
    if (m_hit.surf_id != SURF_NONE && m_hit.t < best.t) {
        best = m_hit;
    }

    // 4. Optional Stopper
    if (u.flags.y != 0u) {
        let n_stop = u.counts.z;
        for (var i = 0u; i < n_stop; i = i + 1u) {
            let seg = stopper_segs[i];
            let t = find_seg_hit(seg, ro, rd, best.t);
            if (t > EPS_T && t < best.t) {
                let p = ro + t * rd;
                best = Hit(t, p, seg_normal(seg, p, false), SURF_STOPPER);
            }
        }
        if (n_stop > 0u) {
            let s0 = stopper_segs[0u];
            let s_last = stopper_segs[n_stop - 1u];
            let t_sb = intersect_disk(ro, rd, s0.bounds.x, 0.0, s0.coeffs.x + 0.002, best.t);
            if (t_sb > EPS_T && t_sb < best.t) {
                best = Hit(t_sb, ro + t_sb * rd, vec3<f32>(0.0, -1.0, 0.0), SURF_STOPPER);
            }
            let r_top = s_last.coeffs.x + s_last.coeffs.y + s_last.coeffs.z + s_last.coeffs.w;
            let t_st = intersect_disk(ro, rd, s_last.bounds.y, 0.0, r_top + 0.002, best.t);
            if (t_st > EPS_T && t_st < best.t) {
                best = Hit(t_st, ro + t_st * rd, vec3<f32>(0.0, 1.0, 0.0), SURF_STOPPER);
            }
        }
    }

    return best;
}

// ---- Studio Lights ----
struct StudioLight {
    pos: vec3<f32>,
    normal: vec3<f32>, // points toward scene center
    radius: f32,
    le_scale: f32,
};

fn build_disk_light(pos: vec3<f32>, aim: vec3<f32>, radius: f32, le_scale: f32) -> StudioLight {
    let n = normalize(aim - pos);
    return StudioLight(pos, n, radius, le_scale);
}

fn get_key_light() -> StudioLight {
    let h = u.flask1.z;
    let aim = vec3<f32>(0.0, 0.42 * h, 0.0);
    let az = u.scene_params0.y;
    let preset = u.render_flags.y;

    if (preset == 1u) {
        // Backlit spectrophotometer lightbox
        let dist = max(20.0, 1.35 * h);
        return build_disk_light(vec3<f32>(0.0, 0.48 * h, -dist), aim, max(12.0, 0.85 * h), 5.0);
    } else if (preset == 2u) {
        // Overhead lab lamp
        let dist = max(36.0, 2.2 * h);
        return build_disk_light(
            vec3<f32>(3.0 * sin(az), dist, -2.5 * cos(az)),
            aim,
            max(4.5, 0.28 * h),
            42.0,
        );
    } else if (preset == 3u) {
        // Low side window daylight
        let dist = max(38.0, 2.3 * h);
        let dir = normalize(vec3<f32>(cos(az) * 0.90, 0.44, sin(az) * 0.55));
        return build_disk_light(aim + dir * dist, aim, max(8.5, 0.50 * h), 28.0);
    } else {
        // Lab Studio (angled softbox kept ahead of white card plane)
        let dist = max(36.0, 2.15 * h);
        let dir = normalize(vec3<f32>(cos(az) * 0.76, 0.68, sin(az) * 0.45));
        return build_disk_light(aim + dir * dist, aim, max(5.8, 0.34 * h), 34.0);
    }
}

fn get_fill_light() -> StudioLight {
    let h = u.flask1.z;
    let aim = vec3<f32>(0.0, 0.46 * h, 0.0);
    let az = u.scene_params0.y;
    let preset = u.render_flags.y;

    if (preset == 1u) {
        // Gentle front-right specular rim fill for backlit mode
        let dist = max(32.0, 1.9 * h);
        return build_disk_light(vec3<f32>(0.7 * dist, 0.8 * dist, 0.6 * dist), aim, max(7.0, 0.42 * h), 4.5);
    } else {
        let dist = max(36.0, 2.1 * h);
        let dir = normalize(vec3<f32>(-cos(az) * 0.85, 0.72, 0.35));
        return build_disk_light(aim + dir * dist, aim, max(9.5, 0.55 * h), 2.4);
    }
}

fn intersect_disk_light(ro: vec3<f32>, rd: vec3<f32>, light: StudioLight, t_best: f32) -> f32 {
    let denom = dot(rd, light.normal);
    if (denom >= -1e-6) {
        return -1.0; // back side or parallel
    }
    let t = dot(light.pos - ro, light.normal) / denom;
    if (t <= EPS_T || t >= t_best) {
        return -1.0;
    }
    let p = ro + t * rd;
    let d = p - light.pos;
    if (dot(d, d) <= light.radius * light.radius) {
        return t;
    }
    return -1.0;
}

fn intersect_box(ro: vec3<f32>, rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>, t_best: f32) -> vec4<f32> {
    // Returns (t, normal.xyz) with t < 0 if miss
    let inv_d = 1.0 / select(rd, vec3<f32>(1e-8), abs(rd) < vec3<f32>(1e-8));
    let t0s = (bmin - ro) * inv_d;
    let t1s = (bmax - ro) * inv_d;
    let tsmaller = min(t0s, t1s);
    let tbigger = max(t0s, t1s);
    let t_enter = max(max(tsmaller.x, tsmaller.y), tsmaller.z);
    let t_exit = min(min(tbigger.x, tbigger.y), tbigger.z);

    if (t_enter > t_exit || t_exit < EPS_T) {
        return vec4<f32>(-1.0, 0.0, 0.0, 0.0);
    }
    let t = select(t_exit, t_enter, t_enter > EPS_T);
    if (t <= EPS_T || t >= t_best) {
        return vec4<f32>(-1.0, 0.0, 0.0, 0.0);
    }
    let p = ro + t * rd;
    let c = 0.5 * (bmin + bmax);
    let half_s = 0.5 * (bmax - bmin);
    let rel = (p - c) / max(half_s, vec3<f32>(1e-5));
    let abs_rel = abs(rel);
    var n = vec3<f32>(0.0);
    if (abs_rel.x >= abs_rel.y && abs_rel.x >= abs_rel.z) {
        n = vec3<f32>(sign(rel.x), 0.0, 0.0);
    } else if (abs_rel.y >= abs_rel.z) {
        n = vec3<f32>(0.0, sign(rel.y), 0.0);
    } else {
        n = vec3<f32>(0.0, 0.0, sign(rel.z));
    }
    return vec4<f32>(t, n);
}

fn intersect_scene(ro: vec3<f32>, rd: vec3<f32>, include_lights: bool) -> Hit {
    var best = intersect_flask(ro, rd, 1e20);

    // Lab bench plane at y = BENCH_Y
    if (abs(rd.y) > 1e-7) {
        let tb = (BENCH_Y - ro.y) / rd.y;
        if (tb > EPS_T && tb < best.t) {
            let pb = ro + tb * rd;
            if (pb.x * pb.x + pb.z * pb.z <= 120.0 * 120.0) {
                best = Hit(tb, pb, vec3<f32>(0.0, 1.0, 0.0), SURF_BENCH);
            }
        }
    }

    // White Contrast Card behind flask
    if (u.render_flags.z != 0u && u.render_flags.y != 1u) {
        let card_hw = max(5.5, 2.2 * u.flask3.x);
        let card_h = max(14.0, 1.08 * u.flask1.z);
        let card_z = -u.scene_params0.z;
        let bmin = vec3<f32>(-card_hw, BENCH_Y, card_z - 0.16);
        let bmax = vec3<f32>(card_hw, BENCH_Y + card_h, card_z);
        let bh = intersect_box(ro, rd, bmin, bmax, best.t);
        if (bh.x > EPS_T && bh.x < best.t) {
            best = Hit(bh.x, ro + bh.x * rd, bh.yzw, SURF_CARD);
        }
    }

    if (include_lights) {
        let key = get_key_light();
        let tk = intersect_disk_light(ro, rd, key, best.t);
        if (tk > EPS_T && tk < best.t) {
            best = Hit(tk, ro + tk * rd, key.normal, SURF_KEY_LIGHT);
        }
        let fill = get_fill_light();
        let tf = intersect_disk_light(ro, rd, fill, best.t);
        if (tf > EPS_T && tf < best.t) {
            best = Hit(tf, ro + tf * rd, fill.normal, SURF_FILL_LIGHT);
        }
    }

    return best;
}

fn eval_enamel_mask(p: vec3<f32>) -> f32 {
    if (u.flags.z == 0u) {
        return 0.0;
    }
    let y_mark = u.flask0.x;
    // Crisp 360-degree etched white calibration ring at y_mark
    if (abs(p.y - y_mark) < 0.028) {
        return 1.0;
    }
    let ly0 = u.flask3.y;
    let ly1 = u.flask3.z;
    if (p.y >= ly0 && p.y <= ly1 && p.z > 0.0) {
        let theta = atan2(p.x, p.z); // 0 is +Z front facing default camera
        let max_theta = 0.62;
        if (abs(theta) < max_theta) {
            let uv_x = (theta + max_theta) / (2.0 * max_theta);
            let uv_y = 1.0 - (p.y - ly0) / max(ly1 - ly0, 1e-4);
            let val = textureSampleLevel(label_tex, label_sampler, vec2<f32>(uv_x, uv_y), 0.0).r;
            return val;
        }
    }
    return 0.0;
}

fn eval_env_spd(rd: vec3<f32>) -> f32 {
    // Clean studio gradient dome; brighter overhead, deep neutral slate near horizon
    let t = clamp(0.5 * (rd.y + 1.0), 0.0, 1.0);
    let base = mix(0.08, 0.24, smoothstep(0.35, 0.95, t));
    // Subtle wrap studio glow in upper sides for realistic glass silhouette reflections
    let band = exp(-pow((rd.y - 0.35) / 0.25, 2.0)) * 0.14;
    return base + band;
}

fn make_onb(n: vec3<f32>) -> mat3x3<f32> {
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.y) < 0.99);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return mat3x3<f32>(t, b, n);
}

fn sample_cosine_hemisphere(n: vec3<f32>, rng: ptr<function, u32>) -> vec3<f32> {
    let u1 = pcg_next(rng);
    let u2 = pcg_next(rng);
    let r = sqrt(u1);
    let phi = TWO_PI * u2;
    let local = vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(0.0, 1.0 - u1)));
    let onb = make_onb(n);
    return normalize(onb * local);
}

struct DielectricBoundary {
    med_i: u32,
    med_t: u32,
    n_facing: vec3<f32>,
};

fn medium_ior(med: u32, lambda_nm: f32) -> f32 {
    if (med == MED_GLASS) {
        return ior_glass(lambda_nm);
    } else if (med == MED_LIQUID) {
        return ior_liquid(lambda_nm);
    }
    return 1.0;
}

fn resolve_dielectric_boundary(hit: Hit, rd: vec3<f32>) -> DielectricBoundary {
    let entering = dot(rd, hit.normal) < 0.0;
    let n_facing = select(-hit.normal, hit.normal, entering);

    if (hit.surf_id == SURF_OUTER || hit.surf_id == SURF_STOPPER) {
        let med_i = select(MED_GLASS, MED_AIR, entering);
        let med_t = select(MED_AIR, MED_GLASS, entering);
        return DielectricBoundary(med_i, med_t, n_facing);
    } else if (hit.surf_id == SURF_INNER) {
        let r = sqrt(hit.pos.x * hit.pos.x + hit.pos.z * hit.pos.z);
        let in_liquid = (u.flags.x != 0u) && (hit.pos.y <= eval_meniscus_y(r) + 2e-4);
        let med_cavity = select(MED_AIR, MED_LIQUID, in_liquid);
        let med_i = select(med_cavity, MED_GLASS, entering);
        let med_t = select(MED_GLASS, med_cavity, entering);
        return DielectricBoundary(med_i, med_t, n_facing);
    } else {
        // SURF_MENISCUS (normal points upward into Air)
        let med_i = select(MED_LIQUID, MED_AIR, entering);
        let med_t = select(MED_AIR, MED_LIQUID, entering);
        return DielectricBoundary(med_i, med_t, n_facing);
    }
}

fn xyz_to_lin_srgb(xyz: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
         3.2404542 * xyz.x - 1.5371385 * xyz.y - 0.4985314 * xyz.z,
        -0.9692660 * xyz.x + 1.8760108 * xyz.y + 0.0415560 * xyz.z,
         0.0556434 * xyz.x - 0.2040259 * xyz.y + 1.0572252 * xyz.z,
    );
}

fn lin_srgb_to_xyz(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.4124564 * rgb.x + 0.3575761 * rgb.y + 0.1804375 * rgb.z,
        0.2126729 * rgb.x + 0.7151522 * rgb.y + 0.0721750 * rgb.z,
        0.0193339 * rgb.x + 0.1191920 * rgb.y + 0.9503041 * rgb.z,
    );
}

fn gamut_map_srgb(rgb: vec3<f32>) -> vec3<f32> {
    let luma = 0.2126 * rgb.x + 0.7152 * rgb.y + 0.0722 * rgb.z;
    if (luma <= 0.0) {
        return vec3<f32>(0.0);
    }
    let m = min(rgb.x, min(rgb.y, rgb.z));
    var mapped = rgb;
    if (m < 0.0) {
        let t = luma / (luma - m);
        mapped = vec3<f32>(luma) + t * (rgb - vec3<f32>(luma));
    }
    return max(mapped, vec3<f32>(0.0));
}

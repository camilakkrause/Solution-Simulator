@group(1) @binding(0) var<storage, read_write> accum_xyz: array<vec4<f32>>;
@group(1) @binding(1) var<storage, read> caustic_accum: array<vec4<f32>>;

fn sample_caustic_xyz_raw(ix: i32, iy: i32, n_i: i32) -> vec3<f32> {
    let cx = clamp(ix, 0, n_i - 1);
    let cy = clamp(iy, 0, n_i - 1);
    return caustic_accum[u32(cy * n_i + cx)].xyz;
}

fn lookup_caustic_irradiance_xyz(pos_xz: vec2<f32>) -> vec3<f32> {
    if (u.render_flags.x == 0u) {
        return vec3<f32>(0.0);
    }
    let half_ext = max(u.scene_params1.y, 1.0);
    if (abs(pos_xz.x) >= half_ext || abs(pos_xz.y) >= half_ext) {
        return vec3<f32>(0.0);
    }
    let n_i = i32(u.render_flags2.y);
    let n_f = f32(n_i);
    let uc = (pos_xz.x / (2.0 * half_ext) + 0.5) * n_f - 0.5;
    let vc = (pos_xz.y / (2.0 * half_ext) + 0.5) * n_f - 0.5;

    let ix = i32(floor(uc));
    let iy = i32(floor(vc));
    let fx = clamp(uc - f32(ix), 0.0, 1.0);
    let fy = clamp(vc - f32(iy), 0.0, 1.0);

    let c00 = sample_caustic_xyz_raw(ix,     iy,     n_i);
    let c10 = sample_caustic_xyz_raw(ix + 1, iy,     n_i);
    let c01 = sample_caustic_xyz_raw(ix,     iy + 1, n_i);
    let c11 = sample_caustic_xyz_raw(ix + 1, iy + 1, n_i);
    let bilinear = mix(mix(c00, c10, fx), mix(c01, c11, fx), fy);

    let cross_avg = 0.25 * (
        sample_caustic_xyz_raw(ix - 1, iy,     n_i) +
        sample_caustic_xyz_raw(ix + 2, iy,     n_i) +
        sample_caustic_xyz_raw(ix,     iy - 1, n_i) +
        sample_caustic_xyz_raw(ix,     iy + 2, n_i)
    );
    let sum_xyz = mix(bilinear, cross_avg, 0.25);

    let texel_size = (2.0 * half_ext) / n_f;
    let texel_area = texel_size * texel_size;
    let frames = f32(max(1u, u.dims.z + 1u));
    return sum_xyz / (frames * texel_area);
}

fn eval_nee_scalar(
    hit_pos: vec3<f32>,
    n_surf: vec3<f32>,
    light: StudioLight,
    albedo: f32,
    rng: ptr<function, u32>,
) -> f32 {
    let onb = make_onb(light.normal);
    let lr = sqrt(pcg_next(rng)) * light.radius;
    let lphi = TWO_PI * pcg_next(rng);
    let p_light = light.pos + lr * (cos(lphi) * onb[0] + sin(lphi) * onb[1]);

    let to_l = p_light - hit_pos;
    let dist = length(to_l);
    let wi = to_l / max(dist, 1e-6);

    let cos_surf = dot(wi, n_surf);
    let cos_l = dot(-wi, light.normal);
    if (cos_surf <= 0.0 || cos_l <= 0.0) {
        return 0.0;
    }

    let shadow = intersect_scene(hit_pos + EPS_OFFSET * n_surf, wi, false);
    if (shadow.surf_id != SURF_NONE && shadow.t < dist - 1e-3) {
        return 0.0;
    }

    let area = PI * light.radius * light.radius;
    let g = cos_surf * cos_l / max(dist * dist, 1e-4);
    return albedo * INV_PI * light.le_scale * g * area;
}

fn lut_filter_xyz(caustic_xyz: vec3<f32>, d_liq: f32, d_glass: f32) -> vec3<f32> {
    if (d_liq <= 1e-5 && d_glass <= 1e-5) {
        return caustic_xyz;
    }
    let white_xyz = eval_beer_lut(0.0, 0.0);
    let filt_xyz = eval_beer_lut(d_liq, d_glass);
    let w_rgb = max(gamut_map_srgb(xyz_to_lin_srgb(white_xyz)), vec3<f32>(1e-4));
    let f_rgb = gamut_map_srgb(xyz_to_lin_srgb(filt_xyz));
    let trans_rgb = clamp(f_rgb / w_rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let c_rgb = gamut_map_srgb(xyz_to_lin_srgb(caustic_xyz));
    return max(lin_srgb_to_xyz(c_rgb * trans_rgb), vec3<f32>(0.0));
}

fn hwss_filter_xyz(
    caustic_xyz: vec3<f32>,
    throughput: vec4<f32>,
    inv_pdf: vec4<f32>,
    s0: SpectralSample,
    s1: SpectralSample,
    s2: SpectralSample,
    s3: SpectralSample,
) -> vec3<f32> {
    let white_xyz = 0.25 * (
        inv_pdf.x * s0.spd * s0.cmf +
        inv_pdf.y * s1.spd * s1.cmf +
        inv_pdf.z * s2.spd * s2.cmf +
        inv_pdf.w * s3.spd * s3.cmf
    );
    let filt_xyz = 0.25 * (
        throughput.x * inv_pdf.x * s0.spd * s0.cmf +
        throughput.y * inv_pdf.y * s1.spd * s1.cmf +
        throughput.z * inv_pdf.z * s2.spd * s2.cmf +
        throughput.w * inv_pdf.w * s3.spd * s3.cmf
    );
    let w_rgb = max(gamut_map_srgb(xyz_to_lin_srgb(white_xyz)), vec3<f32>(1e-4));
    let f_rgb = gamut_map_srgb(xyz_to_lin_srgb(filt_xyz));
    let trans_rgb = clamp(f_rgb / w_rgb, vec3<f32>(0.0), vec3<f32>(8.0));
    let c_rgb = gamut_map_srgb(xyz_to_lin_srgb(caustic_xyz));
    return max(lin_srgb_to_xyz(c_rgb * trans_rgb), vec3<f32>(0.0));
}

fn eval_bench_albedo(pos: vec3<f32>) -> f32 {
    let r_bench = length(pos.xz);
    let edge_fade = 1.0 - 0.55 * smoothstep(32.0, 95.0, r_bench);
    return u.scene_params0.w * edge_fade;
}

/// Virtual camera-relative studio side softbox strips (outside camera FOV) for crisp glass edge highlights.
fn eval_strip_reflector(ro: vec3<f32>, rd: vec3<f32>, t_limit: f32) -> f32 {
    let h = u.flask1.z;
    let side_x = 1.25 * h;
    let right = u.cam_right.xyz;
    let fwd = u.cam_forward.xyz;
    let rx = dot(ro, right);
    let dx = dot(rd, right);
    if (abs(dx) < 1e-4) {
        return -1.0;
    }

    var best_t = t_limit;
    var best_val = -1.0;
    for (var s = 0u; s < 2u; s = s + 1u) {
        let plane_x = select(side_x, -side_x, s == 0u);
        let t = (plane_x - rx) / dx;
        if (t > EPS_T && t < best_t) {
            let p = ro + t * rd;
            let z_rel = dot(p, fwd);
            let y_norm = p.y / max(2.4 * h, 1.0);
            let z_norm = (z_rel - 1.0 * h) / max(1.8 * h, 1.0);
            if (y_norm >= -0.05 && y_norm <= 1.05 && abs(z_norm) <= 1.0) {
                let wy = smoothstep(0.0, 0.10, y_norm) * (1.0 - smoothstep(0.88, 1.05, y_norm));
                let wz = 1.0 - smoothstep(0.45, 1.0, abs(z_norm));
                best_t = t;
                best_val = 0.90 * wy * wz;
            }
        }
    }
    return best_val;
}

/// Deterministic reflection probe for outer glass surface at depth 0 (non-dispersive LUT mode).
fn eval_outer_reflection_probe_lut(
    ro_in: vec3<f32>,
    rd_in: vec3<f32>,
    key_light: StudioLight,
    fill_light: StudioLight,
    rng: ptr<function, u32>,
) -> vec3<f32> {
    let hit = intersect_scene(ro_in, rd_in, true);
    let white_lut = eval_beer_lut(0.0, 0.0);

    if (hit.surf_id == SURF_KEY_LIGHT) {
        return key_light.le_scale * white_lut;
    }
    if (hit.surf_id == SURF_FILL_LIGHT) {
        return fill_light.le_scale * white_lut;
    }
    let strip_le = eval_strip_reflector(ro_in, rd_in, hit.t);
    if (strip_le > 0.0) {
        return strip_le * white_lut;
    }
    if (hit.surf_id == SURF_NONE) {
        return eval_env_spd(rd_in) * white_lut;
    }
    if (hit.surf_id == SURF_BENCH || hit.surf_id == SURF_CARD) {
        var n_surf = hit.normal;
        if (dot(rd_in, n_surf) > 0.0) {
            n_surf = -n_surf;
        }
        let albedo = select(0.90, eval_bench_albedo(hit.pos), hit.surf_id == SURF_BENCH);
        var xyz = vec3<f32>(0.0);
        if (hit.surf_id == SURF_BENCH) {
            let c_irr = lookup_caustic_irradiance_xyz(hit.pos.xz);
            if (c_irr.x + c_irr.y + c_irr.z > 0.0) {
                xyz += albedo * INV_PI * c_irr;
            }
        }
        let nee_k = eval_nee_scalar(hit.pos, n_surf, key_light, albedo, rng);
        let nee_f = eval_nee_scalar(hit.pos, n_surf, fill_light, albedo, rng);
        xyz += (nee_k + nee_f) * white_lut;

        // 1 indirect cosine bounce for bench/card accuracy
        let rd2 = sample_cosine_hemisphere(n_surf, rng);
        let hit2 = intersect_scene(hit.pos + EPS_OFFSET * n_surf, rd2, false);
        if (hit2.surf_id == SURF_NONE) {
            xyz += albedo * eval_env_spd(rd2) * white_lut;
        } else if (hit2.surf_id == SURF_BENCH || hit2.surf_id == SURF_CARD) {
            var n2 = hit2.normal;
            if (dot(rd2, n2) > 0.0) {
                n2 = -n2;
            }
            let alb2 = select(0.90, eval_bench_albedo(hit2.pos), hit2.surf_id == SURF_BENCH);
            let nee2 = eval_nee_scalar(hit2.pos, n2, key_light, alb2, rng) + eval_nee_scalar(hit2.pos, n2, fill_light, alb2, rng);
            xyz += albedo * nee2 * white_lut;
        }
        return xyz;
    }
    // Grazing self-reflection into neck/shoulder fallback
    return eval_env_spd(rd_in) * white_lut;
}

/// Deterministic reflection probe for outer glass surface at depth 0 (dispersive spectral mode).
fn eval_outer_reflection_probe_spec(
    ro_in: vec3<f32>,
    rd_in: vec3<f32>,
    fr_vec: vec4<f32>,
    spd_vec: vec4<f32>,
    inv_pdf: vec4<f32>,
    s0: SpectralSample,
    s1: SpectralSample,
    s2: SpectralSample,
    s3: SpectralSample,
    key_light: StudioLight,
    fill_light: StudioLight,
    rng: ptr<function, u32>,
) -> vec3<f32> {
    let hit = intersect_scene(ro_in, rd_in, true);
    var rad_spec = vec4<f32>(0.0);
    var extra = vec3<f32>(0.0);

    if (hit.surf_id == SURF_KEY_LIGHT) {
        rad_spec = fr_vec * key_light.le_scale * spd_vec;
    } else if (hit.surf_id == SURF_FILL_LIGHT) {
        rad_spec = fr_vec * fill_light.le_scale * spd_vec;
    } else {
        let strip_le = eval_strip_reflector(ro_in, rd_in, hit.t);
        if (strip_le > 0.0) {
            rad_spec = fr_vec * strip_le * spd_vec;
        } else if (hit.surf_id == SURF_NONE) {
            rad_spec = fr_vec * eval_env_spd(rd_in) * spd_vec;
        } else if (hit.surf_id == SURF_BENCH || hit.surf_id == SURF_CARD) {
        var n_surf = hit.normal;
        if (dot(rd_in, n_surf) > 0.0) {
            n_surf = -n_surf;
        }
        let albedo = select(0.90, eval_bench_albedo(hit.pos), hit.surf_id == SURF_BENCH);
        if (hit.surf_id == SURF_BENCH) {
            let c_irr = lookup_caustic_irradiance_xyz(hit.pos.xz);
            if (c_irr.x + c_irr.y + c_irr.z > 0.0) {
                extra += hwss_filter_xyz(albedo * INV_PI * c_irr, fr_vec, inv_pdf, s0, s1, s2, s3);
            }
        }
        let nee_s = eval_nee_scalar(hit.pos, n_surf, key_light, albedo, rng) + eval_nee_scalar(hit.pos, n_surf, fill_light, albedo, rng);
        rad_spec = fr_vec * nee_s * spd_vec;

        let rd2 = sample_cosine_hemisphere(n_surf, rng);
        let hit2 = intersect_scene(hit.pos + EPS_OFFSET * n_surf, rd2, false);
        if (hit2.surf_id == SURF_NONE) {
            rad_spec += fr_vec * albedo * eval_env_spd(rd2) * spd_vec;
        }
        } else {
            rad_spec = fr_vec * eval_env_spd(rd_in) * spd_vec;
        }
    }

    return extra + 0.25 * (
        rad_spec.x * inv_pdf.x * s0.cmf +
        rad_spec.y * inv_pdf.y * s1.cmf +
        rad_spec.z * inv_pdf.z * s2.cmf +
        rad_spec.w * inv_pdf.w * s3.cmf
    );
}

fn trace_sample_lut(
    ro_init: vec3<f32>,
    rd_init: vec3<f32>,
    key_light: StudioLight,
    fill_light: StudioLight,
    rng: ptr<function, u32>,
) -> vec3<f32> {
    var ro = ro_init;
    var rd = rd_init;
    var path_weight = 1.0;
    var d_liq = 0.0;
    var d_glass = 0.0;
    var xyz_out = vec3<f32>(0.0);
    var med = MED_AIR;
    var last_was_diffuse = false;
    var hit_diffuse_already = false;

    for (var depth = 0u; depth < 16u; depth = depth + 1u) {
        let hit = intersect_scene(ro, rd, true);

        if (hit.surf_id == SURF_NONE) {
            xyz_out += path_weight * eval_env_spd(rd) * eval_beer_lut(d_liq, d_glass);
            break;
        }

        if (med == MED_LIQUID) {
            d_liq += hit.t;
        } else if (med == MED_GLASS) {
            d_glass += hit.t;
        }

        if (path_weight < 1e-5) {
            break;
        }

        if (hit.surf_id == SURF_KEY_LIGHT) {
            let suppress = last_was_diffuse || (hit_diffuse_already && u.render_flags.x != 0u);
            if (!suppress) {
                xyz_out += path_weight * key_light.le_scale * eval_beer_lut(d_liq, d_glass);
            }
            break;
        }

        if (hit.surf_id == SURF_FILL_LIGHT) {
            if (!last_was_diffuse) {
                xyz_out += path_weight * fill_light.le_scale * eval_beer_lut(d_liq, d_glass);
            }
            break;
        }

        var is_enamel = false;
        if (hit.surf_id == SURF_OUTER) {
            let em = eval_enamel_mask(hit.pos);
            if (em > 0.01) {
                is_enamel = pcg_next(rng) < em;
            }
        }
        let is_ptfe = (hit.surf_id == SURF_STOPPER) && (u.flags.y == 2u);
        let is_diffuse = (hit.surf_id == SURF_BENCH) || (hit.surf_id == SURF_CARD) || is_enamel || is_ptfe;

        if (is_diffuse) {
            var n_surf = hit.normal;
            if (dot(rd, n_surf) > 0.0) {
                n_surf = -n_surf;
            }

            var albedo = 0.58;
            if (hit.surf_id == SURF_BENCH) {
                albedo = eval_bench_albedo(hit.pos);
                let c_irr = lookup_caustic_irradiance_xyz(hit.pos.xz);
                if (c_irr.x + c_irr.y + c_irr.z > 0.0) {
                    let c_raw = albedo * INV_PI * c_irr;
                    xyz_out += path_weight * lut_filter_xyz(c_raw, d_liq, d_glass);
                }
            } else if (hit.surf_id == SURF_CARD) {
                albedo = 0.90;
            } else if (is_enamel) {
                albedo = 0.92;
            } else if (is_ptfe) {
                albedo = 0.88;
            }

            let nee_s = eval_nee_scalar(hit.pos, n_surf, key_light, albedo, rng) + eval_nee_scalar(hit.pos, n_surf, fill_light, albedo, rng);
            if (nee_s > 0.0) {
                xyz_out += path_weight * nee_s * eval_beer_lut(d_liq, d_glass);
            }

            path_weight *= albedo;
            last_was_diffuse = true;
            hit_diffuse_already = true;

            if (depth >= 3u) {
                let p_surv = clamp(path_weight, 0.05, 0.95);
                if (pcg_next(rng) > p_surv) {
                    break;
                }
                path_weight /= p_surv;
            }

            rd = sample_cosine_hemisphere(n_surf, rng);
            ro = hit.pos + EPS_OFFSET * n_surf;
            if (is_enamel && dot(rd, hit.normal) < 0.0) {
                med = MED_GLASS;
            } else {
                med = MED_AIR;
            }
            continue;
        }

        // Dielectric boundary at reference D-line 589.3 nm (exact non-dispersive mode)
        last_was_diffuse = false;
        let bnd = resolve_dielectric_boundary(hit, rd);
        var n_facing = bnd.n_facing;

        if (hit.surf_id == SURF_STOPPER && u.flags.y == 1u && hit.pos.y < u.flask1.z - 0.02) {
            let pert = sample_cosine_hemisphere(n_facing, rng);
            let cand = normalize(mix(n_facing, pert, 0.14));
            if (dot(rd, cand) < -1e-4) {
                n_facing = cand;
            }
        }

        let n1 = medium_ior(bnd.med_i, 589.3);
        let n2 = medium_ior(bnd.med_t, 589.3);
        let cos_i = -dot(rd, n_facing);
        let fr = fresnel_dielectric(cos_i, n1, n2);

        // Deterministic reflection/refraction split at depth 0 on outer glass wall (zero RR grain!)
        if (depth == 0u && hit.surf_id == SURF_OUTER && bnd.med_i == MED_AIR) {
            let refr0 = refract_exact(rd, n_facing, n1 / n2);
            if (refr0.w > 0.5 && fr < 0.999) {
                xyz_out += path_weight * fr * eval_outer_reflection_probe_lut(
                    hit.pos + EPS_OFFSET * n_facing,
                    reflect(rd, n_facing),
                    key_light,
                    fill_light,
                    rng,
                );
                path_weight *= (1.0 - fr);
                rd = refr0.xyz;
                ro = hit.pos - EPS_OFFSET * n_facing;
                med = bnd.med_t;
                continue;
            }
        }

        if (pcg_next(rng) < fr) {
            rd = reflect(rd, n_facing);
            ro = hit.pos + EPS_OFFSET * n_facing;
            med = bnd.med_i;
        } else {
            let refr = refract_exact(rd, n_facing, n1 / n2);
            if (refr.w > 0.5) {
                rd = refr.xyz;
                ro = hit.pos - EPS_OFFSET * n_facing;
                med = bnd.med_t;
            } else {
                rd = refr.xyz;
                ro = hit.pos + EPS_OFFSET * n_facing;
                med = bnd.med_i;
            }
        }
    }

    return xyz_out;
}

fn trace_sample_dispersive(
    ro_init: vec3<f32>,
    rd_init: vec3<f32>,
    u_wl: f32,
    key_light: StudioLight,
    fill_light: StudioLight,
    rng: ptr<function, u32>,
) -> vec3<f32> {
    let w0 = sample_importance_lambda(fract(u_wl + 0.00));
    let w1 = sample_importance_lambda(fract(u_wl + 0.25));
    let w2 = sample_importance_lambda(fract(u_wl + 0.50));
    let w3 = sample_importance_lambda(fract(u_wl + 0.75));
    let lambda = vec4<f32>(w0.x, w1.x, w2.x, w3.x);
    let inv_pdf = vec4<f32>(w0.y, w1.y, w2.y, w3.y);

    let s0 = eval_spectral(lambda.x);
    let s1 = eval_spectral(lambda.y);
    let s2 = eval_spectral(lambda.z);
    let s3 = eval_spectral(lambda.w);
    let spd_vec = vec4<f32>(s0.spd, s1.spd, s2.spd, s3.spd);
    let sigma_liq = vec4<f32>(s0.sigma_liquid, s1.sigma_liquid, s2.sigma_liquid, s3.sigma_liquid);
    let sigma_glass = vec4<f32>(s0.sigma_glass, s1.sigma_glass, s2.sigma_glass, s3.sigma_glass);

    var ro = ro_init;
    var rd = rd_init;
    var throughput = vec4<f32>(1.0);
    var rad_spec = vec4<f32>(0.0);
    var extra_xyz = vec3<f32>(0.0);
    var med = MED_AIR;
    var dispersed = false;
    var last_was_diffuse = false;
    var hit_diffuse_already = false;

    for (var depth = 0u; depth < 16u; depth = depth + 1u) {
        let hit = intersect_scene(ro, rd, true);

        if (hit.surf_id == SURF_NONE) {
            var contrib = throughput * (eval_env_spd(rd) * spd_vec);
            if (depth >= 2u && hit_diffuse_already) {
                contrib = min(contrib, vec4<f32>(40.0));
            }
            rad_spec += contrib;
            break;
        }

        if (med == MED_LIQUID) {
            throughput *= exp(-sigma_liq * hit.t);
        } else if (med == MED_GLASS) {
            throughput *= exp(-sigma_glass * hit.t);
        }

        if (max(max(throughput.x, throughput.y), max(throughput.z, throughput.w)) < 1e-5) {
            break;
        }

        if (hit.surf_id == SURF_KEY_LIGHT) {
            let suppress = last_was_diffuse || (hit_diffuse_already && u.render_flags.x != 0u);
            if (!suppress) {
                var contrib = throughput * (key_light.le_scale * spd_vec);
                if (depth >= 2u && hit_diffuse_already) {
                    contrib = min(contrib, vec4<f32>(40.0));
                }
                rad_spec += contrib;
            }
            break;
        }

        if (hit.surf_id == SURF_FILL_LIGHT) {
            if (!last_was_diffuse) {
                var contrib = throughput * (fill_light.le_scale * spd_vec);
                if (depth >= 2u && hit_diffuse_already) {
                    contrib = min(contrib, vec4<f32>(40.0));
                }
                rad_spec += contrib;
            }
            break;
        }

        var is_enamel = false;
        if (hit.surf_id == SURF_OUTER) {
            let em = eval_enamel_mask(hit.pos);
            if (em > 0.01) {
                is_enamel = pcg_next(rng) < em;
            }
        }
        let is_ptfe = (hit.surf_id == SURF_STOPPER) && (u.flags.y == 2u);
        let is_diffuse = (hit.surf_id == SURF_BENCH) || (hit.surf_id == SURF_CARD) || is_enamel || is_ptfe;

        if (is_diffuse) {
            var n_surf = hit.normal;
            if (dot(rd, n_surf) > 0.0) {
                n_surf = -n_surf;
            }

            var albedo = 0.58;
            if (hit.surf_id == SURF_BENCH) {
                albedo = eval_bench_albedo(hit.pos);
                let c_irr = lookup_caustic_irradiance_xyz(hit.pos.xz);
                if (c_irr.x + c_irr.y + c_irr.z > 0.0) {
                    let c_raw = albedo * INV_PI * c_irr;
                    if (depth == 0u) {
                        extra_xyz += c_raw;
                    } else {
                        extra_xyz += hwss_filter_xyz(c_raw, throughput, inv_pdf, s0, s1, s2, s3);
                    }
                }
            } else if (hit.surf_id == SURF_CARD) {
                albedo = 0.90;
            } else if (is_enamel) {
                albedo = 0.92;
            } else if (is_ptfe) {
                albedo = 0.88;
            }

            let nee_s = eval_nee_scalar(hit.pos, n_surf, key_light, albedo, rng) + eval_nee_scalar(hit.pos, n_surf, fill_light, albedo, rng);
            var nee_contrib = throughput * (nee_s * spd_vec);
            if (depth >= 2u && hit_diffuse_already) {
                nee_contrib = min(nee_contrib, vec4<f32>(40.0));
            }
            rad_spec += nee_contrib;

            throughput *= albedo;
            last_was_diffuse = true;
            hit_diffuse_already = true;

            if (depth >= 3u) {
                let p_surv = clamp(max(max(throughput.x, throughput.y), max(throughput.z, throughput.w)), 0.05, 0.95);
                if (pcg_next(rng) > p_surv) {
                    break;
                }
                throughput /= p_surv;
            }

            rd = sample_cosine_hemisphere(n_surf, rng);
            ro = hit.pos + EPS_OFFSET * n_surf;
            if (is_enamel && dot(rd, hit.normal) < 0.0) {
                med = MED_GLASS;
            } else {
                med = MED_AIR;
            }
            continue;
        }

        last_was_diffuse = false;
        let bnd = resolve_dielectric_boundary(hit, rd);
        var n_facing = bnd.n_facing;

        if (hit.surf_id == SURF_STOPPER && u.flags.y == 1u && hit.pos.y < u.flask1.z - 0.02) {
            let pert = sample_cosine_hemisphere(n_facing, rng);
            let cand = normalize(mix(n_facing, pert, 0.14));
            if (dot(rd, cand) < -1e-4) {
                n_facing = cand;
            }
        }

        let n1_vec = vec4<f32>(
            medium_ior(bnd.med_i, lambda.x),
            medium_ior(bnd.med_i, lambda.y),
            medium_ior(bnd.med_i, lambda.z),
            medium_ior(bnd.med_i, lambda.w),
        );
        let n2_vec = vec4<f32>(
            medium_ior(bnd.med_t, lambda.x),
            medium_ior(bnd.med_t, lambda.y),
            medium_ior(bnd.med_t, lambda.z),
            medium_ior(bnd.med_t, lambda.w),
        );

        let cos_i = -dot(rd, n_facing);
        let fr0 = fresnel_dielectric(cos_i, n1_vec.x, n2_vec.x);
        let fr_vec = vec4<f32>(
            fr0,
            fresnel_dielectric(cos_i, n1_vec.y, n2_vec.y),
            fresnel_dielectric(cos_i, n1_vec.z, n2_vec.z),
            fresnel_dielectric(cos_i, n1_vec.w, n2_vec.w),
        );

        // Deterministic reflection/refraction split at depth 0 on outer glass wall
        if (depth == 0u && hit.surf_id == SURF_OUTER && bnd.med_i == MED_AIR) {
            let refr0 = refract_exact(rd, n_facing, n1_vec.x / n2_vec.x);
            if (refr0.w > 0.5 && fr0 < 0.999) {
                extra_xyz += eval_outer_reflection_probe_spec(
                    hit.pos + EPS_OFFSET * n_facing,
                    reflect(rd, n_facing),
                    throughput * fr_vec,
                    spd_vec,
                    inv_pdf,
                    s0, s1, s2, s3,
                    key_light,
                    fill_light,
                    rng,
                );
                throughput = vec4<f32>(throughput.x * 4.0 * (1.0 - fr0), 0.0, 0.0, 0.0);
                dispersed = true;
                rd = refr0.xyz;
                ro = hit.pos - EPS_OFFSET * n_facing;
                med = bnd.med_t;
                continue;
            }
        }

        if (pcg_next(rng) < fr0) {
            throughput *= fr_vec / max(fr0, 1e-6);
            rd = reflect(rd, n_facing);
            ro = hit.pos + EPS_OFFSET * n_facing;
            med = bnd.med_i;
        } else {
            let refr = refract_exact(rd, n_facing, n1_vec.x / n2_vec.x);
            if (refr.w > 0.5) {
                if (!dispersed && abs(n1_vec.x - n2_vec.x) > 1e-5) {
                    throughput = vec4<f32>(throughput.x * 4.0, 0.0, 0.0, 0.0);
                    dispersed = true;
                } else {
                    throughput *= (1.0 - fr_vec) / max(1.0 - fr0, 1e-6);
                }
                rd = refr.xyz;
                ro = hit.pos - EPS_OFFSET * n_facing;
                med = bnd.med_t;
            } else {
                throughput *= fr_vec / max(fr0, 1e-6);
                rd = refr.xyz;
                ro = hit.pos + EPS_OFFSET * n_facing;
                med = bnd.med_i;
            }
        }
    }

    return extra_xyz + 0.25 * (
        rad_spec.x * inv_pdf.x * s0.cmf +
        rad_spec.y * inv_pdf.y * s1.cmf +
        rad_spec.z * inv_pdf.z * s2.cmf +
        rad_spec.w * inv_pdf.w * s3.cmf
    );
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u.dims.x;
    let h = u.dims.y;
    if (gid.x >= w || gid.y >= h) {
        return;
    }

    let idx = gid.y * w + gid.x;
    let prev = accum_xyz[idx];
    let base_sample_idx = u32(prev.w);

    var rng = hash_u32((gid.x + gid.y * 65537u) ^ (u.dims.z * 747796405u + 982451653u));
    let pixel_wl_seed = f32(hash_u32(gid.x * 1973u ^ gid.y * 9277u ^ 26699u)) * (1.0 / 4294967296.0);
    let spp = max(1u, u.dims.w);
    let key_light = get_key_light();
    let fill_light = get_fill_light();

    var xyz_sum = vec3<f32>(0.0);

    for (var s = 0u; s < spp; s = s + 1u) {
        let sample_n = base_sample_idx + s;
        let px = f32(gid.x) + pcg_next(&rng);
        let py = f32(gid.y) + pcg_next(&rng);
        let ndc_x = (2.0 * px / f32(w) - 1.0) * u.cam_right.w;
        let ndc_y = 1.0 - 2.0 * py / f32(h);

        let fov_tan = u.cam_pos.w;
        let ro = u.cam_pos.xyz;
        let rd = normalize(
            u.cam_forward.xyz +
            ndc_x * fov_tan * u.cam_right.xyz +
            ndc_y * fov_tan * u.cam_up.xyz
        );

        var sample_xyz = vec3<f32>(0.0);
        if (u.flags.w == 0u) {
            sample_xyz = trace_sample_lut(ro, rd, key_light, fill_light, &rng);
        } else {
            let u_wl = fract(pixel_wl_seed + f32(sample_n) * 0.61803398875);
            sample_xyz = trace_sample_dispersive(ro, rd, u_wl, key_light, fill_light, &rng);
        }

        if (all(sample_xyz >= vec3<f32>(-1.0)) && all(sample_xyz < vec3<f32>(1e6))) {
            xyz_sum += max(sample_xyz, vec3<f32>(0.0));
        }
    }

    accum_xyz[idx] = vec4<f32>(prev.xyz + xyz_sum, prev.w + f32(spp));
}

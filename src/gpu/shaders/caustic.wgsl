@group(1) @binding(0) var<storage, read_write> caustic_splat: array<atomic<u32>>;

fn add_splat_texel(ix: i32, iy: i32, n_i: i32, xyz: vec3<f32>, w: f32, rng: ptr<function, u32>) {
    if (ix < 0 || ix >= n_i || iy < 0 || iy >= n_i) {
        return;
    }
    let scaled = clamp(xyz * w * 65536.0, vec3<f32>(0.0), vec3<f32>(1e7));
    let qx = u32(scaled.x + pcg_next(rng));
    let qy = u32(scaled.y + pcg_next(rng));
    let qz = u32(scaled.z + pcg_next(rng));
    let base_idx = u32(iy * n_i + ix) * 3u;
    if (qx > 0u) { atomicAdd(&caustic_splat[base_idx + 0u], qx); }
    if (qy > 0u) { atomicAdd(&caustic_splat[base_idx + 1u], qy); }
    if (qz > 0u) { atomicAdd(&caustic_splat[base_idx + 2u], qz); }
}

fn splat_caustic(pos_xz: vec2<f32>, xyz: vec3<f32>, rng: ptr<function, u32>) {
    let half_ext = max(u.scene_params1.y, 1.0);
    let n_i = i32(u.render_flags2.y);
    let n_f = f32(n_i);
    let uc = (pos_xz.x / (2.0 * half_ext) + 0.5) * n_f - 0.5;
    let vc = (pos_xz.y / (2.0 * half_ext) + 0.5) * n_f - 0.5;
    let ix0 = i32(floor(uc));
    let iy0 = i32(floor(vc));
    let fx = clamp(uc - f32(ix0), 0.0, 1.0);
    let fy = clamp(vc - f32(iy0), 0.0, 1.0);

    add_splat_texel(ix0,      iy0,      n_i, xyz, (1.0 - fx) * (1.0 - fy), rng);
    add_splat_texel(ix0 + 1,  iy0,      n_i, xyz, fx         * (1.0 - fy), rng);
    add_splat_texel(ix0,      iy0 + 1,  n_i, xyz, (1.0 - fx) * fy,         rng);
    add_splat_texel(ix0 + 1,  iy0 + 1,  n_i, xyz, fx         * fy,         rng);
}

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let photon_idx = gid.x;
    let total_frame_photons = max(1u, u.render_flags2.z);
    if (photon_idx >= total_frame_photons || u.render_flags.x == 0u) {
        return;
    }

    var rng = hash_u32(photon_idx ^ (u.dims.z * 747796405u + 2891336453u));
    let u_wl = fract(pcg_next(&rng) + f32(u.dims.z) * 0.61803398875);
    let wl_sample = sample_importance_lambda(u_wl);
    let lambda_nm = wl_sample.x;
    let inv_pdf = wl_sample.y;
    let spec = eval_spectral(lambda_nm);

    let key = get_key_light();
    let l_onb = make_onb(key.normal);
    let lr = sqrt(pcg_next(&rng)) * key.radius;
    let lphi = TWO_PI * pcg_next(&rng);
    let p_light = key.pos + lr * (cos(lphi) * l_onb[0] + sin(lphi) * l_onb[1]);

    // Aim photon uniformly into tight cone bounding the flask
    let flask_center = vec3<f32>(0.0, 0.48 * u.flask3.w, 0.0);
    let bound_r = length(vec3<f32>(u.flask3.x, 0.52 * u.flask3.w, 0.0)) + 0.35;
    let to_center = flask_center - p_light;
    let dist_c = max(length(to_center), bound_r + 0.1);
    let dir_c = to_center / dist_c;
    let sin_max = clamp(bound_r / dist_c, 0.01, 0.99);
    let cos_max = sqrt(max(0.0, 1.0 - sin_max * sin_max));
    let omega_cone = TWO_PI * (1.0 - cos_max);

    let cos_th = mix(cos_max, 1.0, pcg_next(&rng));
    let sin_th = sqrt(max(0.0, 1.0 - cos_th * cos_th));
    let phi_d = TWO_PI * pcg_next(&rng);
    let c_onb = make_onb(dir_c);
    var rd = normalize(c_onb * vec3<f32>(sin_th * cos(phi_d), sin_th * sin(phi_d), cos_th));

    let cos_light = dot(rd, key.normal);
    if (cos_light <= 0.0) {
        return;
    }

    let a_light = PI * key.radius * key.radius;
    let flux_base = inv_pdf * (key.le_scale * spec.spd) * a_light * omega_cone * cos_light / f32(total_frame_photons);

    var ro = p_light + 0.01 * rd;
    var med = MED_AIR;
    var throughput = 1.0;
    var had_specular = false;

    for (var bounce = 0u; bounce < 14u; bounce = bounce + 1u) {
        let hit = intersect_scene(ro, rd, false);
        if (hit.surf_id == SURF_NONE) {
            break;
        }

        if (med == MED_LIQUID) {
            throughput *= exp(-spec.sigma_liquid * hit.t);
        } else if (med == MED_GLASS) {
            throughput *= exp(-spec.sigma_glass * hit.t);
        }

        if (throughput < 1e-4) {
            break;
        }

        if (hit.surf_id == SURF_BENCH) {
            if (had_specular) {
                let xyz = flux_base * throughput * spec.cmf;
                splat_caustic(hit.pos.xz, xyz, &rng);
            }
            break;
        }

        if (hit.surf_id == SURF_CARD) {
            break;
        }

        if (hit.surf_id == SURF_OUTER && eval_enamel_mask(hit.pos) > 0.45) {
            break;
        }

        if (hit.surf_id == SURF_STOPPER && u.flags.y == 2u) {
            break;
        }

        let bnd = resolve_dielectric_boundary(hit, rd);
        var n_facing = bnd.n_facing;

        // Subtle frosted ground-glass neck plug
        if (hit.surf_id == SURF_STOPPER && u.flags.y == 1u && hit.pos.y < u.flask1.z - 0.02) {
            let pert = sample_cosine_hemisphere(n_facing, &rng);
            let cand = normalize(mix(n_facing, pert, 0.14));
            if (dot(rd, cand) < -1e-4) {
                n_facing = cand;
            }
        }

        let n1 = medium_ior(bnd.med_i, lambda_nm);
        let n2 = medium_ior(bnd.med_t, lambda_nm);
        let cos_i = -dot(rd, n_facing);
        let fr = fresnel_dielectric(cos_i, n1, n2);

        had_specular = true;

        if (pcg_next(&rng) < fr) {
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
}

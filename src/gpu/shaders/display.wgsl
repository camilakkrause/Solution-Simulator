@group(1) @binding(0) var<storage, read> accum_xyz: array<vec4<f32>>;
@group(1) @binding(1) var display_tex: texture_storage_2d<rgba8unorm, write>;

fn apply_tone_mapper(rgb: vec3<f32>, mapper: u32) -> vec3<f32> {
    if (mapper == 1u) {
        return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    } else if (mapper == 2u) {
        return clamp((rgb * (2.51 * rgb + 0.03)) / (rgb * (2.43 * rgb + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
    } else {
        // Khronos PBR Neutral tone mapper
        let start_compression = 0.8 - 0.04;
        let desaturation = 0.15;
        let x = min(rgb.x, min(rgb.y, rgb.z));
        let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
        var color = rgb - vec3<f32>(offset);
        let peak = max(color.x, max(color.y, color.z));
        if (peak < start_compression) {
            return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        let d = 1.0 - start_compression;
        let new_peak = 1.0 - d * d / (peak + d - start_compression);
        color *= new_peak / max(peak, 1e-6);
        let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
        return clamp(mix(color, vec3<f32>(new_peak), g), vec3<f32>(0.0), vec3<f32>(1.0));
    }
}

fn lin_to_srgb_channel(c_in: f32) -> f32 {
    let c = clamp(c_in, 0.0, 1.0);
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, 12.92 * c, c <= 0.0031308);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u.dims.x;
    let h = u.dims.y;
    if (gid.x >= w || gid.y >= h) {
        return;
    }
    let idx = gid.y * w + gid.x;
    let acc = accum_xyz[idx];
    var xyz = (acc.xyz / max(acc.w, 1.0)) * u.scene_params1.x;

    if (u.render_flags2.x != 0u) {
        xyz = vec3<f32>(
            dot(u.bradford_row0.xyz, xyz),
            dot(u.bradford_row1.xyz, xyz),
            dot(u.bradford_row2.xyz, xyz),
        );
    }

    let lin_rgb = gamut_map_srgb(xyz_to_lin_srgb(xyz));
    let mapped = apply_tone_mapper(lin_rgb, u.render_flags.w);
    let srgb = vec3<f32>(
        lin_to_srgb_channel(mapped.x),
        lin_to_srgb_channel(mapped.y),
        lin_to_srgb_channel(mapped.z),
    );
    textureStore(display_tex, vec2<i32>(gid.xy), vec4<f32>(srgb, 1.0));
}

@group(1) @binding(0) var<storage, read_write> caustic_splat: array<atomic<u32>>;
@group(1) @binding(1) var<storage, read_write> caustic_accum: array<vec4<f32>>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = u.render_flags2.y;
    if (gid.x >= n || gid.y >= n) {
        return;
    }
    let idx = gid.y * n + gid.x;
    let base_idx = idx * 3u;

    let sx = atomicExchange(&caustic_splat[base_idx + 0u], 0u);
    let sy = atomicExchange(&caustic_splat[base_idx + 1u], 0u);
    let sz = atomicExchange(&caustic_splat[base_idx + 2u], 0u);

    let inv_scale = 1.0 / 65536.0;
    let delta = vec3<f32>(f32(sx), f32(sy), f32(sz)) * inv_scale;
    caustic_accum[idx] = caustic_accum[idx] + vec4<f32>(delta, 0.0);
}

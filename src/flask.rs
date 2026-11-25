use std::f32::consts::PI;

use bytemuck::{Pod, Zeroable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlaskVolume {
    Ml10,
    Ml25,
    Ml50,
    Ml100,
    Ml250,
    Ml500,
    Ml1000,
}

impl FlaskVolume {
    pub const ALL: [FlaskVolume; 7] = [
        FlaskVolume::Ml10,
        FlaskVolume::Ml25,
        FlaskVolume::Ml50,
        FlaskVolume::Ml100,
        FlaskVolume::Ml250,
        FlaskVolume::Ml500,
        FlaskVolume::Ml1000,
    ];

    pub fn ml(self) -> f32 {
        match self {
            FlaskVolume::Ml10 => 10.0,
            FlaskVolume::Ml25 => 25.0,
            FlaskVolume::Ml50 => 50.0,
            FlaskVolume::Ml100 => 100.0,
            FlaskVolume::Ml250 => 250.0,
            FlaskVolume::Ml500 => 500.0,
            FlaskVolume::Ml1000 => 1000.0,
        }
    }

    pub fn tolerance_ml(self) -> f32 {
        match self {
            FlaskVolume::Ml10 => 0.025,
            FlaskVolume::Ml25 => 0.04,
            FlaskVolume::Ml50 => 0.06,
            FlaskVolume::Ml100 => 0.10,
            FlaskVolume::Ml250 => 0.15,
            FlaskVolume::Ml500 => 0.25,
            FlaskVolume::Ml1000 => 0.40,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FlaskVolume::Ml10 => "10 mL",
            FlaskVolume::Ml25 => "25 mL",
            FlaskVolume::Ml50 => "50 mL",
            FlaskVolume::Ml100 => "100 mL",
            FlaskVolume::Ml250 => "250 mL",
            FlaskVolume::Ml500 => "500 mL",
            FlaskVolume::Ml1000 => "1000 mL",
        }
    }

    pub fn from_ml_int(ml: u32) -> Option<Self> {
        match ml {
            10 => Some(FlaskVolume::Ml10),
            25 => Some(FlaskVolume::Ml25),
            50 => Some(FlaskVolume::Ml50),
            100 => Some(FlaskVolume::Ml100),
            250 => Some(FlaskVolume::Ml250),
            500 => Some(FlaskVolume::Ml500),
            1000 => Some(FlaskVolume::Ml1000),
            _ => None,
        }
    }

    /// (total_h, r_bulb, r_neck, r_base, wall_t, base_t) in cm.
    pub fn dimensions_cm(self) -> (f32, f32, f32, f32, f32, f32) {
        match self {
            FlaskVolume::Ml10 => (9.0, 1.45, 0.45, 0.95, 0.11, 0.18),
            FlaskVolume::Ml25 => (11.5, 2.00, 0.55, 1.30, 0.12, 0.20),
            FlaskVolume::Ml50 => (14.5, 2.50, 0.65, 1.65, 0.13, 0.22),
            FlaskVolume::Ml100 => (17.5, 3.12, 0.72, 2.05, 0.15, 0.25),
            FlaskVolume::Ml250 => (22.5, 4.15, 0.85, 2.70, 0.17, 0.28),
            FlaskVolume::Ml500 => (26.5, 5.15, 1.00, 3.35, 0.19, 0.32),
            FlaskVolume::Ml1000 => (30.5, 6.40, 1.15, 4.10, 0.22, 0.36),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum StopperType {
    None = 0,
    Glass = 1,
    Ptfe = 2,
}

impl StopperType {
    pub const ALL: [StopperType; 3] = [StopperType::None, StopperType::Glass, StopperType::Ptfe];

    pub fn label(self) -> &'static str {
        match self {
            StopperType::None => "None (Open Rim)",
            StopperType::Glass => "Pennyhead Glass Stopper",
            StopperType::Ptfe => "PTFE / Polymer Stopper",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolventPreset {
    Water,
    Ethanol,
    Methanol,
    Acetone,
    Dmso,
    Chloroform,
    Toluene,
    Custom,
}

impl SolventPreset {
    pub const ALL: [SolventPreset; 8] = [
        SolventPreset::Water,
        SolventPreset::Ethanol,
        SolventPreset::Methanol,
        SolventPreset::Acetone,
        SolventPreset::Dmso,
        SolventPreset::Chloroform,
        SolventPreset::Toluene,
        SolventPreset::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SolventPreset::Water => "Water (n_D = 1.3330)",
            SolventPreset::Ethanol => "Ethanol (n_D = 1.3614)",
            SolventPreset::Methanol => "Methanol (n_D = 1.3284)",
            SolventPreset::Acetone => "Acetone (n_D = 1.3586)",
            SolventPreset::Dmso => "DMSO (n_D = 1.4793)",
            SolventPreset::Chloroform => "Chloroform (n_D = 1.4459)",
            SolventPreset::Toluene => "Toluene (n_D = 1.4969)",
            SolventPreset::Custom => "Custom n_D",
        }
    }

    /// Cauchy coefficients (A, B) with lambda in micrometers: n(lambda) = A + B / lambda_um^2.
    pub fn cauchy_ab(self, custom_nd: f32) -> (f32, f32) {
        match self {
            SolventPreset::Water => (1.3242, 0.00305),
            SolventPreset::Ethanol => (1.3526, 0.00306),
            SolventPreset::Methanol => (1.3204, 0.00278),
            SolventPreset::Acetone => (1.3492, 0.00326),
            SolventPreset::Dmso => (1.4660, 0.00462),
            SolventPreset::Chloroform => (1.4342, 0.00406),
            SolventPreset::Toluene => (1.4770, 0.00691),
            SolventPreset::Custom => {
                let b = 0.0035;
                let lambda_d_um = 0.5893;
                (custom_nd - b / (lambda_d_um * lambda_d_um), b)
            }
        }
    }

    pub fn capillary_length_cm(self) -> f32 {
        match self {
            SolventPreset::Water => 0.27,
            SolventPreset::Dmso => 0.20,
            _ => 0.17,
        }
    }
}

/// GPU-friendly cubic profile segment: r(u) = c0 + c1*u + c2*u^2 + c3*u^3 for u = (y - y0)/(y1 - y0) in [0, 1].
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ProfileSegment {
    pub y0: f32,
    pub y1: f32,
    pub r_min: f32,
    pub r_max: f32,
    pub c0: f32,
    pub c1: f32,
    pub c2: f32,
    pub c3: f32,
}

impl ProfileSegment {
    pub fn eval_r(&self, y: f32) -> f32 {
        let dy = (self.y1 - self.y0).max(1e-6);
        let u = ((y - self.y0) / dy).clamp(0.0, 1.0);
        self.c0 + u * (self.c1 + u * (self.c2 + u * self.c3))
    }

    pub fn eval_dr_dy(&self, y: f32) -> f32 {
        let dy = (self.y1 - self.y0).max(1e-6);
        let u = ((y - self.y0) / dy).clamp(0.0, 1.0);
        (self.c1 + u * (2.0 * self.c2 + 3.0 * self.c3 * u)) / dy
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MeniscusState {
    pub fill_active: bool,
    pub y_bot: f32,
    pub y_contact: f32,
    pub r_wall: f32,
    pub h_men: f32,
    pub l_c: f32,
}

#[derive(Debug, Clone)]
pub struct FlaskGeometry {
    pub volume: FlaskVolume,
    pub total_h: f32,
    pub base_t: f32,
    pub y_belly: f32,
    #[allow(dead_code)]
    pub y_neck_start: f32,
    pub y_mark: f32,
    pub r_base_flat: f32,
    pub r_inner_flat: f32,
    pub r_bulb_max: f32,
    pub r_neck_in: f32,
    pub r_rim_in: f32,
    pub r_rim_out: f32,
    pub label_y_min: f32,
    pub label_y_max: f32,
    #[allow(dead_code)]
    pub wall_t: f32,
    pub outer_segs: Vec<ProfileSegment>,
    pub inner_segs: Vec<ProfileSegment>,
    pub stopper_segs: Vec<ProfileSegment>,
    pub calibrated_volume_ml: f32,
    pub max_safe_volume_ml: f32,
    pub label_tex_data: Vec<u8>,
}

impl FlaskGeometry {
    pub fn build(volume: FlaskVolume) -> Self {
        let (total_h, r_bulb_nom, r_neck, r_base, wall_t, base_t) = volume.dimensions_cm();
        let target_mark_y = 0.61 * total_h;
        let nominal_ml = volume.ml();

        // Binary search a subtle bulb radius scale so V(target_mark_y) == nominal_ml.
        let mut lo = 0.70_f32;
        let mut hi = 1.40_f32;
        for _ in 0..28 {
            let mid = 0.5 * (lo + hi);
            let (_, inner_segs, _, _, _) =
                build_profiles(total_h, r_bulb_nom * mid, r_neck, r_base, wall_t, base_t);
            let vol = integrate_volume_to(&inner_segs, target_mark_y);
            if vol < nominal_ml {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let best_scale = 0.5 * (lo + hi);
        let (outer_segs, inner_segs, y_belly, y_neck_start, r_base_flat) =
            build_profiles(total_h, r_bulb_nom * best_scale, r_neck, r_base, wall_t, base_t);

        let y_mark = solve_y_for_volume(&inner_segs, nominal_ml, base_t, total_h - 0.5)
            .unwrap_or(target_mark_y);
        let calibrated_volume_ml = integrate_volume_to(&inner_segs, y_mark);
        let max_safe_volume_ml = integrate_volume_to(&inner_segs, total_h - 0.35);

        let r_inner_flat = inner_segs.first().map(|s| s.c0).unwrap_or(0.5 * r_base_flat);
        let r_rim_in = inner_segs.last().map(|s| s.eval_r(s.y1)).unwrap_or(r_neck - wall_t);
        let r_rim_out = outer_segs.last().map(|s| s.eval_r(s.y1)).unwrap_or(r_neck);
        let r_bulb_max = outer_segs
            .iter()
            .map(|s| s.r_max)
            .fold(0.0_f32, f32::max);
        let r_neck_in = (r_neck - wall_t).max(0.18);

        let stopper_segs = build_stopper_profile(total_h, r_neck, r_neck_in);

        // Label sits across the upper pear-shaped shoulder cleanly below the neck fillet.
        let label_y_min = y_belly + 0.14 * (y_neck_start - y_belly);
        let label_y_max = y_belly + 0.72 * (y_neck_start - y_belly);
        let label_tex_data = rasterize_enamel_label(volume);

        Self {
            volume,
            total_h,
            base_t,
            y_belly,
            y_neck_start,
            y_mark,
            r_base_flat,
            r_inner_flat,
            r_bulb_max,
            r_neck_in,
            r_rim_in,
            r_rim_out,
            label_y_min,
            label_y_max,
            wall_t,
            outer_segs,
            inner_segs,
            stopper_segs,
            calibrated_volume_ml,
            max_safe_volume_ml,
            label_tex_data,
        }
    }

    /// Approximate inner bulb diameter in cm for analytical Beer-Lambert color preview.
    pub fn bulb_inner_diameter_cm(&self) -> f32 {
        2.0 * eval_profile_radius(&self.inner_segs, self.y_belly)
    }

    pub fn neck_inner_diameter_cm(&self) -> f32 {
        2.0 * self.r_neck_in
    }

    pub fn solve_meniscus(&self, fill_fraction: f32, l_c: f32) -> MeniscusState {
        if fill_fraction <= 0.001 {
            return MeniscusState {
                fill_active: false,
                y_bot: self.base_t,
                y_contact: self.base_t,
                r_wall: self.r_inner_flat,
                h_men: 0.0,
                l_c,
            };
        }

        let target_ml = (fill_fraction * self.volume.ml()).clamp(0.02, self.max_safe_volume_ml);
        let y_bot = solve_y_for_volume(&self.inner_segs, target_ml, self.base_t, self.total_h - 0.35)
            .unwrap_or(self.y_mark);

        let mut r_wall = eval_profile_radius(&self.inner_segs, y_bot);
        let mut h_men = 0.0_f32;
        for _ in 0..8 {
            let e1 = (-r_wall / l_c).exp();
            let tanh_half = (1.0 - e1) / (1.0 + e1).max(1e-5);
            let max_headroom = (self.total_h - 0.06 - y_bot).max(0.0);
            h_men = (0.24_f32)
                .min(0.32 * r_wall)
                .min(1.55 * l_c * tanh_half)
                .min(max_headroom);
            r_wall = eval_profile_radius(&self.inner_segs, y_bot + h_men);
        }

        MeniscusState {
            fill_active: true,
            y_bot,
            y_contact: y_bot + h_men,
            r_wall,
            h_men,
            l_c,
        }
    }
}

pub fn eval_profile_radius(segs: &[ProfileSegment], y: f32) -> f32 {
    if segs.is_empty() {
        return 0.0;
    }
    if y <= segs[0].y0 {
        return segs[0].c0;
    }
    let last = &segs[segs.len() - 1];
    if y >= last.y1 {
        return last.eval_r(last.y1);
    }
    let idx = segs
        .partition_point(|s| s.y1 < y)
        .min(segs.len() - 1);
    segs[idx].eval_r(y)
}

/// Exact analytical volume V(y) = pi * int R_in(t)^2 dt across cubic Hermite segments.
pub fn integrate_volume_to(segs: &[ProfileSegment], y_target: f32) -> f32 {
    let mut vol = 0.0_f64;
    // 4-point Gauss-Legendre on [0, 1] integrates polynomials up to degree 7 exactly (R^2 has degree 6!).
    const GL_NODES: [f64; 4] = [0.0694318442, 0.3300094782, 0.6699905218, 0.9305681558];
    const GL_WEIGHTS: [f64; 4] = [0.1739274226, 0.3260725774, 0.3260725774, 0.1739274226];

    for seg in segs {
        if y_target <= seg.y0 {
            break;
        }
        let y_hi = y_target.min(seg.y1);
        let dy_seg = (seg.y1 - seg.y0) as f64;
        if dy_seg <= 1e-8 || y_hi <= seg.y0 {
            continue;
        }
        let u_hi = ((y_hi - seg.y0) as f64 / dy_seg).clamp(0.0, 1.0);
        let mut seg_int = 0.0_f64;
        for (&node, &w) in GL_NODES.iter().zip(GL_WEIGHTS.iter()) {
            let u = u_hi * node;
            let r = seg.c0 as f64
                + u * (seg.c1 as f64 + u * (seg.c2 as f64 + u * seg.c3 as f64));
            seg_int += w * r * r;
        }
        vol += std::f64::consts::PI * (dy_seg * u_hi) * seg_int;
    }
    vol as f32
}

fn solve_y_for_volume(
    segs: &[ProfileSegment],
    target_ml: f32,
    y_min: f32,
    y_max: f32,
) -> Option<f32> {
    let v_lo = integrate_volume_to(segs, y_min);
    let v_hi = integrate_volume_to(segs, y_max);
    if target_ml <= v_lo {
        return Some(y_min);
    }
    if target_ml >= v_hi {
        return Some(y_max);
    }
    let mut lo = y_min;
    let mut hi = y_max;
    for _ in 0..36 {
        let mid = 0.5 * (lo + hi);
        if integrate_volume_to(segs, mid) < target_ml {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn eval_outer_design_curve(
    y: f32,
    total_h: f32,
    r_bulb: f32,
    r_neck: f32,
    r_base_flat: f32,
    y_belly: f32,
    y_neck_start: f32,
) -> f32 {
    let rim_h = (0.38 * r_neck).clamp(0.20, 0.52);
    let y_lip_start = total_h - rim_h;

    if y <= y_belly {
        // Smooth rounded bottom heel: starts from flat base radius with steep finite slope (~74 deg bevel).
        let t = (y / y_belly.max(1e-4)).clamp(0.0, 1.0);
        // s(t) = 1 - (1 - t)^1.85 gives smooth curvature peaking at y_belly with zero slope ds/dt=0 at t=1!
        let s = 1.0 - (1.0 - t).max(0.0).powf(1.85);
        r_base_flat + (r_bulb - r_base_flat) * s
    } else if y < y_neck_start {
        // Classic pear/conical volumetric flask shoulder sweeping into cylindrical neck.
        let t = ((y - y_belly) / (y_neck_start - y_belly).max(1e-4)).clamp(0.0, 1.0);
        // Blend between smooth cosine-like belly rolloff and conical straight taper.
        let s_belly = smoothstep(t);
        let s_taper = 0.55 * s_belly + 0.45 * (t * t * (3.0 - 2.0 * t)).max(0.0).powf(0.85);
        r_bulb + (r_neck - r_bulb) * s_taper
    } else if y <= y_lip_start {
        // Straight cylindrical neck where calibration ring sits.
        r_neck
    } else {
        // Subtle rolled fire-polished lip bead at top of neck.
        let t = ((y - y_lip_start) / rim_h.max(1e-4)).clamp(0.0, 1.0);
        let bump = (PI * t).sin().max(0.0).powf(1.15);
        let lip_flare = 0.17 * r_neck;
        r_neck + lip_flare * bump + 0.04 * r_neck * smoothstep(t)
    }
}

fn build_profiles(
    total_h: f32,
    r_bulb: f32,
    r_neck: f32,
    r_base: f32,
    wall_t: f32,
    base_t: f32,
) -> (Vec<ProfileSegment>, Vec<ProfileSegment>, f32, f32, f32) {
    let y_belly = (0.145 * total_h).max(base_t + 0.45);
    let y_neck_start = 0.46 * total_h;
    let r_base_flat = (0.82 * r_base).min(r_bulb * 0.86);
    let rim_h = (0.38 * r_neck).clamp(0.20, 0.52);
    let y_lip_start = total_h - rim_h;

    // 1. Sample dense outer curve and build cubic Hermite segments at well-spaced knots.
    let mut outer_knots = Vec::new();
    for i in 0..=8 {
        let t = i as f32 / 8.0;
        // Cluster slightly near y = 0 for crisp heel curvature.
        let warped = t.powf(1.25);
        outer_knots.push(warped * y_belly);
    }
    for i in 1..=12 {
        let t = i as f32 / 12.0;
        outer_knots.push(y_belly + t * (y_neck_start - y_belly));
    }
    let neck_len = (y_lip_start - y_neck_start).max(0.1);
    for i in 1..=4 {
        let t = i as f32 / 5.0;
        outer_knots.push(y_neck_start + t * neck_len);
    }
    outer_knots.push(y_lip_start);
    for i in 1..=5 {
        let t = i as f32 / 5.0;
        outer_knots.push(y_lip_start + t * rim_h);
    }

    let outer_pts: Vec<(f32, f32)> = outer_knots
        .iter()
        .map(|&y| {
            (
                y,
                eval_outer_design_curve(y, total_h, r_bulb, r_neck, r_base_flat, y_belly, y_neck_start),
            )
        })
        .collect();

    let outer_segs = fit_c1_hermite_segments(&outer_pts);

    // 2. Construct inner profile by offsetting outer profile inward along 2D curve normals.
    let n_dense = 240usize;
    let mut offset_pts: Vec<(f32, f32)> = Vec::with_capacity(n_dense + 1);
    for i in 0..=n_dense {
        let y = (i as f32 / n_dense as f32) * total_h;
        let r = eval_profile_radius(&outer_segs, y);
        let dr_dy = eval_profile_dr_dy(&outer_segs, y);
        let len = (1.0 + dr_dy * dr_dy).sqrt();
        // Inward 2D normal in (y, r) coordinates: tangent = (1, dr_dy), inward normal = (dr_dy, -1) / len
        let mut local_wt = wall_t;
        // Slightly thicker glass in bottom heel, matching real blown flasks.
        if y < y_belly {
            let k = 1.0 - smoothstep(y / y_belly);
            local_wt *= 1.0 + 0.18 * k;
        }
        // Keep cylindrical inner neck wall clean at the rolled lip (no inward bulge!).
        if y > y_lip_start {
            let t_lip = (y - y_lip_start) / rim_h.max(1e-4);
            let y_in = y;
            let r_in = (r_neck - wall_t) + 0.04 * r_neck * smoothstep(t_lip);
            offset_pts.push((y_in, r_in.max(0.15)));
            continue;
        }
        let y_in = y + local_wt * (dr_dy / len);
        let r_in = r - local_wt * (1.0 / len);
        offset_pts.push((y_in, r_in));
    }

    // Build inner knots starting from flat inner base floor at y = base_t.
    let r_inner_flat = (0.58 * r_base_flat).max(0.25);
    let r_inner_belly = (eval_sample_polyline(&offset_pts, y_belly)).max(r_inner_flat + 0.15);

    let mut inner_pts: Vec<(f32, f32)> = Vec::new();
    // Smooth inner bottom heel from (base_t, r_inner_flat) up to y_belly
    let inner_heel_top = y_belly.max(base_t + 0.30);
    for i in 0..=7 {
        let t = i as f32 / 7.0;
        let y = base_t + t.powf(1.25) * (inner_heel_top - base_t);
        let s = 1.0 - (1.0 - t).powf(1.80);
        let r_heel = r_inner_flat + (r_inner_belly - r_inner_flat) * s;
        let r_out = eval_profile_radius(&outer_segs, y);
        inner_pts.push((y, r_heel.min(r_out - 0.65 * wall_t)));
    }
    for i in 1..=12 {
        let t = i as f32 / 12.0;
        let y = inner_heel_top + t * (y_neck_start - inner_heel_top);
        let r_off = eval_sample_polyline(&offset_pts, y);
        let r_out = eval_profile_radius(&outer_segs, y);
        inner_pts.push((y, r_off.min(r_out - 0.65 * wall_t).max(0.15)));
    }
    for i in 1..=4 {
        let t = i as f32 / 5.0;
        let y = y_neck_start + t * neck_len;
        let r_out = eval_profile_radius(&outer_segs, y);
        inner_pts.push((y, (r_neck - wall_t).min(r_out - 0.65 * wall_t).max(0.15)));
    }
    inner_pts.push((y_lip_start, (r_neck - wall_t).max(0.15)));
    inner_pts.push((
        total_h,
        (r_neck - wall_t + 0.04 * r_neck).min(eval_profile_radius(&outer_segs, total_h) - 0.65 * wall_t),
    ));

    let inner_segs = fit_c1_hermite_segments(&inner_pts);

    (outer_segs, inner_segs, y_belly, y_neck_start, r_base_flat)
}

fn eval_profile_dr_dy(segs: &[ProfileSegment], y: f32) -> f32 {
    if segs.is_empty() {
        return 0.0;
    }
    let idx = segs.partition_point(|s| s.y1 < y).min(segs.len() - 1);
    segs[idx].eval_dr_dy(y)
}

fn eval_sample_polyline(pts: &[(f32, f32)], y: f32) -> f32 {
    if pts.is_empty() {
        return 0.0;
    }
    if y <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        let (y0, r0) = w[0];
        let (y1, r1) = w[1];
        if y <= y1 && y1 > y0 {
            let t = (y - y0) / (y1 - y0);
            return r0 + t * (r1 - r0);
        }
    }
    pts.last().unwrap().1
}

/// Fit C1-continuous, overshoot-free Fritsch-Carlson cubic Hermite segments through `(y, r)` points.
fn fit_c1_hermite_segments(pts: &[(f32, f32)]) -> Vec<ProfileSegment> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }

    let mut secants = vec![0.0_f32; n - 1];
    for i in 0..(n - 1) {
        let dy = (pts[i + 1].0 - pts[i].0).max(1e-5);
        secants[i] = (pts[i + 1].1 - pts[i].1) / dy;
    }

    let mut slopes = vec![0.0_f32; n];
    slopes[0] = secants[0];
    slopes[n - 1] = secants[n - 2];
    for i in 1..(n - 1) {
        let d_prev = secants[i - 1];
        let d_next = secants[i];
        if d_prev * d_next <= 0.0 || d_prev.abs() < 1e-5 || d_next.abs() < 1e-5 {
            slopes[i] = 0.0;
        } else {
            let dy_prev = (pts[i].0 - pts[i - 1].0).max(1e-5);
            let dy_next = (pts[i + 1].0 - pts[i].0).max(1e-5);
            // Weighted harmonic mean preserves monotonicity with zero overshoot
            let w1 = 2.0 * dy_next + dy_prev;
            let w2 = dy_next + 2.0 * dy_prev;
            slopes[i] = (w1 + w2) / (w1 / d_prev + w2 / d_next);
        }
    }

    // Fritsch-Carlson monotonicity limiter on each segment
    for i in 0..(n - 1) {
        let d = secants[i];
        if d.abs() < 1e-6 {
            slopes[i] = 0.0;
            slopes[i + 1] = 0.0;
        } else {
            let alpha = slopes[i] / d;
            let beta = slopes[i + 1] / d;
            let sq = alpha * alpha + beta * beta;
            if sq > 9.0 {
                let tau = 3.0 / sq.sqrt();
                slopes[i] = tau * alpha * d;
                slopes[i + 1] = tau * beta * d;
            }
        }
    }

    let mut segs = Vec::with_capacity(n - 1);
    for i in 0..(n - 1) {
        let (y0, r0) = pts[i];
        let (y1, r1) = pts[i + 1];
        let h = (y1 - y0).max(1e-5);
        let m0 = slopes[i] * h;
        let m1 = slopes[i + 1] * h;
        let c0 = r0;
        let c1 = m0;
        let c2 = -3.0 * r0 + 3.0 * r1 - 2.0 * m0 - m1;
        let c3 = 2.0 * r0 - 2.0 * r1 + m0 + m1;

        let mut r_min = r0.min(r1);
        let mut r_max = r0.max(r1);
        for k in 1..8 {
            let u = k as f32 / 8.0;
            let rk = c0 + u * (c1 + u * (c2 + u * c3));
            r_min = r_min.min(rk);
            r_max = r_max.max(rk);
        }

        segs.push(ProfileSegment {
            y0,
            y1,
            r_min: (r_min - 0.01).max(0.0),
            r_max: r_max + 0.01,
            c0,
            c1,
            c2,
            c3,
        });
    }
    segs
}

/// Build an authentic conical ground stopper + pennyhead/cap profile seated cleanly inside the neck.
fn build_stopper_profile(total_h: f32, r_neck: f32, r_neck_in: f32) -> Vec<ProfileSegment> {
    let plug_depth = (1.55 * r_neck).clamp(0.75, 1.8);
    let y_bot = total_h - plug_depth;
    let r_plug_bot = (0.86 * r_neck_in).max(0.12);
    let r_plug_top = (0.985 * r_neck_in).max(r_plug_bot + 0.02);
    let cap_h = (1.85 * r_neck).clamp(0.9, 2.4);

    let pts = vec![
        (y_bot, 0.015),
        (y_bot + 0.10 * plug_depth, r_plug_bot),
        (total_h - 0.05, r_plug_top),
        (total_h + 0.14 * cap_h, r_neck * 1.15),
        (total_h + 0.34 * cap_h, r_neck * 0.62),
        (total_h + 0.56 * cap_h, r_neck * 0.62),
        (total_h + 0.78 * cap_h, r_neck * 1.48),
        (total_h + 0.94 * cap_h, r_neck * 1.44),
        (total_h + cap_h, 0.02),
    ];
    fit_c1_hermite_segments(&pts)
}

/// Procedural 512x512 R8Unorm enamel marking texture for the front shoulder of the flask.
fn rasterize_enamel_label(volume: FlaskVolume) -> Vec<u8> {
    const W: usize = 512;
    const H: usize = 512;
    let mut buf = vec![0u8; W * H];

    let line1 = volume.label();
    let line2 = if volume == FlaskVolume::Ml10 {
        format!("A   ±{:.3} mL", volume.tolerance_ml())
    } else {
        format!("A   ±{:.2} mL", volume.tolerance_ml())
    };
    let line3 = "20 °C   BORO 3.3";

    draw_text_centered(&mut buf, W, H, line1, 165, 8);
    draw_text_centered(&mut buf, W, H, &line2, 265, 5);
    draw_text_centered(&mut buf, W, H, line3, 335, 4);

    // Subtle classic horizontal indicator ticks flanking the capacity number
    for y in 125..131 {
        for x in 170..342 {
            buf[y * W + x] = 235;
        }
    }
    for y in 395..401 {
        for x in 170..342 {
            buf[y * W + x] = 235;
        }
    }

    // Soft 3x3 box blur for crisp anti-aliased enamel edges on GPU bilinear sampling
    let src = buf.clone();
    for y in 1..(H - 1) {
        for x in 1..(W - 1) {
            let mut sum = 0u32;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    sum += src[(y as isize + dy) as usize * W + (x as isize + dx) as usize] as u32;
                }
            }
            buf[y * W + x] = (sum / 9) as u8;
        }
    }
    buf
}

fn draw_text_centered(buf: &mut [u8], w: usize, h: usize, text: &str, top_y: usize, scale: usize) {
    let chars: Vec<char> = text.chars().collect();
    let char_adv = 6 * scale;
    let total_w = chars.len() * char_adv;
    let start_x = w.saturating_sub(total_w) / 2;

    for (ci, &ch) in chars.iter().enumerate() {
        let glyph = glyph_5x7(ch);
        let gx0 = start_x + ci * char_adv;
        for (row, &bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 == 1 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = gx0 + col * scale + sx;
                            let py = top_y + row * scale + sy;
                            if px < w && py < h {
                                buf[py * w + px] = 255;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'm' => [0b00000, 0b00000, 0b11010, 0b10101, 0b10101, 0b10001, 0b10001],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '±' => [0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000, 0b11111],
        '°' => [0b01100, 0b10010, 0b10010, 0b01100, 0b00000, 0b00000, 0b00000],
        _ => [0; 7],
    }
}

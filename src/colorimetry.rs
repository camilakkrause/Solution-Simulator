use std::sync::OnceLock;

use crate::spectrum::SPECTRUM_LEN;

pub const D65_WHITE_XYZ: [f32; 3] = [0.95047, 1.00000, 1.08883];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Illuminant {
    D65 = 0,
    D50 = 1,
    A = 2,
    Fl2 = 3,
    LabLed = 4,
    EqualEnergy = 5,
}

impl Illuminant {
    pub const ALL: [Illuminant; 6] = [
        Illuminant::D65,
        Illuminant::D50,
        Illuminant::A,
        Illuminant::Fl2,
        Illuminant::LabLed,
        Illuminant::EqualEnergy,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Illuminant::D65 => "CIE D65 (Daylight 6500K)",
            Illuminant::D50 => "CIE D50 (Horizon 5000K)",
            Illuminant::A => "CIE A (Tungsten 2856K)",
            Illuminant::Fl2 => "CIE FL2 (Lab Fluorescent)",
            Illuminant::LabLed => "5000K High-CRI Lab LED",
            Illuminant::EqualEnergy => "CIE E (Equal Energy)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ToneMapper {
    PbrNeutral = 0,
    Linear = 1,
    Aces = 2,
}

impl ToneMapper {
    pub const ALL: [ToneMapper; 3] = [ToneMapper::PbrNeutral, ToneMapper::Linear, ToneMapper::Aces];

    pub fn label(self) -> &'static str {
        match self {
            ToneMapper::PbrNeutral => "PBR Neutral (Scientific)",
            ToneMapper::Linear => "Linear Clamp",
            ToneMapper::Aces => "ACES Filmic",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ColorReadout {
    #[allow(dead_code)]
    pub xyz: [f32; 3],
    pub xy: [f32; 2],
    pub lab: [f32; 3],
    #[allow(dead_code)]
    pub linear_srgb: [f32; 3],
    pub srgb_u8: [u8; 3],
    pub hex: String,
    pub mean_transmittance_pct: f32,
}

/// CIE 1931 2-degree Standard Observer (x_bar, y_bar, z_bar) at 5 nm steps from 380 to 780 nm (81 entries).
const CIE_1931_5NM: [[f32; 3]; 81] = [
    [0.001368, 0.000039, 0.006450], // 380
    [0.002236, 0.000064, 0.010550], // 385
    [0.004243, 0.000120, 0.020050], // 390
    [0.007650, 0.000217, 0.036210], // 395
    [0.014310, 0.000396, 0.067850], // 400
    [0.023190, 0.000640, 0.110200], // 405
    [0.043510, 0.001210, 0.207400], // 410
    [0.077630, 0.002180, 0.371300], // 415
    [0.134380, 0.004000, 0.645600], // 420
    [0.214770, 0.007300, 1.039050], // 425
    [0.283900, 0.011600, 1.385600], // 430
    [0.328500, 0.016840, 1.622960], // 435
    [0.348280, 0.023000, 1.747060], // 440
    [0.348060, 0.029800, 1.782600], // 445
    [0.336200, 0.038000, 1.772110], // 450
    [0.318700, 0.048000, 1.744100], // 455
    [0.290800, 0.060000, 1.669200], // 460
    [0.251100, 0.073900, 1.528100], // 465
    [0.195360, 0.090980, 1.287640], // 470
    [0.142100, 0.112600, 1.041900], // 475
    [0.095640, 0.139020, 0.812950], // 480
    [0.057950, 0.169300, 0.616200], // 485
    [0.032010, 0.208020, 0.465180], // 490
    [0.014700, 0.258600, 0.353300], // 495
    [0.004900, 0.323000, 0.272000], // 500
    [0.002400, 0.407300, 0.212300], // 505
    [0.009300, 0.503000, 0.158200], // 510
    [0.029100, 0.608200, 0.111700], // 515
    [0.063270, 0.710000, 0.078250], // 520
    [0.109600, 0.793200, 0.057250], // 525
    [0.165500, 0.862000, 0.042160], // 530
    [0.225750, 0.914850, 0.029840], // 535
    [0.290400, 0.954000, 0.020300], // 540
    [0.359700, 0.980300, 0.013400], // 545
    [0.433450, 0.994950, 0.008750], // 550
    [0.512050, 1.000000, 0.005750], // 555
    [0.594500, 0.995000, 0.003900], // 560
    [0.678400, 0.978600, 0.002750], // 565
    [0.762100, 0.952000, 0.002100], // 570
    [0.842500, 0.915400, 0.001800], // 575
    [0.916300, 0.870000, 0.001650], // 580
    [0.978600, 0.816300, 0.001400], // 585
    [1.026300, 0.757000, 0.001100], // 590
    [1.056700, 0.694900, 0.001000], // 595
    [1.062200, 0.631000, 0.000800], // 600
    [1.045600, 0.566800, 0.000600], // 605
    [1.002600, 0.503000, 0.000340], // 610
    [0.938400, 0.441200, 0.000240], // 615
    [0.854450, 0.381000, 0.000190], // 620
    [0.751400, 0.321000, 0.000100], // 625
    [0.642400, 0.265000, 0.000050], // 630
    [0.541900, 0.217000, 0.000030], // 635
    [0.447900, 0.175000, 0.000020], // 640
    [0.360800, 0.138200, 0.000010], // 645
    [0.283500, 0.107000, 0.000000], // 650
    [0.218700, 0.081600, 0.000000], // 655
    [0.164900, 0.061000, 0.000000], // 660
    [0.121200, 0.044580, 0.000000], // 665
    [0.087400, 0.032000, 0.000000], // 670
    [0.063600, 0.023200, 0.000000], // 675
    [0.046770, 0.017000, 0.000000], // 680
    [0.032900, 0.011920, 0.000000], // 685
    [0.022700, 0.008210, 0.000000], // 690
    [0.015840, 0.005723, 0.000000], // 695
    [0.011359, 0.004102, 0.000000], // 700
    [0.008111, 0.002929, 0.000000], // 705
    [0.005790, 0.002091, 0.000000], // 710
    [0.004109, 0.001484, 0.000000], // 715
    [0.002899, 0.001047, 0.000000], // 720
    [0.002049, 0.000740, 0.000000], // 725
    [0.001440, 0.000520, 0.000000], // 730
    [0.001000, 0.000361, 0.000000], // 735
    [0.000690, 0.000249, 0.000000], // 740
    [0.000476, 0.000172, 0.000000], // 745
    [0.000332, 0.000120, 0.000000], // 750
    [0.000235, 0.000085, 0.000000], // 755
    [0.000166, 0.000060, 0.000000], // 760
    [0.000117, 0.000042, 0.000000], // 765
    [0.000083, 0.000030, 0.000000], // 770
    [0.000059, 0.000021, 0.000000], // 775
    [0.000042, 0.000015, 0.000000], // 780
];

/// CIE 15:2004 Standard Illuminant D65 relative SPD at 5 nm steps from 380 to 780 nm.
const CIE_D65_5NM: [f32; 81] = [
    49.9755, 52.3118, 54.6482, 68.7015, 82.7549, 87.1204, 91.4860, 92.4589, 93.4318, 90.0570,
    86.6823, 95.7736, 104.8650, 110.9360, 117.0080, 117.4100, 117.8120, 116.3360, 114.8610,
    115.3920, 115.9230, 112.3670, 108.8110, 109.0820, 109.3540, 108.5780, 107.8020, 106.2960,
    104.7900, 106.2390, 107.6890, 106.0470, 104.4050, 104.2250, 104.0460, 102.0230, 100.0000,
    98.1671, 96.3342, 96.0611, 95.7880, 92.2368, 88.6856, 89.3459, 90.0062, 89.8026, 89.5991,
    88.6489, 87.6987, 85.4936, 83.2886, 83.4939, 83.6992, 81.8630, 80.0268, 80.1207, 80.2146,
    81.2462, 82.2778, 80.2810, 78.2842, 74.0027, 69.7213, 70.6652, 71.6091, 72.9790, 74.3490,
    67.9765, 61.6040, 65.7448, 69.8856, 72.4863, 75.0870, 69.3398, 63.5927, 55.0054, 46.4182,
    56.6118, 66.8054, 65.0941, 63.3828,
];

/// CIE Daylight basis functions S0, S1, S2 at 10 nm steps from 380 to 780 nm (41 entries).
const CIE_DAYLIGHT_S0_10NM: [f32; 41] = [
    63.4, 65.8, 94.8, 104.8, 105.9, 96.8, 113.9, 125.6, 125.5, 121.3, 121.3, 113.5, 113.1, 110.8,
    106.5, 108.8, 105.3, 104.4, 100.0, 96.0, 95.1, 89.1, 90.5, 90.3, 88.4, 84.0, 85.1, 81.9, 82.6,
    84.9, 81.3, 71.9, 74.3, 76.4, 63.3, 71.7, 77.0, 65.2, 47.7, 68.6, 65.0,
];

const CIE_DAYLIGHT_S1_10NM: [f32; 41] = [
    38.5, 35.0, 43.4, 46.3, 43.9, 37.1, 36.7, 35.9, 32.6, 27.9, 24.3, 20.1, 16.2, 13.2, 8.6, 6.1,
    4.2, 1.9, 0.0, -1.6, -3.5, -3.5, -5.8, -7.2, -8.6, -9.5, -10.9, -10.7, -12.0, -14.0, -13.6,
    -12.0, -13.3, -12.9, -10.6, -11.6, -12.2, -10.2, -7.8, -11.2, -10.4,
];

const CIE_DAYLIGHT_S2_10NM: [f32; 41] = [
    3.0, 1.2, -1.1, -0.5, -0.7, -1.2, -2.6, -2.9, -2.8, -2.6, -2.6, -1.8, -1.5, -1.3, -1.2, -1.0,
    -0.5, -0.3, 0.0, 0.2, 0.5, 2.1, 3.2, 4.1, 4.7, 5.1, 6.7, 7.3, 8.6, 9.8, 10.2, 8.3, 9.6, 8.5,
    7.0, 7.6, 8.0, 6.7, 5.2, 7.4, 6.8,
];

const XYZ_TO_SRGB: [[f32; 3]; 3] = [
    [3.2404542, -1.5371385, -0.4985314],
    [-0.9692660, 1.8760108, 0.0415560],
    [0.0556434, -0.2040259, 1.0572252],
];

const M_BRADFORD: [[f32; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

const M_BRADFORD_INV: [[f32; 3]; 3] = [
    [0.9869929, -0.1470543, 0.1599627],
    [0.4323053, 0.5183603, 0.0492912],
    [-0.0085287, 0.0400428, 0.9684867],
];

static CMF_1NM: OnceLock<[[f32; 3]; SPECTRUM_LEN]> = OnceLock::new();

/// CIE 1931 2-degree color matching functions interpolated onto 380..=780 nm (1 nm grid).
pub fn cmf_1nm() -> &'static [[f32; 3]; SPECTRUM_LEN] {
    CMF_1NM.get_or_init(|| {
        let mut out = [[0.0f32; 3]; SPECTRUM_LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            let x = i as f32 / 5.0;
            let idx = (x.floor() as usize).min(79);
            let t = x - idx as f32;
            let p0 = CIE_1931_5NM[idx.saturating_sub(1)];
            let p1 = CIE_1931_5NM[idx];
            let p2 = CIE_1931_5NM[(idx + 1).min(80)];
            let p3 = CIE_1931_5NM[(idx + 2).min(80)];
            for c in 0..3 {
                *slot = [
                    catmull_rom(p0[0], p1[0], p2[0], p3[0], t).max(0.0),
                    catmull_rom(p0[1], p1[1], p2[1], p3[1], t).max(0.0),
                    catmull_rom(p0[2], p1[2], p2[2], p3[2], t).max(0.0),
                ];
                let _ = c;
            }
        }
        out
    })
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

fn interp_5nm(table: &[f32; 81], idx_1nm: usize) -> f32 {
    let q = idx_1nm / 5;
    let r = idx_1nm % 5;
    if r == 0 || q >= 80 {
        return table[q.min(80)];
    }
    let t = r as f32 / 5.0;
    table[q] + t * (table[q + 1] - table[q])
}

fn interp_10nm(table: &[f32; 41], idx_1nm: usize) -> f32 {
    let q = idx_1nm / 10;
    let r = idx_1nm % 10;
    if r == 0 || q >= 40 {
        return table[q.min(40)];
    }
    let t = r as f32 / 10.0;
    table[q] + t * (table[q + 1] - table[q])
}

fn gaussian(x: f32, center: f32, sigma: f32, amp: f32) -> f32 {
    let d = (x - center) / sigma;
    amp * (-0.5 * d * d).exp()
}

/// Illuminant relative SPD on 380..=780 nm normalized so sum(I * y_bar) == 1.0.
pub fn illuminant_spd_1nm(illum: Illuminant) -> [f32; SPECTRUM_LEN] {
    let mut spd = [0.0f32; SPECTRUM_LEN];
    match illum {
        Illuminant::D65 => {
            for (i, slot) in spd.iter_mut().enumerate() {
                *slot = interp_5nm(&CIE_D65_5NM, i);
            }
        }
        Illuminant::D50 => {
            // CIE D50 canonical chromaticity (5003 K)
            let xd = 0.34567_f32;
            let yd = 0.35850_f32;
            let denom = 0.0241 + 0.2562 * xd - 0.7341 * yd;
            let m1 = (-1.3515 - 1.7703 * xd + 5.9114 * yd) / denom;
            let m2 = (0.0300 - 31.4424 * xd + 30.0717 * yd) / denom;
            for (i, slot) in spd.iter_mut().enumerate() {
                let s0 = interp_10nm(&CIE_DAYLIGHT_S0_10NM, i);
                let s1 = interp_10nm(&CIE_DAYLIGHT_S1_10NM, i);
                let s2 = interp_10nm(&CIE_DAYLIGHT_S2_10NM, i);
                *slot = (s0 + m1 * s1 + m2 * s2).max(0.0);
            }
        }
        Illuminant::A => {
            // Planckian radiator at T = 2856 K (c2 = 1.4388e7 nm*K)
            let c2 = 1.4388e7_f64;
            let t = 2856.0_f64;
            let ref_560 = (560.0_f64).powi(-5) / ((c2 / (560.0 * t)).exp() - 1.0);
            for (i, slot) in spd.iter_mut().enumerate() {
                let l = 380.0_f64 + i as f64;
                let rad = l.powi(-5) / ((c2 / (l * t)).exp() - 1.0);
                *slot = (100.0 * rad / ref_560) as f32;
            }
        }
        Illuminant::Fl2 => {
            // Cool White Fluorescent: halophosphate phosphor continuum + Hg emission lines
            for (i, slot) in spd.iter_mut().enumerate() {
                let l = 380.0 + i as f32;
                let continuum = gaussian(l, 480.0, 42.0, 26.0)
                    + gaussian(l, 578.0, 44.0, 62.0)
                    + gaussian(l, 625.0, 32.0, 14.0);
                let hg_lines = gaussian(l, 404.7, 2.2, 22.0)
                    + gaussian(l, 435.8, 2.2, 68.0)
                    + gaussian(l, 546.1, 2.2, 54.0)
                    + gaussian(l, 578.0, 2.5, 22.0);
                *slot = (continuum + hg_lines).max(0.0);
            }
        }
        Illuminant::LabLed => {
            // 5000K high-CRI phosphor-converted white LED
            for (i, slot) in spd.iter_mut().enumerate() {
                let l = 380.0 + i as f32;
                let blue_pump = gaussian(l, 450.0, 11.5, 92.0);
                let phosphor = gaussian(l, 535.0, 44.0, 56.0)
                    + gaussian(l, 610.0, 48.0, 74.0)
                    + gaussian(l, 660.0, 28.0, 18.0);
                *slot = (blue_pump + phosphor).max(0.0);
            }
        }
        Illuminant::EqualEnergy => {
            spd.fill(1.0);
        }
    }

    let cmf = cmf_1nm();
    let y_norm: f32 = spd.iter().zip(cmf.iter()).map(|(&s, c)| s * c[1]).sum();
    if y_norm > 1e-8 {
        for s in spd.iter_mut() {
            *s /= y_norm;
        }
    }
    spd
}

/// Illuminant white-point CIE XYZ for the normalized SPD.
pub fn illuminant_white_xyz(illum: Illuminant) -> [f32; 3] {
    let spd = illuminant_spd_1nm(illum);
    let cmf = cmf_1nm();
    let mut xyz = [0.0f32; 3];
    for (&s, c) in spd.iter().zip(cmf.iter()) {
        xyz[0] += s * c[0];
        xyz[1] += s * c[1];
        xyz[2] += s * c[2];
    }
    xyz
}

pub fn mat3_mul_vec3(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

pub fn mat3_mul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

/// Bradford chromatic adaptation transform mapping `src_white` -> `dst_white`.
pub fn bradford_cat_matrix(src_white: [f32; 3], dst_white: [f32; 3]) -> [[f32; 3]; 3] {
    let cone_src = mat3_mul_vec3(&M_BRADFORD, src_white);
    let cone_dst = mat3_mul_vec3(&M_BRADFORD, dst_white);
    let diag = [
        [cone_dst[0] / cone_src[0].max(1e-6), 0.0, 0.0],
        [0.0, cone_dst[1] / cone_src[1].max(1e-6), 0.0],
        [0.0, 0.0, cone_dst[2] / cone_src[2].max(1e-6)],
    ];
    mat3_mul(&M_BRADFORD_INV, &mat3_mul(&diag, &M_BRADFORD))
}

pub fn xyz_to_linear_srgb(xyz: [f32; 3]) -> [f32; 3] {
    mat3_mul_vec3(&XYZ_TO_SRGB, xyz)
}

/// Luminance-preserving gamut mapping into non-negative sRGB primaries.
pub fn gamut_map_linear_srgb(rgb: [f32; 3]) -> [f32; 3] {
    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    if luma <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let m = rgb[0].min(rgb[1]).min(rgb[2]);
    let mut mapped = rgb;
    if m < 0.0 {
        let t = luma / (luma - m);
        mapped = [
            luma + t * (rgb[0] - luma),
            luma + t * (rgb[1] - luma),
            luma + t * (rgb[2] - luma),
        ];
    }
    [mapped[0].max(0.0), mapped[1].max(0.0), mapped[2].max(0.0)]
}

#[allow(dead_code)]
pub fn apply_tone_mapper(rgb: [f32; 3], mapper: ToneMapper) -> [f32; 3] {
    match mapper {
        ToneMapper::PbrNeutral => {
            let start_compression = 0.8 - 0.04;
            let desaturation = 0.15;
            let x = rgb[0].min(rgb[1]).min(rgb[2]);
            let offset = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };
            let mut color = [rgb[0] - offset, rgb[1] - offset, rgb[2] - offset];
            let peak = color[0].max(color[1]).max(color[2]);
            if peak < start_compression {
                return [
                    color[0].clamp(0.0, 1.0),
                    color[1].clamp(0.0, 1.0),
                    color[2].clamp(0.0, 1.0),
                ];
            }
            let d = 1.0 - start_compression;
            let new_peak = 1.0 - d * d / (peak + d - start_compression);
            let scale = new_peak / peak;
            color = [color[0] * scale, color[1] * scale, color[2] * scale];
            let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
            [
                (color[0] + g * (new_peak - color[0])).clamp(0.0, 1.0),
                (color[1] + g * (new_peak - color[1])).clamp(0.0, 1.0),
                (color[2] + g * (new_peak - color[2])).clamp(0.0, 1.0),
            ]
        }
        ToneMapper::Linear => [
            rgb[0].clamp(0.0, 1.0),
            rgb[1].clamp(0.0, 1.0),
            rgb[2].clamp(0.0, 1.0),
        ],
        ToneMapper::Aces => {
            let f = |c: f32| -> f32 {
                ((c * (2.51 * c + 0.03)) / (c * (2.43 * c + 0.59) + 0.14)).clamp(0.0, 1.0)
            };
            [f(rgb[0]), f(rgb[1]), f(rgb[2])]
        }
    }
}

pub fn linear_to_srgb_channel(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[allow(dead_code)]
pub fn xyz_to_srgb_u8(xyz: [f32; 3], mapper: ToneMapper) -> [u8; 3] {
    let lin = gamut_map_linear_srgb(xyz_to_linear_srgb(xyz));
    let mapped = apply_tone_mapper(lin, mapper);
    [
        (linear_to_srgb_channel(mapped[0]) * 255.0).round() as u8,
        (linear_to_srgb_channel(mapped[1]) * 255.0).round() as u8,
        (linear_to_srgb_channel(mapped[2]) * 255.0).round() as u8,
    ]
}

/// Convert CIE XYZ into CIE L*a*b* relative to `ref_white`.
pub fn xyz_to_lab(xyz: [f32; 3], ref_white: [f32; 3]) -> [f32; 3] {
    let f = |t: f32| -> f32 {
        let delta = 6.0 / 29.0;
        if t > delta * delta * delta {
            t.cbrt()
        } else {
            t / (3.0 * delta * delta) + 4.0 / 29.0
        }
    };
    let fx = f(xyz[0] / ref_white[0].max(1e-6));
    let fy = f(xyz[1] / ref_white[1].max(1e-6));
    let fz = f(xyz[2] / ref_white[2].max(1e-6));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Analytical Beer-Lambert transmitted solution color for a uniform path length `path_cm`.
pub fn compute_transmitted_color(
    sigma_a: &[f32; SPECTRUM_LEN],
    path_cm: f32,
    illuminant: Illuminant,
    white_balance: bool,
) -> ColorReadout {
    let spd = illuminant_spd_1nm(illuminant);
    let cmf = cmf_1nm();
    let white_src = illuminant_white_xyz(illuminant);

    let mut xyz = [0.0f32; 3];
    let mut t_sum = 0.0f32;
    let mut y_weight_sum = 0.0f32;
    for i in 0..SPECTRUM_LEN {
        let trans = (-sigma_a[i] * path_cm.max(0.0)).exp();
        let w = spd[i];
        xyz[0] += w * trans * cmf[i][0];
        xyz[1] += w * trans * cmf[i][1];
        xyz[2] += w * trans * cmf[i][2];
        t_sum += trans * w * cmf[i][1];
        y_weight_sum += w * cmf[i][1];
    }

    let adapted_xyz = if white_balance {
        let cat = bradford_cat_matrix(white_src, D65_WHITE_XYZ);
        mat3_mul_vec3(&cat, xyz)
    } else {
        xyz
    };

    let sum_xyz = (adapted_xyz[0] + adapted_xyz[1] + adapted_xyz[2]).max(1e-6);
    let xy = [adapted_xyz[0] / sum_xyz, adapted_xyz[1] / sum_xyz];
    let lab = xyz_to_lab(
        adapted_xyz,
        if white_balance {
            D65_WHITE_XYZ
        } else {
            white_src
        },
    );
    let linear_srgb = gamut_map_linear_srgb(xyz_to_linear_srgb(adapted_xyz));
    let srgb_u8 = [
        (linear_to_srgb_channel(linear_srgb[0]) * 255.0).round() as u8,
        (linear_to_srgb_channel(linear_srgb[1]) * 255.0).round() as u8,
        (linear_to_srgb_channel(linear_srgb[2]) * 255.0).round() as u8,
    ];
    let hex = format!("#{:02X}{:02X}{:02X}", srgb_u8[0], srgb_u8[1], srgb_u8[2]);
    let mean_transmittance_pct = if y_weight_sum > 0.0 {
        100.0 * t_sum / y_weight_sum
    } else {
        0.0
    };

    ColorReadout {
        xyz: adapted_xyz,
        xy,
        lab,
        linear_srgb,
        srgb_u8,
        hex,
        mean_transmittance_pct,
    }
}

/// Pure monochromatic wavelength sRGB swatch for the spectrum plot rainbow axis.
pub fn wavelength_to_srgb(lambda_nm: f32) -> [u8; 3] {
    let idx = (lambda_nm.round() as i32 - 380).clamp(0, 400) as usize;
    let c = cmf_1nm()[idx];
    let lin = gamut_map_linear_srgb(xyz_to_linear_srgb(c));
    let peak = lin[0].max(lin[1]).max(lin[2]).max(1e-5);
    // Slight intensity taper near the invisible edges 380 nm and 780 nm
    let edge_fade = if lambda_nm < 410.0 {
        ((lambda_nm - 375.0) / 35.0).clamp(0.15, 1.0)
    } else if lambda_nm > 700.0 {
        ((785.0 - lambda_nm) / 85.0).clamp(0.12, 1.0)
    } else {
        1.0
    };
    [
        (linear_to_srgb_channel(edge_fade * lin[0] / peak) * 255.0).round() as u8,
        (linear_to_srgb_channel(edge_fade * lin[1] / peak) * 255.0).round() as u8,
        (linear_to_srgb_channel(edge_fade * lin[2] / peak) * 255.0).round() as u8,
    ]
}

pub const BEER_LUT_LIQ_LEN: usize = 512;
pub const BEER_LUT_GLASS_LEN: usize = 16;

/// Build CMF-importance wavelength sampling tables `(icdf_lambda, pdf_lambda)` across 380..=780 nm.
/// `icdf_lambda[k]` maps `u = k / 400.0` to wavelength in nm; `pdf_lambda[k]` is density in nm^-1.
pub fn build_wl_importance_tables(illum: Illuminant) -> ([f32; SPECTRUM_LEN], [f32; SPECTRUM_LEN]) {
    let spd = illuminant_spd_1nm(illum);
    let cmf = cmf_1nm();

    let mut raw = [0.0_f64; SPECTRUM_LEN];
    for i in 0..SPECTRUM_LEN {
        let sum_cmf = (cmf[i][0] + cmf[i][1] + cmf[i][2]) as f64;
        raw[i] = (spd[i] as f64) * sum_cmf.max(0.0);
    }
    let raw_int = trapz_integral(&raw);
    let norm = if raw_int > 1e-12 { raw_int } else { 1.0 };

    let mut pdf_f64 = [0.0_f64; SPECTRUM_LEN];
    for i in 0..SPECTRUM_LEN {
        pdf_f64[i] = 0.90 * (raw[i] / norm) + 0.10 * (1.0 / 400.0);
    }
    let pdf_int = trapz_integral(&pdf_f64);
    for p in pdf_f64.iter_mut() {
        *p /= pdf_int;
    }

    let mut cdf = [0.0_f64; SPECTRUM_LEN];
    for i in 0..(SPECTRUM_LEN - 1) {
        cdf[i + 1] = cdf[i] + 0.5 * (pdf_f64[i] + pdf_f64[i + 1]);
    }
    let total = cdf[SPECTRUM_LEN - 1].max(1e-12);
    for c in cdf.iter_mut() {
        *c /= total;
    }
    cdf[0] = 0.0;
    cdf[SPECTRUM_LEN - 1] = 1.0;

    let mut icdf = [0.0_f32; SPECTRUM_LEN];
    icdf[0] = 380.0;
    icdf[SPECTRUM_LEN - 1] = 780.0;
    let mut seg = 0usize;
    for k in 1..(SPECTRUM_LEN - 1) {
        let u = k as f64 / (SPECTRUM_LEN - 1) as f64;
        while seg + 1 < SPECTRUM_LEN - 1 && cdf[seg + 1] < u {
            seg += 1;
        }
        let span = (cdf[seg + 1] - cdf[seg]).max(1e-12);
        let frac = ((u - cdf[seg]) / span).clamp(0.0, 1.0);
        icdf[k] = (380.0 + seg as f64 + frac) as f32;
    }

    let mut pdf = [0.0_f32; SPECTRUM_LEN];
    for i in 0..SPECTRUM_LEN {
        pdf[i] = pdf_f64[i] as f32;
    }
    (icdf, pdf)
}

fn trapz_integral(vals: &[f64; SPECTRUM_LEN]) -> f64 {
    let mut sum = 0.5 * (vals[0] + vals[SPECTRUM_LEN - 1]);
    for &v in &vals[1..(SPECTRUM_LEN - 1)] {
        sum += v;
    }
    sum
}

/// Pre-integrate exact 1-nm Beer-Lambert CIE XYZ over `(d_liq, d_glass)` using quadratic grid warping.
pub fn build_beer_lut(
    sigma_liq: &[f32; SPECTRUM_LEN],
    sigma_glass: &[f32; SPECTRUM_LEN],
    glass_tint: f32,
    illum: Illuminant,
    d_liq_max: f32,
    d_glass_max: f32,
) -> Vec<[f32; 4]> {
    let spd = illuminant_spd_1nm(illum);
    let cmf = cmf_1nm();
    let mut lut = vec![[0.0_f32; 4]; BEER_LUT_LIQ_LEN * BEER_LUT_GLASS_LEN];

    for j in 0..BEER_LUT_GLASS_LEN {
        let v = j as f32 / (BEER_LUT_GLASS_LEN - 1) as f32;
        let d_glass = d_glass_max * v * v;
        for i in 0..BEER_LUT_LIQ_LEN {
            let u = i as f32 / (BEER_LUT_LIQ_LEN - 1) as f32;
            let d_liq = d_liq_max * u * u;

            let mut xyz = [0.0_f64; 3];
            for k in 0..SPECTRUM_LEN {
                let exponent =
                    -(sigma_liq[k] * d_liq + sigma_glass[k] * glass_tint * d_glass) as f64;
                let w = (spd[k] as f64) * exponent.exp();
                xyz[0] += w * cmf[k][0] as f64;
                xyz[1] += w * cmf[k][1] as f64;
                xyz[2] += w * cmf[k][2] as f64;
            }
            lut[j * BEER_LUT_LIQ_LEN + i] = [xyz[0] as f32, xyz[1] as f32, xyz[2] as f32, 0.0];
        }
    }
    lut
}

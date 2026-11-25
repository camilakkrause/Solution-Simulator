use bytemuck::{Pod, Zeroable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LightingPreset {
    LabStudio = 0,
    BacklitLightbox = 1,
    OverheadLamp = 2,
    WindowDaylight = 3,
}

impl LightingPreset {
    pub const ALL: [LightingPreset; 4] = [
        LightingPreset::LabStudio,
        LightingPreset::BacklitLightbox,
        LightingPreset::OverheadLamp,
        LightingPreset::WindowDaylight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LightingPreset::LabStudio => "Lab Studio Softboxes",
            LightingPreset::BacklitLightbox => "Backlit Spectrophotometer Box",
            LightingPreset::OverheadLamp => "Overhead Lab Lamp",
            LightingPreset::WindowDaylight => "Side Window Daylight",
        }
    }
}

/// 32-byte aligned spectral lookup entry for wavelength 380 + i nm.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SpectralEntry {
    /// [sigma_a_liquid (cm^-1), illuminant_spd, cmf_x, cmf_y]
    pub data0: [f32; 4],
    /// [cmf_z, sigma_a_glass (cm^-1), wl_icdf (nm), wl_pdf (nm^-1)]
    pub data1: [f32; 4],
}

/// Strictly 16-byte-aligned uniform block matching WGSL `SceneUniform`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SceneUniform {
    /// xyz = camera position (cm), w = tan(fov_y / 2)
    pub cam_pos: [f32; 4],
    /// xyz = camera right, w = aspect ratio (width / height)
    pub cam_right: [f32; 4],
    /// xyz = camera up, w = unused
    pub cam_up: [f32; 4],
    /// xyz = camera forward, w = unused
    pub cam_forward: [f32; 4],

    /// Bradford CAT matrix rows (xyz in each vec4)
    pub bradford_row0: [f32; 4],
    pub bradford_row1: [f32; 4],
    pub bradford_row2: [f32; 4],

    /// [width, height, frame_index, spp_per_frame]
    pub dims: [u32; 4],
    /// [outer_seg_count, inner_seg_count, stopper_seg_count, total_caustic_photons]
    pub counts: [u32; 4],
    /// [fill_active, stopper_type, enable_markings, enable_dispersion]
    pub flags: [u32; 4],
    /// [enable_caustics, lighting_preset, enable_white_card, tone_mapper]
    pub render_flags: [u32; 4],
    /// [white_balance, caustic_map_size, photons_per_frame, show_grid]
    pub render_flags2: [u32; 4],

    /// [y_mark, y_bot, y_contact, r_wall]
    pub flask0: [f32; 4],
    /// [h_men, l_c, flask_h, base_t]
    pub flask1: [f32; 4],
    /// [r_base_flat, r_inner_flat, r_rim_in, r_rim_out]
    pub flask2: [f32; 4],
    /// [r_bulb_max, label_y_min, label_y_max, total_h_with_stopper]
    pub flask3: [f32; 4],
    /// [solvent_cauchy_a, solvent_cauchy_b, glass_cauchy_a, glass_cauchy_b]
    pub optics: [f32; 4],
    /// [glass_tint_strength, light_azimuth_rad, card_distance, bench_albedo]
    pub scene_params0: [f32; 4],
    /// [exposure, caustic_half_extent, d_liq_max, d_glass_max]
    pub scene_params1: [f32; 4],
}

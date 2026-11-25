pub mod uniform;

use eframe::wgpu;

use crate::colorimetry::{
    bradford_cat_matrix, build_beer_lut, build_wl_importance_tables, cmf_1nm, illuminant_spd_1nm,
    illuminant_white_xyz, Illuminant, ToneMapper, BEER_LUT_GLASS_LEN, BEER_LUT_LIQ_LEN,
    D65_WHITE_XYZ,
};
use crate::flask::{FlaskGeometry, FlaskVolume, ProfileSegment, SolventPreset, StopperType};
use crate::spectrum::{AbsorbanceSpectrum, BaselineMode, SPECTRUM_LEN};

pub use uniform::{LightingPreset, SceneUniform, SpectralEntry};

pub const CAUSTIC_MAP_SIZE: u32 = 512;
pub const PHOTONS_PER_FRAME: u32 = 262_144;
const MAX_PROFILE_SEGS: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraState {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_deg: f32,
}

impl CameraState {
    pub fn preset_lab(flask: &FlaskGeometry) -> Self {
        Self {
            target: [0.0, 0.49 * flask.total_h, 0.0],
            yaw: 0.24,
            pitch: 0.23,
            distance: 2.40 * flask.total_h,
            fov_deg: 32.0,
        }
    }

    pub fn preset_eye_level(flask: &FlaskGeometry) -> Self {
        Self {
            target: [0.0, 0.50 * flask.total_h, 0.0],
            yaw: 0.05,
            pitch: 0.03,
            distance: 2.35 * flask.total_h,
            fov_deg: 32.0,
        }
    }

    pub fn preset_meniscus(flask: &FlaskGeometry) -> Self {
        Self {
            target: [0.0, flask.y_mark + 0.15, 0.0],
            yaw: 0.16,
            pitch: 0.06,
            distance: (9.5 * flask.r_neck_in).max(4.5),
            fov_deg: 28.0,
        }
    }

    pub fn preset_top(flask: &FlaskGeometry) -> Self {
        Self {
            target: [0.0, 0.22 * flask.total_h, 0.0],
            yaw: 0.25,
            pitch: 0.78,
            distance: 2.25 * flask.total_h,
            fov_deg: 34.0,
        }
    }

    /// Returns (pos, right, up, forward).
    pub fn basis(&self) -> ([f32; 3], [f32; 3], [f32; 3], [f32; 3]) {
        let cp = self.pitch.cos();
        let sp = self.pitch.sin();
        let sy = self.yaw.sin();
        let cy = self.yaw.cos();
        let offset = [
            self.distance * cp * sy,
            self.distance * sp,
            self.distance * cp * cy,
        ];
        let pos = [
            self.target[0] + offset[0],
            (self.target[1] + offset[1]).max(0.15),
            self.target[2] + offset[2],
        ];
        let forward = normalize3([
            self.target[0] - pos[0],
            self.target[1] - pos[1],
            self.target[2] - pos[2],
        ]);
        let world_up = [0.0, 1.0, 0.0];
        let right = normalize3(cross3(forward, world_up));
        let up = normalize3(cross3(right, forward));
        (pos, right, up, forward)
    }
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / len, v[1] / len, v[2] / len]
}

#[derive(Debug, Clone)]
pub struct SceneState {
    pub spectrum: AbsorbanceSpectrum,
    pub path_length_cm: f32,
    pub conc_scale: f32,
    pub baseline_mode: BaselineMode,
    pub manual_baseline: f32,

    pub volume: FlaskVolume,
    pub fill_fraction: f32,
    pub stopper_type: StopperType,
    pub enable_markings: bool,
    pub glass_tint_strength: f32,

    pub solvent: SolventPreset,
    pub custom_nd: f32,

    pub illuminant: Illuminant,
    pub lighting_preset: LightingPreset,
    pub light_azimuth_deg: f32,
    pub enable_white_card: bool,
    pub card_distance_cm: f32,
    pub bench_albedo: f32,

    pub enable_caustics: bool,
    pub enable_dispersion: bool,
    pub white_balance: bool,
    pub tone_mapper: ToneMapper,
    pub exposure: f32,
    pub max_spp: u32,

    pub camera: CameraState,
}

impl SceneState {
    pub fn new(spectrum: AbsorbanceSpectrum, flask: &FlaskGeometry) -> Self {
        Self {
            spectrum,
            path_length_cm: 1.0,
            conc_scale: 1.0,
            baseline_mode: BaselineMode::Raw,
            manual_baseline: 0.0,
            volume: flask.volume,
            fill_fraction: 1.0,
            stopper_type: StopperType::None,
            enable_markings: true,
            glass_tint_strength: 1.0,
            solvent: SolventPreset::Water,
            custom_nd: 1.333,
            illuminant: Illuminant::D65,
            lighting_preset: LightingPreset::LabStudio,
            light_azimuth_deg: 24.0,
            enable_white_card: false,
            card_distance_cm: (3.6 * flask.r_bulb_max).max(11.0),
            bench_albedo: 0.58,
            enable_caustics: true,
            enable_dispersion: false,
            white_balance: true,
            tone_mapper: ToneMapper::PbrNeutral,
            exposure: 1.0,
            max_spp: 256,
            camera: CameraState::preset_lab(flask),
        }
    }

    pub fn effective_baseline_mode(&self) -> BaselineMode {
        match self.baseline_mode {
            BaselineMode::Manual(_) => BaselineMode::Manual(self.manual_baseline),
            other => other,
        }
    }

    pub fn sigma_a_liquid(&self) -> [f32; SPECTRUM_LEN] {
        self.spectrum.effective_sigma_a(
            self.path_length_cm,
            self.conc_scale,
            self.effective_baseline_mode(),
        )
    }
}

/// Borosilicate 3.3 weak spectral absorption (cm^-1): slight green-aqua edge tint from Fe2+ trace.
fn glass_sigma_a_1nm() -> [f32; SPECTRUM_LEN] {
    let mut out = [0.0f32; SPECTRUM_LEN];
    for (i, slot) in out.iter_mut().enumerate() {
        let l = 380.0 + i as f32;
        let uv = (-((l - 365.0) / 18.0).powi(2)).exp() * 0.035;
        let red_tail = ((l - 610.0) / 140.0).clamp(0.0, 1.5).powi(2) * 0.018;
        *slot = 0.0025 + uv + red_tail;
    }
    out
}

pub struct SpectralRenderer {
    width: u32,
    height: u32,
    frame_index: u32,
    total_spp: u32,
    uploaded_label_volume: Option<FlaskVolume>,

    bgl1_pathtrace: wgpu::BindGroupLayout,
    bgl1_display: wgpu::BindGroupLayout,

    caustic_pipeline: wgpu::ComputePipeline,
    resolve_pipeline: wgpu::ComputePipeline,
    pathtrace_pipeline: wgpu::ComputePipeline,
    display_pipeline: wgpu::ComputePipeline,

    uniform_buf: wgpu::Buffer,
    spectral_buf: wgpu::Buffer,
    beer_lut_buf: wgpu::Buffer,
    outer_seg_buf: wgpu::Buffer,
    inner_seg_buf: wgpu::Buffer,
    stopper_seg_buf: wgpu::Buffer,
    caustic_splat_buf: wgpu::Buffer,
    caustic_accum_buf: wgpu::Buffer,
    accum_xyz_buf: wgpu::Buffer,

    label_tex: wgpu::Texture,

    display_tex: wgpu::Texture,
    display_view: wgpu::TextureView,

    bg0: wgpu::BindGroup,
    bg1_caustic: wgpu::BindGroup,
    bg1_resolve: wgpu::BindGroup,
    bg1_pathtrace: wgpu::BindGroup,
    bg1_display: wgpu::BindGroup,
}

impl SpectralRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) -> Self {
        let width = width.max(64);
        let height = height.max(64);

        let common_src = include_str!("shaders/common.wgsl");
        let make_module = |label: &str, body: &str| {
            let full = format!("{common_src}\n{body}");
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(full.into()),
            })
        };

        let caustic_mod = make_module("caustic", include_str!("shaders/caustic.wgsl"));
        let resolve_mod = make_module("caustic_resolve", include_str!("shaders/caustic_resolve.wgsl"));
        let pathtrace_mod = make_module("pathtrace", include_str!("shaders/pathtrace.wgsl"));
        let display_mod = make_module("display", include_str!("shaders/display.wgsl"));

        let bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl0_scene"),
            entries: &[
                bgl_entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                bgl_entry(1, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
                bgl_entry(2, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
                bgl_entry(3, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
                bgl_entry(4, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
                bgl_entry(5, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                bgl_entry(6, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                bgl_entry(7, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });

        let bgl1_caustic = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl1_caustic"),
            entries: &[
                bgl_entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });

        let bgl1_resolve = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl1_resolve"),
            entries: &[
                bgl_entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }),
                bgl_entry(1, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });

        let bgl1_pathtrace = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl1_pathtrace"),
            entries: &[
                bgl_entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }),
                bgl_entry(1, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });

        let bgl1_display = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl1_display"),
            entries: &[
                bgl_entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
                bgl_entry(1, wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2 }),
            ],
        });

        let make_pipeline = |label: &str, bgl1: &wgpu::BindGroupLayout, module: &wgpu::ShaderModule| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[&bgl0, bgl1],
                push_constant_ranges: &[],
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                module,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };

        let caustic_pipeline = make_pipeline("caustic_pipe", &bgl1_caustic, &caustic_mod);
        let resolve_pipeline = make_pipeline("resolve_pipe", &bgl1_resolve, &resolve_mod);
        let pathtrace_pipeline = make_pipeline("pathtrace_pipe", &bgl1_pathtrace, &pathtrace_mod);
        let display_pipeline = make_pipeline("display_pipe", &bgl1_display, &display_mod);

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform_buf"),
            size: std::mem::size_of::<SceneUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let spectral_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spectral_buf"),
            size: (SPECTRUM_LEN * std::mem::size_of::<SpectralEntry>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let beer_lut_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("beer_lut_buf"),
            size: (BEER_LUT_LIQ_LEN * BEER_LUT_GLASS_LEN * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let seg_buf_bytes = (MAX_PROFILE_SEGS * std::mem::size_of::<ProfileSegment>()) as u64;
        let outer_seg_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outer_segs"),
            size: seg_buf_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let inner_seg_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("inner_segs"),
            size: seg_buf_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stopper_seg_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stopper_segs"),
            size: seg_buf_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let caustic_texels = (CAUSTIC_MAP_SIZE * CAUSTIC_MAP_SIZE) as u64;
        let caustic_splat_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("caustic_splat"),
            size: caustic_texels * 3 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let caustic_accum_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("caustic_accum"),
            size: caustic_texels * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let accum_xyz_buf = create_accum_buf(device, width, height);

        let label_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("label_tex"),
            size: wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let label_view = label_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let label_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("label_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let (display_tex, display_view) = create_display_tex(device, width, height);

        let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg0"),
            layout: &bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: spectral_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: outer_seg_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: inner_seg_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: stopper_seg_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&label_view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&label_sampler) },
                wgpu::BindGroupEntry { binding: 7, resource: beer_lut_buf.as_entire_binding() },
            ],
        });

        let bg1_caustic = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg1_caustic"),
            layout: &bgl1_caustic,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: caustic_splat_buf.as_entire_binding() }],
        });
        let bg1_resolve = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg1_resolve"),
            layout: &bgl1_resolve,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: caustic_splat_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: caustic_accum_buf.as_entire_binding() },
            ],
        });
        let bg1_pathtrace = make_bg1_pathtrace(device, &bgl1_pathtrace, &accum_xyz_buf, &caustic_accum_buf);
        let bg1_display = make_bg1_display(device, &bgl1_display, &accum_xyz_buf, &display_view);

        // Zero buffers initially
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("init_clear") });
        enc.clear_buffer(&caustic_splat_buf, 0, None);
        enc.clear_buffer(&caustic_accum_buf, 0, None);
        enc.clear_buffer(&accum_xyz_buf, 0, None);
        queue.submit(Some(enc.finish()));

        Self {
            width,
            height,
            frame_index: 0,
            total_spp: 0,
            uploaded_label_volume: None,
            bgl1_pathtrace,
            bgl1_display,
            caustic_pipeline,
            resolve_pipeline,
            pathtrace_pipeline,
            display_pipeline,
            uniform_buf,
            spectral_buf,
            beer_lut_buf,
            outer_seg_buf,
            inner_seg_buf,
            stopper_seg_buf,
            caustic_splat_buf,
            caustic_accum_buf,
            accum_xyz_buf,
            label_tex,
            display_tex,
            display_view,
            bg0,
            bg1_caustic,
            bg1_resolve,
            bg1_pathtrace,
            bg1_display,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn total_spp(&self) -> u32 {
        self.total_spp
    }

    pub fn display_view(&self) -> &wgpu::TextureView {
        &self.display_view
    }

    pub fn resize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) {
        let width = width.max(64);
        let height = height.max(64);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.accum_xyz_buf = create_accum_buf(device, width, height);
        let (dtex, dview) = create_display_tex(device, width, height);
        self.display_tex = dtex;
        self.display_view = dview;
        self.bg1_pathtrace = make_bg1_pathtrace(device, &self.bgl1_pathtrace, &self.accum_xyz_buf, &self.caustic_accum_buf);
        self.bg1_display = make_bg1_display(device, &self.bgl1_display, &self.accum_xyz_buf, &self.display_view);
        self.reset_accumulation(device, queue);
    }

    pub fn reset_accumulation(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.frame_index = 0;
        self.total_spp = 0;
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("reset_accum") });
        enc.clear_buffer(&self.accum_xyz_buf, 0, None);
        enc.clear_buffer(&self.caustic_accum_buf, 0, None);
        enc.clear_buffer(&self.caustic_splat_buf, 0, None);
        queue.submit(Some(enc.finish()));
    }

    pub fn upload_scene(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, state: &SceneState, flask: &FlaskGeometry) {
        let sigma_liq = state.sigma_a_liquid();
        let spd = illuminant_spd_1nm(state.illuminant);
        let cmf = cmf_1nm();
        let sigma_glass = glass_sigma_a_1nm();
        let (wl_icdf, wl_pdf) = build_wl_importance_tables(state.illuminant);

        let mut spectral_entries = Vec::with_capacity(SPECTRUM_LEN);
        for i in 0..SPECTRUM_LEN {
            spectral_entries.push(SpectralEntry {
                data0: [sigma_liq[i], spd[i], cmf[i][0], cmf[i][1]],
                data1: [cmf[i][2], sigma_glass[i], wl_icdf[i], wl_pdf[i]],
            });
        }
        queue.write_buffer(&self.spectral_buf, 0, bytemuck::cast_slice(&spectral_entries));

        let (d_liq_max, d_glass_max) = lut_bounds(flask);
        let lut = build_beer_lut(
            &sigma_liq,
            &sigma_glass,
            state.glass_tint_strength,
            state.illuminant,
            d_liq_max,
            d_glass_max,
        );
        queue.write_buffer(&self.beer_lut_buf, 0, bytemuck::cast_slice(&lut));

        write_seg_buf(queue, &self.outer_seg_buf, &flask.outer_segs);
        write_seg_buf(queue, &self.inner_seg_buf, &flask.inner_segs);
        write_seg_buf(queue, &self.stopper_seg_buf, &flask.stopper_segs);

        if self.uploaded_label_volume != Some(flask.volume) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.label_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &flask.label_tex_data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(512),
                    rows_per_image: Some(512),
                },
                wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
            );
            self.uploaded_label_volume = Some(flask.volume);
        }

        self.reset_accumulation(device, queue);
    }

    pub fn update_display_only(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, state: &SceneState, flask: &FlaskGeometry) {
        let uniform = build_uniform(self.width, self.height, self.frame_index, 0, state, flask);
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniform));
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("display_only") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("display"), timestamp_writes: None });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, &self.bg0, &[]);
            pass.set_bind_group(1, &self.bg1_display, &[]);
            pass.dispatch_workgroups(self.width.div_ceil(8), self.height.div_ceil(8), 1);
        }
        queue.submit(Some(enc.finish()));
    }

    pub fn render_step(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, state: &SceneState, flask: &FlaskGeometry, spp_per_frame: u32) -> u32 {
        let uniform = build_uniform(self.width, self.height, self.frame_index, spp_per_frame, state, flask);
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniform));

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("spectral_step") });

        if state.enable_caustics {
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("caustic"), timestamp_writes: None });
                pass.set_pipeline(&self.caustic_pipeline);
                pass.set_bind_group(0, &self.bg0, &[]);
                pass.set_bind_group(1, &self.bg1_caustic, &[]);
                pass.dispatch_workgroups(PHOTONS_PER_FRAME.div_ceil(256), 1, 1);
            }
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("caustic_resolve"), timestamp_writes: None });
                pass.set_pipeline(&self.resolve_pipeline);
                pass.set_bind_group(0, &self.bg0, &[]);
                pass.set_bind_group(1, &self.bg1_resolve, &[]);
                pass.dispatch_workgroups(CAUSTIC_MAP_SIZE.div_ceil(8), CAUSTIC_MAP_SIZE.div_ceil(8), 1);
            }
        }

        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("pathtrace"), timestamp_writes: None });
            pass.set_pipeline(&self.pathtrace_pipeline);
            pass.set_bind_group(0, &self.bg0, &[]);
            pass.set_bind_group(1, &self.bg1_pathtrace, &[]);
            pass.dispatch_workgroups(self.width.div_ceil(8), self.height.div_ceil(8), 1);
        }

        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("display"), timestamp_writes: None });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, &self.bg0, &[]);
            pass.set_bind_group(1, &self.bg1_display, &[]);
            pass.dispatch_workgroups(self.width.div_ceil(8), self.height.div_ceil(8), 1);
        }

        queue.submit(Some(enc.finish()));
        self.frame_index += 1;
        self.total_spp += spp_per_frame;
        self.total_spp
    }

    pub fn readback_rgba(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Vec<u8>, String> {
        let bytes_per_pixel = 4u32;
        let unpadded_bpr = self.width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bpr = unpadded_bpr.div_ceil(align) * align;

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback_staging"),
            size: (padded_bpr * self.height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("copy_display") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.display_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        queue.submit(Some(enc.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| format!("Readback channel failed: {e}"))?
            .map_err(|e| format!("GPU buffer map failed: {e:?}"))?;

        let mapped = slice.get_mapped_range();
        let mut rgba = vec![0u8; (unpadded_bpr * self.height) as usize];
        for y in 0..self.height as usize {
            let src_off = y * padded_bpr as usize;
            let dst_off = y * unpadded_bpr as usize;
            rgba[dst_off..dst_off + unpadded_bpr as usize]
                .copy_from_slice(&mapped[src_off..src_off + unpadded_bpr as usize]);
        }
        drop(mapped);
        staging.unmap();
        Ok(rgba)
    }

    pub fn render_headless_png(
        state: &SceneState,
        flask: &FlaskGeometry,
        width: u32,
        height: u32,
        target_spp: u32,
        out_path: &std::path::Path,
    ) -> Result<(), String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "No GPU adapter found for headless rendering.".to_string())?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("headless_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| format!("Failed to create GPU device: {e}"))?;

        let mut renderer = Self::new(&device, &queue, width, height);
        renderer.upload_scene(&device, &queue, state, flask);

        let spp_batch = 16u32.min(target_spp.max(1));
        while renderer.total_spp() < target_spp {
            let rem = target_spp - renderer.total_spp();
            renderer.render_step(&device, &queue, state, flask, rem.min(spp_batch));
            device.poll(wgpu::Maintain::Wait);
        }

        let rgba = renderer.readback_rgba(&device, &queue)?;
        let img = image::RgbaImage::from_raw(width, height, rgba)
            .ok_or_else(|| "Invalid image dimensions for PNG export.".to_string())?;
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Could not create directory '{}': {e}", parent.display()))?;
            }
        }
        img.save(out_path)
            .map_err(|e| format!("Failed to save PNG '{}': {e}", out_path.display()))?;
        Ok(())
    }
}

fn bgl_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    }
}

fn create_accum_buf(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("accum_xyz"),
        size: (width as u64) * (height as u64) * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_display_tex(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("display_tex"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

fn make_bg1_pathtrace(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    accum_xyz: &wgpu::Buffer,
    caustic_accum: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg1_pathtrace"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: accum_xyz.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: caustic_accum.as_entire_binding() },
        ],
    })
}

fn make_bg1_display(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    accum_xyz: &wgpu::Buffer,
    display_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg1_display"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: accum_xyz.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(display_view) },
        ],
    })
}

fn write_seg_buf(queue: &wgpu::Queue, buf: &wgpu::Buffer, segs: &[ProfileSegment]) {
    let mut padded = vec![
        ProfileSegment {
            y0: 0.0,
            y1: 0.0,
            r_min: 0.0,
            r_max: 0.0,
            c0: 0.0,
            c1: 0.0,
            c2: 0.0,
            c3: 0.0,
        };
        MAX_PROFILE_SEGS
    ];
    let n = segs.len().min(MAX_PROFILE_SEGS);
    padded[..n].copy_from_slice(&segs[..n]);
    queue.write_buffer(buf, 0, bytemuck::cast_slice(&padded));
}

fn lut_bounds(flask: &FlaskGeometry) -> (f32, f32) {
    ((6.0 * flask.total_h).max(45.0), 12.0)
}

fn build_uniform(
    width: u32,
    height: u32,
    frame_index: u32,
    spp_per_frame: u32,
    state: &SceneState,
    flask: &FlaskGeometry,
) -> SceneUniform {
    let (cam_pos, cam_right, cam_up, cam_fwd) = state.camera.basis();
    let fov_tan = (0.5 * state.camera.fov_deg.to_radians()).tan();
    let aspect = width as f32 / height.max(1) as f32;

    let src_white = illuminant_white_xyz(state.illuminant);
    let cat = bradford_cat_matrix(src_white, D65_WHITE_XYZ);

    let men = flask.solve_meniscus(state.fill_fraction, state.solvent.capillary_length_cm());
    let (sol_a, sol_b) = state.solvent.cauchy_ab(state.custom_nd);
    // Borosilicate 3.3 glass: n_D = 1.473, Abbe ~65 -> Cauchy A=1.4596, B=0.00465
    let glass_a = 1.4596_f32;
    let glass_b = 0.00465_f32;

    let total_h_with_stopper = if state.stopper_type == StopperType::None {
        flask.total_h
    } else {
        flask.stopper_segs.last().map(|s| s.y1).unwrap_or(flask.total_h)
    };
    let caustic_half_extent = (2.4 * flask.total_h).max(16.0);

    SceneUniform {
        cam_pos: [cam_pos[0], cam_pos[1], cam_pos[2], fov_tan],
        cam_right: [cam_right[0], cam_right[1], cam_right[2], aspect],
        cam_up: [cam_up[0], cam_up[1], cam_up[2], 0.0],
        cam_forward: [cam_fwd[0], cam_fwd[1], cam_fwd[2], 0.0],

        bradford_row0: [cat[0][0], cat[0][1], cat[0][2], 0.0],
        bradford_row1: [cat[1][0], cat[1][1], cat[1][2], 0.0],
        bradford_row2: [cat[2][0], cat[2][1], cat[2][2], 0.0],

        dims: [width, height, frame_index, spp_per_frame],
        counts: [
            flask.outer_segs.len().min(MAX_PROFILE_SEGS) as u32,
            flask.inner_segs.len().min(MAX_PROFILE_SEGS) as u32,
            flask.stopper_segs.len().min(MAX_PROFILE_SEGS) as u32,
            (frame_index + 1) * PHOTONS_PER_FRAME,
        ],
        flags: [
            u32::from(men.fill_active),
            state.stopper_type as u32,
            u32::from(state.enable_markings),
            u32::from(state.enable_dispersion),
        ],
        render_flags: [
            u32::from(state.enable_caustics),
            state.lighting_preset as u32,
            u32::from(state.enable_white_card),
            state.tone_mapper as u32,
        ],
        render_flags2: [
            u32::from(state.white_balance),
            CAUSTIC_MAP_SIZE,
            PHOTONS_PER_FRAME,
            0,
        ],

        flask0: [flask.y_mark, men.y_bot, men.y_contact, men.r_wall],
        flask1: [men.h_men, men.l_c, flask.total_h, flask.base_t],
        flask2: [flask.r_base_flat, flask.r_inner_flat, flask.r_rim_in, flask.r_rim_out],
        flask3: [flask.r_bulb_max, flask.label_y_min, flask.label_y_max, total_h_with_stopper],
        optics: [sol_a, sol_b, glass_a, glass_b],
        scene_params0: [
            state.glass_tint_strength,
            state.light_azimuth_deg.to_radians(),
            state.card_distance_cm,
            state.bench_albedo,
        ],
        scene_params1: [state.exposure, caustic_half_extent, lut_bounds(flask).0, lut_bounds(flask).1],
    }
}

pub fn save_rgba_png(path: &std::path::Path, width: u32, height: u32, rgba: Vec<u8>) -> Result<(), String> {
    let img = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "Invalid RGBA buffer size for PNG export.".to_string())?;
    img.save(path)
        .map_err(|e| format!("Failed to save PNG '{}': {e}", path.display()))?;
    Ok(())
}

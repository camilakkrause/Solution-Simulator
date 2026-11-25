# SolutionSimulator

A GPU-accelerated, physically based spectral ray-tracing desktop application written in Rust (`wgpu` + `eframe`/`egui`). **SolutionSimulator** reads a UV-Vis absorbance spectrum (`.txt`) and renders a 3D ISO 1042 Class A volumetric flask with accurate Beer–Lambert solution color, path-length transparency, glass/liquid refraction and reflection, concave capillary meniscus, etched markings, and spectral bench caustics.

<img width="1362" height="892" alt="image" src="https://github.com/user-attachments/assets/6b4a8bd0-0a60-4183-956d-369fc83197f8" />

---

## Features & Optical Physics

- **True 1 nm Spectral Beer–Lambert Attenuation**
  - Attenuates each wavelength independently along every ray segment inside the liquid and borosilicate glass:
    $$T(\lambda, d) = 10^{-A(\lambda) \cdot s \cdot d / l_0}$$
  - Naturally reproduces path-length color depth and **dichromatism** between the narrow flask neck and wide bulb (e.g., Bromophenol Blue shifting from cyan-teal in the neck to ruby-red in the bulb).
  - Pre-integrates all 401 1-nm wavelengths (`380–780 nm`) into a quadratically spaced $512 \times 16$ `(d_liquid, d_glass)` lookup table for completely noise-free rendering, with an optional toggle for **Cauchy chromatic dispersion** driven by CMF-importance Golden-Ratio quasi-Monte Carlo sampling.
- **Analytic ISO 1042 Class A Volumetric Flasks**
  - Supports **10, 25, 50, 100, 250, 500, and 1000 mL** nominal volumes using smooth Fritsch–Carlson $C^1$ cubic Hermite revolution surfaces (zero triangle faceting).
  - Exact numerical volume integration calibrates the etched graduation ring so `100%` fill places the bottom of the **Young–Laplace concave capillary meniscus** tangent to the mark.
  - Includes optional **Pennyhead Glass** (frosted ground-glass plug) or **PTFE** stoppers and procedural white enamel markings (`mL`, Class A tolerance, `20 °C`, `BORO 3.3`).
- **Nested Dielectrics & Spectral Caustics**
  - Models `Air ↔ Borosilicate 3.3 Glass ↔ Solvent` boundaries (eliminating fake air-gap reflections below the meniscus).
  - Real-time progressive `512 × 512` spectral caustic photon map (`262,144` photons/frame) projecting colored caustics onto the lab bench.
- **Colorimetry, Studio Lighting & Analysis UI**
  - **CIE 1931 2°** colorimetry with selectable CIE illuminants (`D65`, `D50`, `A`, `FL2`, `5000K Lab LED`, `E`), optional Bradford chromatic adaptation white balance, luminance-preserving gamut mapping, and `PBR Neutral`, `Linear`, or `ACES` tone mapping.
  - Interactive `Absorbance A(λ)` / `Transmittance %T(λ)` plot with spectral axis, side-by-side **1 cm cuvette vs. flask bulb** CIE `xy` / `L*a*b*` / hex color swatches, 4 studio lighting presets, and a toggleable **white contrast card** behind the flask.
  - Built-in demo presets (`Thiaz-like Amber Dye`, `KMnO4`, `Bromophenol Blue`, `CuSO4`, `NiSO4`, `Pure Water`).

---

## Input Spectrum Format (`.txt`)

The parser skips the **first 2 header lines** and reads two numeric columns (`wavelength_nm`, `absorbance`):

```text
"RawData - File_260709_175714.spc"
"Wavelength nm."	"Abs."
380.00	0.0898
381.00	0.0923
382.00	0.0934
```

- Supports tabs, spaces, semicolons, or commas as column separators, and `.` or `,` decimal separators.
- Automatically sorts descending wavelengths, resamples onto `380–780 nm` at 1 nm steps (clamping outside source bounds), and computes the `700–780 nm` median for optional **Auto NIR Zero** baseline correction.

---

## Requirements & Quick Start

### Option A: Download Prebuilt Binary (Windows x64)
1. Download `SolutionSimulator-v1.0.0-windows-x64.zip` from the [**Releases**](../../releases) page.
2. Extract the `.zip` and double-click `solution_simulator.exe` (if Windows SmartScreen appears, click **More info → Run anyway**).

### Option B: Build from Source
- **Rust** `>= 1.87` (`rustup` / `cargo`)
- A **DirectX 12**, **Vulkan**, or **Metal** compatible GPU

```bash
# Clone and build in release mode
git clone https://github.com/<your-username>/SolutionSimulator.git
cd SolutionSimulator
cargo run --release
```

Optionally pass a spectrum `.txt` file or built-in preset name at launch:

```bash
cargo run --release -- path/to/spectrum.txt
```

Inside the app, you can **drag-and-drop** any `.txt` file onto the window, click **Open .txt File...**, or click **Save PNG...** to export the converged render.

### Viewport Controls

- **Left-drag**: Orbit camera
- **Right-drag** or **Shift + Left-drag**: Pan
- **Mouse wheel**: Zoom
- **Double-click**: Reset camera to default `3/4 Lab` view

---

## Headless CLI Rendering

You can also render PNGs directly from the command line without opening the GUI:

```bash
cargo run --release -- --render path/to/spectrum.txt --out render.png \
  --width 1024 --height 1024 --spp 256 --volume 100 \
  --baseline nir --preset studio --camera lab --caustics on --white-card off
```

Run `--help` to see all CLI flags (`--fill`, `--conc`, `--path-cm`, `--stopper`, `--dispersion`, `--exposure`, etc.).

---

## Project Structure

```text
SolutionSimulator/
├── .cargo/config.toml        # MSRV-aware Cargo resolver setting
├── .gitignore
├── Cargo.toml
├── Cargo.lock
├── README.md
└── src/
    ├── main.rs               # Entry point, CLI headless runner, unit tests
    ├── spectrum.rs           # .txt spectrum parser, baseline modes, presets
    ├── colorimetry.rs        # CIE 1931 CMFs, illuminants, Bradford CAT, Beer LUT
    ├── flask.rs              # ISO 1042 profiles, volume calibration, meniscus, labels
    ├── ui.rs                 # egui viewport, spectrum plot, color swatches, controls
    └── gpu/
        ├── mod.rs            # wgpu compute pipeline & progressive accumulation
        ├── uniform.rs        # 16-byte-aligned GPU uniform & spectral structs
        └── shaders/
            ├── common.wgsl           # Analytic cubic-Hermite & meniscus ray intersections
            ├── caustic.wgsl          # Importance-sampled spectral caustic photon tracing
            ├── caustic_resolve.wgsl  # Fixed-point atomic splat accumulation resolve
            ├── pathtrace.wgsl        # Spectral LUT & dispersive HWSS path tracer
            └── display.wgsl          # Bradford CAT, gamut mapping & tone mapping
```

---

## Running Tests

```bash
cargo test
```

//! Explicitly invoked tests requiring a Vulkan GPU.

use greign_gpu::{BayerPattern, GpuRuntime, Primitive, ShaderParams};
use greign_pipeline::{CfaPattern, NormalizedMosaic, bilinear_demosaic};
use greign_raw::{Mosaic, Orientation, RawMetadata};
use std::time::Instant;

const GIB: u64 = 1024 * 1024 * 1024;

fn fixture(width: u32, height: u32) -> Vec<[f32; 4]> {
    let count = usize::try_from(width * height).expect("fixture dimensions");
    (0..count)
        .map(|index| {
            let value =
                f32::from(u16::try_from(index % 257).expect("small fixture value")) / 128.0 - 0.5;
            [value, value * 0.75 + 0.1, 1.25 - value, 1.0]
        })
        .collect()
}

fn assert_parity(runtime: &mut GpuRuntime, primitive: Primitive) {
    let (width, height) = (17, 33);
    let input = fixture(width, height);
    let params = ShaderParams::new(width, height, primitive).expect("params");
    let expected: Vec<_> = input
        .iter()
        .copied()
        .map(|pixel| params.apply_reference(pixel))
        .collect();
    let actual = runtime
        .execute_readback(width, height, primitive, &input)
        .expect("GPU execution");
    for (expected, actual) in expected.iter().zip(actual) {
        for channel in 0..4 {
            let tolerance = 1.0e-5_f32.max(expected[channel].abs() * 1.0e-5);
            assert!(
                (expected[channel] - actual[channel]).abs() <= tolerance,
                "expected {expected:?}, actual {actual:?}"
            );
        }
    }
}

#[test]
#[ignore = "requires an explicitly permitted Vulkan GPU"]
fn all_primitives_match_cpu_on_non_multiple_dimensions() {
    let mut runtime = GpuRuntime::initialize(2 * GIB).expect("GPU initialization");
    assert_parity(
        &mut runtime,
        Primitive::Normalize {
            black: [0.1, 0.2, 0.3],
            white: [1.1, 1.3, 1.5],
        },
    );
    assert_parity(&mut runtime, Primitive::WhiteBalance([2.0, 1.0, 1.5]));
    assert_parity(&mut runtime, Primitive::Exposure(1.25));
    assert_parity(&mut runtime, Primitive::ToneCurve);
    assert_parity(
        &mut runtime,
        Primitive::ColorMatrix([[1.1, -0.1, 0.05], [-0.05, 1.0, 0.1], [0.02, -0.2, 1.2]]),
    );
    assert_eq!(runtime.memory_report().used_bytes, 0);
}

#[test]
#[ignore = "requires an explicitly permitted Vulkan GPU"]
fn repeated_device_initialization_succeeds() {
    for _ in 0..3 {
        let runtime = GpuRuntime::initialize(2 * GIB).expect("GPU initialization");
        eprintln!("{:?}", runtime.capability_report());
        assert!(!runtime.capability_report().adapter_name.is_empty());
    }
}

#[test]
#[ignore = "requires an explicitly permitted Vulkan GPU"]
fn resident_texture_never_reads_back_and_releases_budget_on_drop() {
    let (width, height) = (257, 129);
    let input = fixture(width, height);
    let mut runtime = GpuRuntime::initialize(2 * GIB).expect("GPU initialization");
    let texture = runtime
        .execute_resident(width, height, Primitive::ToneCurve, &input)
        .expect("resident texture");
    assert_eq!((texture.width(), texture.height()), (width, height));
    assert!(runtime.memory_report().used_bytes > 0);
    std::hint::black_box(texture.texture());
    std::hint::black_box(texture.view());
    drop(texture);
    assert_eq!(runtime.memory_report().used_bytes, 0);
}

#[test]
#[ignore = "requires an explicitly permitted Vulkan GPU"]
fn resident_timestamp_benchmark_beats_cpu_reference() {
    let (width, height) = (1_920, 1_080);
    let input = fixture(width, height);
    let primitive =
        Primitive::ColorMatrix([[1.1, -0.1, 0.05], [-0.05, 1.0, 0.1], [0.02, -0.2, 1.2]]);
    let params = ShaderParams::new(width, height, primitive).expect("params");
    let mut runtime = GpuRuntime::initialize(2 * GIB).expect("GPU initialization");
    let mut timing = runtime
        .benchmark_resident(width, height, primitive, &input, 20)
        .expect("timestamp benchmark");
    timing.kernel_ms.sort_by(f64::total_cmp);
    let gpu_p50 = timing.kernel_ms[timing.kernel_ms.len() / 2];

    let mut cpu_ms = Vec::with_capacity(20);
    for _ in 0..20 {
        let start = Instant::now();
        for pixel in &input {
            std::hint::black_box(params.apply_reference(*pixel));
        }
        cpu_ms.push(start.elapsed().as_secs_f64() * 1_000.0);
    }
    cpu_ms.sort_by(f64::total_cmp);
    let cpu_p50 = cpu_ms[cpu_ms.len() / 2];
    eprintln!(
        "adapter={:?} gpu_kernel_p50_ms={gpu_p50:.4} cpu_p50_ms={cpu_p50:.4} gpu_wall_ms={:.4}",
        runtime.capability_report(),
        timing.wall_ms
    );
    assert!(
        gpu_p50 < cpu_p50,
        "GPU kernel p50 {gpu_p50} ms is not faster than CPU {cpu_p50} ms"
    );
}

#[test]
#[ignore = "requires an explicitly permitted Vulkan GPU"]
fn bilinear_demosaic_matches_cpu_for_every_bayer_layout() {
    let (width, height) = (17, 33);
    let samples: Vec<f32> = (0..width * height)
        .map(|index| f32::from(u16::try_from(index % 101).expect("small fixture value")) / 100.0)
        .collect();
    let layouts = [
        ("RGGB", BayerPattern::Rggb),
        ("BGGR", BayerPattern::Bggr),
        ("GRBG", BayerPattern::Grbg),
        ("GBRG", BayerPattern::Gbrg),
    ];
    let mut runtime = GpuRuntime::initialize(2 * GIB).expect("GPU initialization");
    for (cfa, pattern) in layouts {
        let mosaic = NormalizedMosaic {
            width,
            height,
            cfa: CfaPattern::parse(cfa).expect("Bayer pattern"),
            samples: samples.clone(),
        };
        let expected = bilinear_demosaic(&mosaic).expect("CPU demosaic");
        let actual = runtime
            .demosaic_readback(width, height, pattern, &samples)
            .expect("GPU demosaic");
        for (expected, actual) in expected.pixels.iter().zip(actual) {
            for channel in 0..3 {
                assert!(
                    (expected[channel] - actual[channel]).abs() <= 1.0e-6,
                    "layout={cfa} expected={expected:?} actual={actual:?}"
                );
            }
            assert!((actual[3] - 1.0).abs() <= f32::EPSILON);
        }
    }
    assert_eq!(runtime.memory_report().used_bytes, 0);
}

#[test]
#[ignore = "requires an explicitly permitted Vulkan GPU"]
fn fused_quality_render_matches_cpu_within_quantization_tolerance() {
    let (width, height) = (33_u32, 27_u32);
    let samples = (0..width * height)
        .map(|index| {
            // Include values below black level: the second PPG pass must
            // consume the same clamped known samples as the CPU.
            let value = (index * 37) % 3_656;
            u16::try_from(value).expect("12-bit fixture")
        })
        .collect();
    let mosaic = Mosaic {
        metadata: RawMetadata {
            backend: "test",
            make: "Test".into(),
            model: "Bayer".into(),
            mode: "raw".into(),
            width,
            height,
            components_per_pixel: 1,
            bits_per_component: 12,
            orientation: Orientation::Rotate90,
            active_area: None,
            crop_area: None,
            black_levels: vec![256.0; 4],
            white_levels: vec![4_095.0],
            white_balance: [1.8, 1.0, 1.4],
            cfa_pattern: Some("RGGB".into()),
            xyz_to_camera_d65: [
                [1.0424, -0.3138, -0.13],
                [-0.4221, 1.1938, 0.2584],
                [-0.0547, 0.1658, 0.6183],
            ],
        },
        samples,
    };
    let settings = greign_pipeline::QualitySettings::default();
    let expected = greign_pipeline::develop_quality(&mosaic, settings)
        .expect("CPU quality")
        .to_rgb8();
    let mut runtime = GpuRuntime::initialize(2 * GIB).expect("GPU initialization");
    let (actual, timing) = runtime
        .develop_quality_readback(&mosaic, settings)
        .expect("GPU quality");
    assert_eq!((actual.width, actual.height), (height, width));
    assert_eq!(actual.pixels.len(), expected.len());
    for (index, (actual, expected)) in actual.pixels.iter().zip(expected).enumerate() {
        assert!(
            i16::from(*actual).abs_diff(i16::from(expected)) <= 2,
            "byte {index}: gpu={actual} cpu={expected} timing={timing:?}"
        );
    }
    eprintln!("quality_timing={timing:?}");
    assert_eq!(runtime.memory_report().used_bytes, 0);
    let (resident, host_submit_ms) = runtime
        .develop_quality_resident(&mosaic, settings)
        .expect("resident GPU quality");
    assert_eq!((resident.width(), resident.height()), (height, width));
    assert!(host_submit_ms > 0.0);
    std::hint::black_box(resident.texture());
    std::hint::black_box(resident.view());
    assert!(runtime.memory_report().used_bytes > 0);
    drop(resident);
    assert_eq!(runtime.memory_report().used_bytes, 0);
    let (prepared, prepare_submit_ms) = runtime
        .prepare_quality_resident(&mosaic, settings)
        .expect("prepared GPU quality");
    let (edited, edit_submit_ms) = runtime
        .render_prepared_quality(
            &prepared,
            greign_pipeline::QualitySettings {
                exposure_ev: 0.5,
                ..settings
            },
        )
        .expect("edited GPU quality");
    eprintln!("prepare_submit_ms={prepare_submit_ms:.4} edit_submit_ms={edit_submit_ms:.4}");
    assert_eq!((edited.width(), edited.height()), (height, width));
    drop(edited);
    drop(prepared);
    assert_eq!(runtime.memory_report().used_bytes, 0);
}

#[test]
#[ignore = "requires an explicitly permitted Vulkan GPU"]
fn clipped_regions_match_cpu_for_all_bayer_phases_and_odd_crop() {
    let mut runtime = GpuRuntime::initialize(2 * GIB).expect("GPU");
    for pattern in ["RGGB", "BGGR", "GRBG", "GBRG"] {
        let (width, height) = (194_u32, 146_u32);
        let cfa = CfaPattern::parse(pattern).unwrap();
        let samples = (0..width * height)
            .map(|i| {
                let x = i % width;
                let y = i / width;
                if (48..144).contains(&x) && (32..112).contains(&y) {
                    4095
                } else {
                    match cfa.color_at(x, y) {
                        greign_pipeline::CfaColor::Red => 3200,
                        greign_pipeline::CfaColor::Green => 3000,
                        greign_pipeline::CfaColor::Blue => 1700,
                    }
                }
            })
            .collect();
        let mosaic = Mosaic {
            metadata: RawMetadata {
                backend: "test",
                make: "Test".into(),
                model: "Clipped Bayer".into(),
                mode: "raw".into(),
                width,
                height,
                components_per_pixel: 1,
                bits_per_component: 12,
                orientation: Orientation::Rotate270,
                active_area: None,
                crop_area: Some(greign_raw::ActiveArea {
                    x: 1,
                    y: 1,
                    width: 191,
                    height: 143,
                }),
                black_levels: vec![256.0; 4],
                white_levels: vec![4095.0],
                white_balance: [1.8, 1.0, 1.3],
                cfa_pattern: Some(pattern.into()),
                xyz_to_camera_d65: [
                    [1.0424, -0.3138, -0.13],
                    [-0.4221, 1.1938, 0.2584],
                    [-0.0547, 0.1658, 0.6183],
                ],
            },
            samples,
        };
        for strength in [0.0, 0.5, 1.0] {
            let settings = greign_pipeline::QualitySettings {
                chroma_denoise: strength,
                ..greign_pipeline::QualitySettings::default()
            };
            let expected = greign_pipeline::develop_quality(&mosaic, settings)
                .unwrap()
                .to_rgb8();
            let (actual, _) = runtime.develop_quality_readback(&mosaic, settings).unwrap();
            for (i, (a, b)) in actual.pixels.iter().zip(expected).enumerate() {
                assert!(
                    a.abs_diff(b) <= 2,
                    "{pattern} denoise={strength} byte={i}: {a} != {b}"
                );
            }
        }
    }
}

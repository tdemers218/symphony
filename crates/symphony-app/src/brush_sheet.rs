//! Repeatable opacity and spacing probes for imported brush presets.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use image::{GrayImage, Luma};
use symphony_brushes::Brush;
use symphony_core::id::Id;
use symphony_core::math::Affine;
use symphony_core::stroke::{Point, Stroke, StrokeKind, Texture};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 480;
const START: u32 = 100;
const END: u32 = 800;
const TAP_Y: u32 = 80;
const STEADY_Y: u32 = 240;
const RAMP_Y: u32 = 400;
const DIAMETER: f32 = 64.0;
const STEADY_PRESSURE: f32 = 1.0;
const MIN_PRESSURE: f32 = 0.05;

fn texture(image: &GrayImage) -> Texture {
    Texture {
        width: image.width(),
        height: image.height(),
        pixels: image.as_raw().clone(),
    }
}

fn point(x: u32, y: u32, pressure: f32) -> Point {
    Point {
        x: x as f32 + 0.5,
        y: y as f32 + 0.5,
        pressure,
        width_offset: 0.0,
        pen: Default::default(),
    }
}

fn sheet(brush: &Brush) -> GrayImage {
    let settings = super::settings(brush, None, None);
    let tex = settings.texture.unwrap();
    let tip = brush.tip.as_ref().map(texture);
    let grain = brush.grain.as_ref().map(texture);
    let mut image = GrayImage::from_pixel(WIDTH, HEIGHT, Luma([255]));
    for points in [
        vec![point(START, TAP_Y, STEADY_PRESSURE)],
        vec![
            point(START, STEADY_Y, STEADY_PRESSURE),
            point(END, STEADY_Y, STEADY_PRESSURE),
        ],
        vec![
            point(START, RAMP_Y, MIN_PRESSURE),
            point(END, RAMP_Y, STEADY_PRESSURE),
        ],
    ] {
        // Standardize nominal diameter for comparisons and keep the
        // imported ratio between minimum and maximum width.
        let stroke = Stroke {
            id: Id(7),
            kind: StrokeKind::Brush,
            art: Default::default(),
            swatch: Id(0),
            min_width: DIAMETER * settings.min_width / settings.max_width,
            max_width: DIAMETER,
            points,
            tips: Default::default(),
            texture: Some(tex),
            shape: Vec::new(),
        };
        if let Some(c) = symphony_vector::stamp::rasterize(
            &stroke,
            &tex,
            tip.as_ref(),
            grain.as_ref(),
            Affine::IDENTITY,
            [0, 0, WIDTH as i32, HEIGHT as i32],
        ) {
            for (i, a) in c.data.into_iter().enumerate() {
                let x = c.x as u32 + i as u32 % c.width;
                let y = c.y as u32 + i as u32 / c.width;
                image.put_pixel(x, y, Luma([255 - a]));
            }
        }
    }
    image
}

fn coverage(image: &GrayImage, x: u32, y: u32) -> u8 {
    255 - image.get_pixel(x, y)[0]
}

/// Ignore shared white background so large margins cannot hide differences.
/// This assumes a black stroke on an opaque white background.
fn coverage_error(actual: &GrayImage, reference: &GrayImage, y0: u32, y1: u32) -> Option<f32> {
    let mut delta = 0u64;
    let mut pixels = 0u32;
    for y in y0..y1 {
        for x in 0..WIDTH {
            let a = coverage(actual, x, y);
            let b = coverage(reference, x, y);
            if a != 0 || b != 0 {
                delta += u64::from(a.abs_diff(b));
                pixels += 1;
            }
        }
    }
    (pixels > 0).then(|| delta as f32 / pixels as f32)
}

#[test]
#[ignore = "requires an external brush file and output directory"]
fn brush_sheet() {
    let input = PathBuf::from(
        std::env::var_os("SYMPHONY_BRUSH_SHEET_IN").expect("set SYMPHONY_BRUSH_SHEET_IN"),
    );
    let out = PathBuf::from(
        std::env::var_os("SYMPHONY_BRUSH_SHEET_OUT").expect("set SYMPHONY_BRUSH_SHEET_OUT"),
    );
    let reference = std::env::var_os("SYMPHONY_BRUSH_SHEET_REFERENCE").map(PathBuf::from);
    std::fs::create_dir_all(&out).unwrap();
    if let Some(reference) = &reference {
        assert_ne!(
            out.canonicalize().unwrap(),
            reference.canonicalize().unwrap(),
            "reference and output must be separate directories"
        );
    }
    let got = symphony_brushes::import(&input).unwrap();
    for problem in got.problems {
        eprintln!("{problem}");
    }
    let mut report = String::from("image\tsource\tnative_diameter\tspacing\tflow\ttap\tsteady_mid\tsteady_min\tsteady_max\tramp_25\tramp_50\tramp_75\ttap_error\tsteady_error\tramp_error\n");
    let mut rendered = 0;
    for (index, brush) in got.brushes.iter().enumerate() {
        if brush.eraser {
            continue;
        }
        let name: String = brush
            .name
            .chars()
            .take(60)
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let file = format!("{:04}-{name}.png", index + 1);
        let image = sheet(brush);
        write_metrics(&mut report, &file, brush, &image, reference.as_deref());
        image.save(out.join(&file)).unwrap();
        rendered += 1;
    }
    assert!(
        rendered != 0,
        "no paint brushes found in {}",
        input.display()
    );
    std::fs::write(out.join("metrics.tsv"), report).unwrap();
}

fn write_metrics(
    report: &mut String,
    file: &str,
    brush: &Brush,
    image: &GrayImage,
    reference: Option<&Path>,
) {
    let mid = (START + END) / 2;
    write!(
        report,
        "{file}\t{}\t{:.3}\t{:.4}\t{:.4}\t{}\t{}",
        brush.source.title(),
        brush.diameter,
        brush.spacing,
        brush.flow,
        coverage(image, START, TAP_Y),
        coverage(image, mid, STEADY_Y)
    )
    .unwrap();
    let steady = (START + 64..=END - 64).map(|x| coverage(image, x, STEADY_Y));
    write!(
        report,
        "\t{}\t{}",
        steady.clone().min().unwrap(),
        steady.max().unwrap()
    )
    .unwrap();
    for step in 1..=3 {
        write!(
            report,
            "\t{}",
            coverage(image, START + (END - START) * step / 4, RAMP_Y)
        )
        .unwrap();
    }
    if let Some(path) = reference.map(|dir| dir.join(file)).filter(|p| p.exists()) {
        let expected = image::open(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .to_luma8();
        assert_eq!(
            expected.dimensions(),
            (WIDTH, HEIGHT),
            "{}: reference dimensions",
            path.display()
        );
        for (y0, y1) in [(0, 160), (160, 320), (320, HEIGHT)] {
            if let Some(error) = coverage_error(image, &expected, y0, y1) {
                write!(report, "\t{error:.3}").unwrap();
            } else {
                report.push('\t');
            }
        }
    } else {
        report.push_str("\t\t\t");
    }
    report.push('\n');
}

#[test]
fn imported_pressure_settings_survive_rendering() {
    let imported = symphony_brushes::import_bytes(
        "myb",
        "Pressure probe",
        br#"{
        "settings":{
            "radius_logarithmic":{"base_value":3},
            "opaque":{"base_value":0.8},
            "opaque_multiply":{"base_value":0,"inputs":{"pressure":[[0,0],[1,1]]}},
            "dabs_per_actual_radius":{"base_value":4}
        }
    }"#,
    )
    .unwrap();
    let image = sheet(&imported.brushes[0]);
    let midpoint = (START + END) / 2;
    assert!(coverage(&image, START, TAP_Y).abs_diff(204) <= 1);
    assert!(coverage(&image, midpoint, STEADY_Y).abs_diff(204) <= 1);
    let ramp = [1, 2, 3].map(|i| coverage(&image, START + (END - START) * i / 4, RAMP_Y));
    assert!(ramp[0] < ramp[1] && ramp[1] < ramp[2], "{ramp:?}");
    assert!(coverage(&image, midpoint, 160) == 0);
    assert_eq!(coverage_error(&image, &image, 0, HEIGHT), Some(0.0));
    let white = GrayImage::from_pixel(WIDTH, HEIGHT, Luma([255]));
    assert!(coverage_error(&image, &white, 0, HEIGHT).unwrap() > 100.0);
}

#[test]
fn low_flow_builds_up_without_exceeding_stroke_opacity() {
    let mut imported = symphony_brushes::import_bytes(
        "myb",
        "Flow probe",
        br#"{"settings":{"opaque":{"base_value":0.7}}}"#,
    )
    .unwrap();
    let brush = &mut imported.brushes[0];
    brush.flow = 0.1;
    brush.spacing = 0.125;

    let image = sheet(brush);
    let tap = coverage(&image, START, TAP_Y);
    let steady = coverage(&image, (START + END) / 2, STEADY_Y);
    assert!(tap.abs_diff(18) <= 1, "single dab: {tap}");
    assert!(
        steady > tap * 3 && steady <= 179,
        "overlapping dabs: {steady}"
    );
}

#[test]
fn sparse_imported_spacing_survives_rendering() {
    let mut data = Vec::new();
    for header in [28u32, 2, 2, 2, 1] {
        data.extend(header.to_be_bytes());
    }
    data.extend(b"GIMP");
    data.extend(200u32.to_be_bytes());
    data.extend([255; 4]);
    let imported = symphony_brushes::import_bytes("gbr", "Spacing probe", &data).unwrap();
    let image = sheet(&imported.brushes[0]);
    // At 200% spacing with a 64 px tip, consecutive dabs are 128 px apart.
    assert_eq!(coverage(&image, START, STEADY_Y), 255);
    assert_eq!(coverage(&image, START + 64, STEADY_Y), 0);
    assert_eq!(coverage(&image, START + 128, STEADY_Y), 255);
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{Parser, Subcommand};
use photoncam::dither::{dither, to_grayscale};
use photoncam::document::{DitherMode, Document, Shape};
use photoncam::formats::{GCodeConfig, GCodeGenerator, Lbrn2File, RuidaGenerator, SvgFormat};
use photoncam::geometry::{Point2D, Polyline};
use photoncam::optimizer::CutOptimizer;
use photoncam::simulator::Simulator;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "photoncam")]
#[command(author = "Niko Pietinen")]
#[command(version)]
#[command(about = "PhotonCAM – Modern, high-performance laser CAM software compatible with LightBurn .lbrn2", long_about = None)]
struct Cli {
    #[arg(help = "Optional project file to open directly in PhotonCAM (.lbrn2, .lbrn, .svg)")]
    file: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect a LightBurn .lbrn2 or SVG file
    Info {
        #[arg(help = "Input file path (.lbrn2 or .svg)")]
        input: PathBuf,
    },
    /// Generate CNC / GRBL G-Code from a laser project
    Gcode {
        #[arg(help = "Input file path (.lbrn2 or .svg)")]
        input: PathBuf,
        #[arg(short, long, help = "Output .gcode file path")]
        output: Option<PathBuf>,
        #[arg(long, default_value = "1000.0", help = "Max spindle / laser PWM S value")]
        max_s: f64,
        #[arg(long, default_value = "6000.0", help = "Rapid travel rate in mm/min")]
        rapid: f64,
        #[arg(long, default_value = "0.0", help = "Laser beam kerf compensation in mm (0.0 = disabled)")]
        kerf: f64,
    },
    /// Generate Ruida DSP .rd binary file
    Ruida {
        #[arg(help = "Input file path (.lbrn2 or .svg)")]
        input: PathBuf,
        #[arg(short, long, help = "Output .rd file path")]
        output: Option<PathBuf>,
        #[arg(long, default_value = "0.0", help = "Laser beam kerf compensation in mm (0.0 = disabled)")]
        kerf: f64,
    },
    /// Run toolpath simulation and estimate cycle time
    Simulate {
        #[arg(help = "Input file path (.lbrn2 or .svg)")]
        input: PathBuf,
        #[arg(long, default_value = "3000.0", help = "Machine acceleration in mm/s^2")]
        accel: f64,
        #[arg(long, default_value = "0.0", help = "Laser beam kerf compensation in mm (0.0 = disabled)")]
        kerf: f64,
    },
    /// Dither an image for laser engraving
    Dither {
        #[arg(help = "Input image path (PNG, JPG, BMP)")]
        input: PathBuf,
        #[arg(short, long, help = "Output 1-bit image path")]
        output: PathBuf,
        #[arg(long, default_value = "atkinson", help = "Dither algorithm: atkinson, floyd, jarvis, stucki, bayer, halftone, threshold")]
        algorithm: String,
    },
    /// Create a sample LightBurn .lbrn2 file with gears and cut settings
    Demo {
        #[arg(short, long, default_value = "demo.lbrn2", help = "Output .lbrn2 file path")]
        output: PathBuf,
    },
    #[cfg(feature = "gui")]
    /// Launch the interactive CAM graphical user interface
    Gui,
}

fn load_document(path: &PathBuf) -> Result<Document, String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "lbrn2" | "lbrn" => Lbrn2File::load_from_file(path),
        "svg" => SvgFormat::load_from_file(path),
        _ => Err(format!("Unsupported file extension: .{}", ext)),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let cmd = match cli.command {
        Some(c) => c,
        None => {
            #[cfg(feature = "gui")]
            {
                Commands::Gui
            }
            #[cfg(not(feature = "gui"))]
            {
                println!("PhotonCAM CLI. Use --help for available commands.");
                return Ok(());
            }
        }
    };

    match cmd {
        Commands::Info { input } => {
            let doc = load_document(&input)?;
            let bounds = doc.bounds();
            println!("=======================================================");
            println!(" PhotonCAM Document: {}", doc.name);
            println!("=======================================================");
            println!("Workspace Bed:   {:.1} x {:.1} mm", doc.width, doc.height);
            println!("Bounding Box:    {:.2}, {:.2} -> {:.2}, {:.2} (Size: {:.2} x {:.2} mm)",
                bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y, bounds.width(), bounds.height());
            println!("Total Shapes:    {}", doc.shapes.len());
            println!("Layers ({}):", doc.layers.len());
            for (idx, layer) in &doc.layers {
                println!("  - [{:02}] {} ({}): Mode={:?}, Speed={:.1} mm/s, Power={:.0}-{:.0}%, Passes={}",
                    idx, layer.name, layer.color_hex, layer.cut_setting.mode,
                    layer.cut_setting.speed, layer.cut_setting.power_min, layer.cut_setting.power_max,
                    layer.cut_setting.num_passes);
            }
        }
        Commands::Gcode { input, output, max_s, rapid, kerf } => {
            let doc = load_document(&input)?;
            let mut optimizer = CutOptimizer::new();
            optimizer.default_kerf = kerf;
            let job = optimizer.optimize(&doc);

            let mut config = GCodeConfig::default();
            config.max_s_value = max_s;
            config.rapid_speed = rapid;

            let generator = GCodeGenerator::new(config);
            let gcode = generator.generate(&doc, &job);

            let out_path = output.unwrap_or_else(|| input.with_extension("gcode"));
            let mut file = File::create(&out_path)?;
            file.write_all(gcode.as_bytes())?;
            println!("✓ Exported G-Code to: {}", out_path.display());
            println!("  Paths: {}, G-Code lines: {}", job.paths.len(), gcode.lines().count());
        }
        Commands::Ruida { input, output, kerf } => {
            let doc = load_document(&input)?;
            let mut optimizer = CutOptimizer::new();
            optimizer.default_kerf = kerf;
            let job = optimizer.optimize(&doc);

            let generator = RuidaGenerator::new();
            let bytes = generator.generate_rd(&doc, &job);

            let out_path = output.unwrap_or_else(|| input.with_extension("rd"));
            let mut file = File::create(&out_path)?;
            file.write_all(&bytes)?;
            println!("✓ Exported Ruida .rd to: {}", out_path.display());
            println!("  Paths: {}, File size: {} bytes", job.paths.len(), bytes.len());
        }
        Commands::Simulate { input, accel, kerf } => {
            let doc = load_document(&input)?;
            let mut optimizer = CutOptimizer::new();
            optimizer.default_kerf = kerf;
            let job = optimizer.optimize(&doc);

            let mut sim = Simulator::new();
            sim.acceleration_mm_s2 = accel;
            let stats = sim.simulate(&doc, &job);

            println!("=======================================================");
            println!(" PhotonCAM Simulation: {}", doc.name);
            println!("=======================================================");
            println!("Toolpath Cuts:       {}", stats.cut_count);
            println!("Cut Distance:        {:.2} mm", stats.total_cut_distance_mm);
            println!("Rapid Distance:      {:.2} mm", stats.total_rapid_distance_mm);
            println!("Estimated Cut Time:  {:.1} s", stats.estimated_cut_time_sec);
            println!("Estimated Rapid Time:{:.1} s", stats.estimated_rapid_time_sec);
            println!("Total Job Time:      {:.1} s ({:.2} min)",
                stats.total_estimated_time_sec, stats.total_estimated_time_sec / 60.0);
            println!("Laser Duty Cycle:    {:.1}%",
                (stats.estimated_cut_time_sec / stats.total_estimated_time_sec) * 100.0);
        }
        Commands::Dither { input, output, algorithm } => {
            let img = image::open(&input)?.to_rgba8();
            let (w, h) = img.dimensions();
            let raw = img.into_raw();
            let gray = to_grayscale(&raw, w as usize, h as usize, 4);

            let mode = match algorithm.to_lowercase().as_str() {
                "floyd" | "floydsteinberg" => DitherMode::FloydSteinberg,
                "jarvis" => DitherMode::Jarvis,
                "stucki" => DitherMode::Stucki,
                "bayer" | "ordered" => DitherMode::OrderedBayer,
                "halftone" => DitherMode::Halftone,
                "threshold" => DitherMode::Threshold,
                _ => DitherMode::Atkinson,
            };

            let mask = dither(&gray, w as usize, h as usize, mode);

            // Save output as 1-bit grayscale image
            let mut out_img = image::GrayImage::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let idx = y as usize * w as usize + x as usize;
                    let val = if mask[idx] { 0 } else { 255 }; // black dot for laser, white for background
                    out_img.put_pixel(x, y, image::Luma([val]));
                }
            }
            out_img.save(&output)?;
            println!("✓ Dithered image saved to: {} ({:?}, {}x{})", output.display(), mode, w, h);
        }
        Commands::Demo { output } => {
            let mut doc = Document::new("Gear_Demo", 200.0, 200.0);

            // Layer 0: Cut (Red, 10mm/s, 80% power)
            let layer0 = doc.get_or_create_layer(0);
            layer0.name = "Cut_Perimeter".to_string();
            layer0.color_hex = "#FF0000".to_string();
            layer0.cut_setting.speed = 10.0;
            layer0.cut_setting.power_max = 85.0;

            // Layer 1: Engrave (Blue, 150mm/s, 30% power)
            let layer1 = doc.get_or_create_layer(1);
            layer1.name = "Engrave_Holes".to_string();
            layer1.color_hex = "#0000FF".to_string();
            layer1.cut_setting.speed = 150.0;
            layer1.cut_setting.power_max = 30.0;

            // Add center circle
            doc.add_shape(Shape::Circle {
                layer_index: 0,
                center: Point2D::new(100.0, 100.0),
                radius: 12.0,
            });

            // Add 4 mounting holes around center
            for i in 0..4 {
                let a = (i as f64) * std::f64::consts::FRAC_PI_2;
                doc.add_shape(Shape::Circle {
                    layer_index: 0,
                    center: Point2D::new(100.0 + 25.0 * a.cos(), 100.0 + 25.0 * a.sin()),
                    radius: 3.5,
                });
            }

            // Add outer 12-tooth gear polygon
            let teeth = 12;
            let mut gear_pts = Vec::new();
            for i in 0..teeth {
                let a0 = (i as f64 / teeth as f64) * std::f64::consts::TAU;
                let a1 = a0 + (0.25 / teeth as f64) * std::f64::consts::TAU;
                let a2 = a0 + (0.50 / teeth as f64) * std::f64::consts::TAU;
                let a3 = a0 + (0.75 / teeth as f64) * std::f64::consts::TAU;

                let r_inner = 40.0;
                let r_outer = 48.0;

                gear_pts.push(Point2D::new(100.0 + r_inner * a0.cos(), 100.0 + r_inner * a0.sin()));
                gear_pts.push(Point2D::new(100.0 + r_outer * a1.cos(), 100.0 + r_outer * a1.sin()));
                gear_pts.push(Point2D::new(100.0 + r_outer * a2.cos(), 100.0 + r_outer * a2.sin()));
                gear_pts.push(Point2D::new(100.0 + r_inner * a3.cos(), 100.0 + r_inner * a3.sin()));
            }

            doc.add_shape(Shape::Path {
                layer_index: 0,
                polyline: Polyline::new(gear_pts, true),
            });

            Lbrn2File::save_to_file(&doc, &output)?;
            println!("✓ Created sample LightBurn .lbrn2 project at: {}", output.display());
        }
        #[cfg(feature = "gui")]
        Commands::Gui => {
            let layout = photoncam::gui::ui_layout::UiLayoutSettings::load_or_default();
            let width = layout.main_window_width.unwrap_or(1880.0);
            let height = layout.main_window_height.unwrap_or(1050.0);

            let mut viewport = eframe::egui::ViewportBuilder::default()
                .with_app_id("PhotonCAM")
                .with_inner_size([width, height])
                .with_min_inner_size([1280.0, 750.0])
                .with_title("PhotonCAM - Modern Laser CAM");

            // Load window and taskbar icon
            let icon_bytes = include_bytes!("../assets/laserburn_icon_256.png");
            if let Ok(img) = image::load_from_memory(icon_bytes) {
                let rgba_img = img.into_rgba8();
                let (width, height) = rgba_img.dimensions();
                let icon_data = eframe::egui::IconData {
                    rgba: rgba_img.into_raw(),
                    width,
                    height,
                };
                viewport = viewport.with_icon(icon_data);
            }

            let options = eframe::NativeOptions {
                viewport,
                ..Default::default()
            };
            let open_file = cli.file.clone();
            eframe::run_native(
                "PhotonCAM",
                options,
                Box::new(move |cc| {
                    let mut app = photoncam::PhotonCamApp::new(cc);
                    if let Some(ref f) = open_file {
                        let _ = app.open_file(f);
                    }
                    Ok(Box::new(app))
                }),
            )?;
        }
    }

    Ok(())
}

#[cfg(feature = "gui")]
use crate::document::{CutMode, DitherMode, Document, Layer, Shape};
use crate::formats::{GCodeConfig, GCodeGenerator, Lbrn2File, RuidaGenerator, SvgFormat};
use crate::geometry::{BoundingBox, Point2D, Polyline};
use crate::optimizer::CutOptimizer;
use eframe::egui;

pub mod tool_palette;
pub mod floating_panels;
pub mod generators_dialogs;
pub mod ui_layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTool {
    Select,
    NodeEdit,
    DrawRect,
    DrawCircle,
    DrawLine,
    DrawText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    Right,
    Top,
    Bottom,
    CenterH,
    CenterV,
    CenterToBed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributeMode {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LengthUnit {
    Millimeters,
    Inches,
}

impl LengthUnit {
    pub const ALL: &'static [LengthUnit] = &[LengthUnit::Millimeters, LengthUnit::Inches];

    pub fn label(&self) -> &'static str {
        match self {
            LengthUnit::Millimeters => "Millimeters (mm)",
            LengthUnit::Inches => "Inches (in)",
        }
    }

    pub fn suffix(&self) -> &'static str {
        match self {
            LengthUnit::Millimeters => " mm",
            LengthUnit::Inches => " in",
        }
    }

    pub fn to_display(&self, mm: f64) -> f64 {
        match self {
            LengthUnit::Millimeters => mm,
            LengthUnit::Inches => mm / 25.4,
        }
    }

    pub fn from_display(&self, val: f64) -> f64 {
        match self {
            LengthUnit::Millimeters => val,
            LengthUnit::Inches => val * 25.4,
        }
    }

    pub fn format(&self, mm: f64) -> String {
        match self {
            LengthUnit::Millimeters => format!("{:.1} mm", mm),
            LengthUnit::Inches => format!("{:.3} in", mm / 25.4),
        }
    }

    pub fn format_dual(&self, mm: f64) -> String {
        format!("{:.1} mm ({:.3} in)", mm, mm / 25.4)
    }
}

#[derive(Debug, Clone)]
pub struct PreviewMove {
    pub from: Point2D,
    pub to: Point2D,
    pub is_cut: bool,
    pub color: egui::Color32,
    pub power: f64,
    pub speed: f64,
    pub start_time: f64,
    pub duration: f64,
}

#[derive(Debug, Clone)]
pub struct PreviewJobData {
    pub moves: Vec<PreviewMove>,
    pub total_time_sec: f64,
    pub total_cut_distance: f64,
    pub total_rapid_distance: f64,
    pub bounds: BoundingBox,
}

impl PreviewJobData {
    pub fn build(doc: &Document, default_kerf: f64) -> Self {
        let mut opt = CutOptimizer::new();
        opt.default_kerf = default_kerf;
        let job = opt.optimize(doc);

        let sim = crate::simulator::Simulator::default();
        let mut moves = Vec::new();
        let mut current_pos = Point2D::ZERO;
        let mut current_time = 0.0;
        let mut total_cut_distance = 0.0;
        let mut total_rapid_distance = 0.0;
        let mut bounds = BoundingBox::empty();

        for path in &job.paths {
            let layer = doc.layers.get(&path.layer_index);
            let cut_speed = layer.map(|l| l.cut_setting.speed).unwrap_or(50.0);
            let cut_power = layer.map(|l| l.cut_setting.power_max).unwrap_or(80.0);
            let layer_color = layer
                .map(|l| {
                    let hex = l.color_hex.trim_start_matches('#');
                    if let Ok(c) = u32::from_str_radix(hex, 16) {
                        egui::Color32::from_rgb(((c >> 16) & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, (c & 0xFF) as u8)
                    } else {
                        egui::Color32::from_rgb(255, 60, 60)
                    }
                })
                .unwrap_or(egui::Color32::from_rgb(255, 60, 60));

            let pts = &path.polyline.points;
            if pts.len() < 2 {
                continue;
            }

            // Rapid move to start of path
            let rapid_dist = current_pos.distance_to(&pts[0]);
            if rapid_dist > 1e-4 {
                let duration = sim.trapezoid_time(rapid_dist, sim.rapid_speed_mm_s);
                moves.push(PreviewMove {
                    from: current_pos,
                    to: pts[0],
                    is_cut: false,
                    color: egui::Color32::from_rgba_unmultiplied(120, 150, 180, 100),
                    power: 0.0,
                    speed: sim.rapid_speed_mm_s,
                    start_time: current_time,
                    duration,
                });
                current_time += duration;
                total_rapid_distance += rapid_dist;
                bounds.expand_to_include(&pts[0]);
            }
            current_pos = pts[0];

            // Cut moves along polyline
            for i in 0..(pts.len() - 1) {
                let p1 = pts[i];
                let p2 = pts[i + 1];
                let seg_dist = p1.distance_to(&p2);
                let duration = if cut_speed > 0.0 { seg_dist / cut_speed } else { 0.01 };
                let power = path.powers.as_ref().and_then(|pw| pw.get(i)).copied().unwrap_or(cut_power);

                moves.push(PreviewMove {
                    from: p1,
                    to: p2,
                    is_cut: true,
                    color: layer_color,
                    power,
                    speed: cut_speed,
                    start_time: current_time,
                    duration,
                });
                current_time += duration;
                total_cut_distance += seg_dist;
                bounds.expand_to_include(&p2);
            }

            // Close path if closed polyline
            if path.polyline.is_closed && pts.len() > 2 {
                let p_last = *pts.last().unwrap();
                let p_first = pts[0];
                let seg_dist = p_last.distance_to(&p_first);
                let duration = if cut_speed > 0.0 { seg_dist / cut_speed } else { 0.01 };
                moves.push(PreviewMove {
                    from: p_last,
                    to: p_first,
                    is_cut: true,
                    color: layer_color,
                    power: cut_power,
                    speed: cut_speed,
                    start_time: current_time,
                    duration,
                });
                current_time += duration;
                total_cut_distance += seg_dist;
            }

            if let Some(last_pt) = pts.last() {
                current_pos = if path.polyline.is_closed { pts[0] } else { *last_pt };
            }
        }

        Self {
            moves,
            total_time_sec: current_time.max(0.001),
            total_cut_distance,
            total_rapid_distance,
            bounds,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettingsProfile {
    #[serde(default = "default_profile_version")]
    pub version: u32,
    pub device: crate::device::DeviceSettings,
    #[serde(default)]
    pub camera: Option<crate::camera::CameraCalibration>,
    #[serde(default)]
    pub layers: Option<std::collections::BTreeMap<usize, crate::document::Layer>>,
}

fn default_profile_version() -> u32 {
    1
}

#[derive(Debug, Clone)]
pub enum FileDialogAction {
    OpenProject(std::path::PathBuf),
    ImportImage(std::path::PathBuf),
    SaveProject(std::path::PathBuf),
    ExportGCode(std::path::PathBuf),
    ExportRuida(std::path::PathBuf),
    BrowseFont(std::path::PathBuf),
    ExportSettings(std::path::PathBuf),
    ImportSettings(std::path::PathBuf),
    ExportMaterialLibrary(std::path::PathBuf),
    ImportMaterialLibrary(std::path::PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragHandle {
    None,
    Move,
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

#[derive(Debug, Clone)]
pub struct OneShotSnapshot {
    pub started_at: std::time::Instant,
    pub was_streaming: bool,
    pub initial_frame_number: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DepthTestType {
    MultiPass,
    PowerSteps,
    PassesVsSpeed,
    LineInterval,
}

#[cfg(feature = "gui")]
pub struct LaserBurnApp {
    pub doc: Document,
    pub active_tool: ActiveTool,
    pub selected_shape_indices: Vec<usize>, // Supports multi-selection!
    pub selected_layer: usize,
    pub zoom: f32,
    pub pan: egui::Vec2,

    // Interactive Dragging & Sizing
    pub drag_handle: DragHandle,
    pub drag_start_doc: Option<Point2D>,
    pub last_drag_doc: Option<Point2D>,
    pub lock_aspect_ratio: bool,
    pub custom_rot_angle: f64,

    // Rubberband Box Selection
    pub is_box_selecting: bool,
    pub box_select_start: Option<Point2D>,
    pub box_select_current: Option<Point2D>,

    // In-progress shape creation
    pub is_drawing: bool,
    pub draw_start: Option<Point2D>,
    pub draw_current: Option<Point2D>,

    // Node Editing State
    pub selected_node_index: Option<usize>,
    pub dragging_node_index: Option<usize>,

    // Machine & Connection Manager
    pub device: crate::device::DeviceManager,
    pub jog_step: f64,
    pub jog_speed: f64,
    pub command_input: String,
    pub available_ports: Vec<String>,

    // Windows / Dialogs State
    pub show_properties_window: bool,
    pub show_objects_window: bool,
    pub show_job_window: bool,
    pub show_connection_window: bool,
    pub show_settings_window: bool,
    pub show_web_remote_window: bool,
    pub web_remote_port: u16,
    pub web_remote_active: bool,
    pub web_remote_stopper: Option<std::sync::Arc<std::sync::Mutex<bool>>>,
    pub show_move_window: bool,
    pub show_camera_window: bool,
    pub show_text_dialog: bool,
    pub text_input: String,
    pub text_font_size: f64,
    pub text_is_singleline: bool,
    pub text_font_path: String,
    pub text_font_name: String,
    pub text_pos_x: f64,
    pub text_pos_y: f64,
    pub text_align: crate::text::TextAlign,
    pub text_on_path_enabled: bool,
    pub text_on_path_offset: f64,
    pub text_on_path_perpendicular: f64,
    pub text_on_path_reverse: bool,

    // Status
    pub status_msg: String,

    // Internal Shape Clipboard for Cut / Copy / Paste
    pub clipboard_shapes: Vec<Shape>,

    // Camera Feed & Manager
    pub camera_manager: crate::camera::CameraManager,
    pub camera_calibration: crate::camera::CameraCalibration,
    pub lens_undistorter: crate::camera::LensUndistorter,
    pub camera_texture: Option<egui::TextureHandle>,
    pub last_camera_frame_number: u64,
    pub last_raw_frame: Option<crate::camera::CameraFrame>,
    pub last_applied_calibration: Option<crate::camera::CameraCalibration>,
    pub camera_window_tab: usize,

    // 4-Point Camera Alignment Wizard State
    pub wizard_step: usize,
    pub wizard_target_idx: usize,
    pub wizard_points: [Option<(f64, f64)>; 4],
    pub wizard_dragged_pt: Option<usize>,
    pub wizard_magnifier_zoom: f32,

    // Keyboard Jogging Control
    pub keyboard_jog_laser: bool,

    // 1-Click Bed Overlay Snapshot Capture State
    pub oneshot_snapshot: Option<OneShotSnapshot>,

    // Non-blocking asynchronous native file dialog channel
    pub file_dialog_tx: std::sync::mpsc::Sender<FileDialogAction>,
    pub file_dialog_rx: std::sync::mpsc::Receiver<FileDialogAction>,

    // GPU Texture Cache for photographic raster image shapes
    pub image_textures: std::collections::HashMap<usize, (u64, egui::TextureHandle)>,

    // Simulation / Estimate Cache
    pub doc_version: u64,
    pub cached_estimate: crate::simulator::SimulationStats,
    pub last_sim_version: u64,

    // Undo / Redo History Stacks (up to 50 snapshots)
    pub undo_stack: Vec<Vec<Shape>>,
    pub redo_stack: Vec<Vec<Shape>>,

    // Offset Shapes Tool State
    pub show_offset_window: bool,
    pub offset_distance: f64,
    pub offset_inward: bool,
    pub offset_both: bool,
    pub offset_delete_orig: bool,

    // Array Tools (Grid & Radial Array)
    pub show_grid_array_window: bool,
    pub grid_columns: usize,
    pub grid_rows: usize,
    pub grid_spacing_x: f64,
    pub grid_spacing_y: f64,
    pub grid_use_spacing: bool,

    pub show_radial_array_window: bool,
    pub radial_count: usize,
    pub radial_total_angle: f64,
    pub radial_rotate_copies: bool,
    pub radial_center_x: f64,
    pub radial_center_y: f64,
    pub radial_center_auto: bool,

    // Multiple Machine Profiles State
    pub show_device_profiles_window: bool,
    pub profile_new_name: String,

    // Display Units State
    pub length_unit: LengthUnit,

    // Image Trace (Bitmap to Vector) State
    pub show_trace_window: bool,
    pub trace_mode: crate::trace::TraceMode,
    pub trace_threshold: u8,
    pub trace_invert: bool,
    pub trace_blur_radius: usize,
    pub trace_min_area: f64,
    pub trace_tolerance: f64,
    pub trace_delete_image: bool,
    pub trace_preview_polylines: Vec<crate::geometry::Polyline>,
    pub trace_source_shape_idx: Option<usize>,

    // Material Test Generator Window State
    pub show_material_test_window: bool,
    pub mat_test_rows: usize,
    pub mat_test_cols: usize,
    pub mat_test_min_speed: f64,
    pub mat_test_max_speed: f64,
    pub mat_test_min_power: f64,
    pub mat_test_max_power: f64,
    pub mat_test_box_size: f64,
    pub mat_test_is_engrave: bool,

    // Engraving Depth Test Generator Window State
    pub show_depth_test_window: bool,
    pub depth_test_type: DepthTestType,
    pub depth_test_steps: usize,
    pub depth_test_box_w: f64,
    pub depth_test_box_h: f64,
    pub depth_test_speed: f64,
    pub depth_test_power: f64,
    pub depth_test_min_power: f64,
    pub depth_test_max_power: f64,
    pub depth_test_interval: f64,
    pub depth_test_cross_hatch: bool,
    pub depth_test_cut_outline: bool,

    // Serial Traffic Panel State
    pub show_traffic_panel: bool,
    pub hide_ok_in_traffic_log: bool,
    pub explain_traffic_codes: bool,

    // macOS Apple Shortcuts list
    #[cfg(target_os = "macos")]
    pub available_shortcuts: Vec<String>,

    // Material Library & Database State
    pub material_lib: crate::material::MaterialLibrary,
    pub show_material_library: bool,
    pub mat_lib_search: String,
    pub mat_lib_category_filter: String,
    pub mat_lib_selected_category: Option<String>,
    pub mat_lib_selected_material: Option<String>,
    pub mat_lib_selected_thickness: Option<f64>,
    pub mat_lib_selected_op: Option<String>,

    // Save Layer to Library Modal Dialog State
    pub show_save_material_dialog: bool,
    pub save_mat_category: String,
    pub save_mat_name: String,
    pub save_mat_thickness_mm: f64,
    pub save_mat_thickness_label: String,
    pub save_mat_op_name: String,
    pub save_mat_notes: String,

    // Visual Cut Preview Simulator Window State
    pub show_preview_window: bool,
    pub preview_progress: f64,
    pub preview_playing: bool,
    pub preview_speed: f64,
    pub preview_job: Option<PreviewJobData>,
    pub preview_job_version: u64,

    // Smart 2D Auto-Nesting Material Packing State
    pub show_nesting_window: bool,
    pub nesting_options: crate::nesting::NestingOptions,
    pub nesting_result: Option<crate::nesting::NestingResult>,

    // Advanced Generators (Box Maker, Living Hinge, QR Code, Variable Text)
    pub show_box_maker_window: bool,
    pub box_maker_params: crate::generator::BoxMakerParams,
    pub show_living_hinge_window: bool,
    pub living_hinge_params: crate::generator::LivingHingeParams,
    pub show_qr_code_window: bool,
    pub qr_code_params: crate::generator::QrCodeParams,
    pub show_variable_text_window: bool,
    pub variable_text_params: crate::generator::VariableTextParams,

    // Photoshop-Style V2 Tool Palette State
    pub show_tools_palette: bool,
    pub detach_tools_window: bool,
    pub open_flyout: Option<tool_palette::ToolCategory>,
    pub last_flyout_anchor: Option<egui::Rect>,

    // Persistent UI Layout & Windows State
    pub ui_layout: ui_layout::UiLayoutSettings,
}

#[cfg(feature = "gui")]
impl Default for LaserBurnApp {
    fn default() -> Self {
        let (file_dialog_tx, file_dialog_rx) = std::sync::mpsc::channel();
        let ui_layout = ui_layout::UiLayoutSettings::load_or_default();
        let mut app = Self {
            doc: Document::new("PhotonCAM_Project", 400.0, 400.0),
            active_tool: ActiveTool::Select,
            selected_shape_indices: Vec::new(),
            selected_layer: 0,
            zoom: ui_layout.zoom,
            pan: egui::Vec2::new(ui_layout.pan_x, ui_layout.pan_y),

            drag_handle: DragHandle::None,
            drag_start_doc: None,
            last_drag_doc: None,
            lock_aspect_ratio: true,
            custom_rot_angle: 1.0,

            is_box_selecting: false,
            box_select_start: None,
            box_select_current: None,

            is_drawing: false,
            draw_start: None,
            draw_current: None,

            selected_node_index: None,
            dragging_node_index: None,

            device: crate::device::DeviceManager::new(),
            jog_step: ui_layout.jog_step,
            jog_speed: ui_layout.jog_speed,
            command_input: "".to_string(),
            available_ports: Vec::new(),

            show_properties_window: ui_layout.show_properties_window,
            show_objects_window: ui_layout.show_objects_window,
            show_job_window: ui_layout.show_job_window,
            show_connection_window: ui_layout.show_connection_window,
            show_settings_window: ui_layout.show_settings_window,
            show_web_remote_window: ui_layout.show_web_remote_window,
            web_remote_port: 8080,
            web_remote_active: false,
            web_remote_stopper: None,
            show_move_window: ui_layout.show_move_window,
            show_camera_window: ui_layout.show_camera_window,
            show_text_dialog: ui_layout.show_text_dialog,
            text_input: "Text".to_string(),
            text_font_size: 15.0,
            text_is_singleline: false,
            text_font_path: crate::text::default_system_font(),
            text_font_name: "Arial / System".to_string(),
            text_pos_x: 60.0,
            text_pos_y: 60.0,
            text_align: crate::text::TextAlign::Left,
            text_on_path_enabled: false,
            text_on_path_offset: 0.0,
            text_on_path_perpendicular: 0.0,
            text_on_path_reverse: false,

            status_msg: "PhotonCAM Ready - Click inside shapes to select, drag to move, or use Arrow Keys to Jog".to_string(),
            clipboard_shapes: Vec::new(),
            camera_manager: crate::camera::CameraManager::default(),
            camera_calibration: crate::camera::CameraCalibration::load(),
            lens_undistorter: crate::camera::LensUndistorter::default(),
            camera_texture: None,
            last_camera_frame_number: 0,
            last_raw_frame: Self::load_bed_snapshot_from_disk(),
            last_applied_calibration: None,
            camera_window_tab: 0,
            wizard_step: 0,
            wizard_target_idx: 0,
            wizard_points: [None; 4],
            wizard_dragged_pt: None,
            wizard_magnifier_zoom: 4.0,
            keyboard_jog_laser: true,
            oneshot_snapshot: None,
            image_textures: std::collections::HashMap::new(),
            doc_version: 0,
            cached_estimate: crate::simulator::SimulationStats::default(),
            last_sim_version: u64::MAX,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            show_offset_window: ui_layout.show_offset_window,
            offset_distance: 2.0,
            offset_inward: false,
            offset_both: false,
            offset_delete_orig: false,

            show_grid_array_window: ui_layout.show_grid_array_window,
            grid_columns: 3,
            grid_rows: 2,
            grid_spacing_x: 5.0,
            grid_spacing_y: 5.0,
            grid_use_spacing: true,

            show_radial_array_window: ui_layout.show_radial_array_window,
            radial_count: 8,
            radial_total_angle: 360.0,
            radial_rotate_copies: true,
            radial_center_x: 200.0,
            radial_center_y: 200.0,
            radial_center_auto: true,

            show_device_profiles_window: ui_layout.show_device_profiles_window,
            profile_new_name: String::new(),

            length_unit: ui_layout.length_unit,

            show_trace_window: ui_layout.show_trace_window,
            trace_mode: crate::trace::TraceMode::Outline,
            trace_threshold: 128,
            trace_invert: false,
            trace_blur_radius: 1,
            trace_min_area: 0.2,
            trace_tolerance: 0.08,
            trace_delete_image: false,
            trace_preview_polylines: Vec::new(),
            trace_source_shape_idx: None,

            show_material_test_window: ui_layout.show_material_test_window,
            mat_test_rows: 5,
            mat_test_cols: 5,
            mat_test_min_speed: 20.0,
            mat_test_max_speed: 100.0,
            mat_test_min_power: 15.0,
            mat_test_max_power: 95.0,
            mat_test_box_size: 10.0,
            mat_test_is_engrave: true,

            show_depth_test_window: ui_layout.show_depth_test_window,
            depth_test_type: DepthTestType::MultiPass,
            depth_test_steps: 6,
            depth_test_box_w: 16.0,
            depth_test_box_h: 12.0,
            depth_test_speed: 100.0,
            depth_test_power: 70.0,
            depth_test_min_power: 15.0,
            depth_test_max_power: 85.0,
            depth_test_interval: 0.08,
            depth_test_cross_hatch: true,
            depth_test_cut_outline: true,
            show_traffic_panel: ui_layout.show_traffic_panel,
            hide_ok_in_traffic_log: ui_layout.hide_ok_in_traffic_log,
            explain_traffic_codes: ui_layout.explain_traffic_codes,
            #[cfg(target_os = "macos")]
            available_shortcuts: Vec::new(),
            material_lib: crate::material::MaterialLibrary::load_or_default(),
            show_material_library: ui_layout.show_material_library,
            mat_lib_search: String::new(),
            mat_lib_category_filter: "All Categories".to_string(),
            mat_lib_selected_category: None,
            mat_lib_selected_material: None,
            mat_lib_selected_thickness: None,
            mat_lib_selected_op: None,
            show_save_material_dialog: false,
            save_mat_category: "PhotonCAM Oletus (10W-20W)".to_string(),
            save_mat_name: "Birch Plywood".to_string(),
            save_mat_thickness_mm: 3.0,
            save_mat_thickness_label: "3.0 mm".to_string(),
            save_mat_op_name: "Clean Cut".to_string(),
            save_mat_notes: String::new(),
            show_preview_window: ui_layout.show_preview_window,
            preview_progress: 0.0,
            preview_playing: false,
            preview_speed: 1.0,
            preview_job: None,
            preview_job_version: 0,
            show_nesting_window: ui_layout.show_nesting_window,
            nesting_options: crate::nesting::NestingOptions::default(),
            nesting_result: None,
            show_box_maker_window: ui_layout.show_box_maker_window,
            box_maker_params: crate::generator::BoxMakerParams::default(),
            show_living_hinge_window: ui_layout.show_living_hinge_window,
            living_hinge_params: crate::generator::LivingHingeParams::default(),
            show_qr_code_window: ui_layout.show_qr_code_window,
            qr_code_params: crate::generator::QrCodeParams::default(),
            show_variable_text_window: ui_layout.show_variable_text_window,
            variable_text_params: crate::generator::VariableTextParams::default(),
            show_tools_palette: ui_layout.show_tools_palette,
            detach_tools_window: ui_layout.detach_tools_window,
            open_flyout: None,
            last_flyout_anchor: None,
            file_dialog_tx,
            file_dialog_rx,
            ui_layout,
        };

        app.doc.width = app.device.settings.bed_width;
        app.doc.height = app.device.settings.bed_height;

        if app.camera_calibration.homography_points.len() == 4 {
            for i in 0..4 {
                let pt = &app.camera_calibration.homography_points[i];
                app.wizard_points[i] = Some((pt.camera_u, pt.camera_v));
            }
        }

        // Text input defaults
        app.text_input = "Text".to_string();
        app.text_pos_x = 100.0;
        app.text_pos_y = 120.0;
        app.text_font_size = 15.0;
        app.text_is_singleline = false;
        app.selected_layer = 0;

        // Laser starts in safe OFF state with head at origin
        app.device.is_weak_light_on = false;
        app.device.machine_x = 0.0;
        app.device.machine_y = 0.0;

        app
    }
}

#[cfg(feature = "gui")]
impl LaserBurnApp {
    /// Creates a new LaserBurnApp initialized with eframe CreationContext and restored layout
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let mut app = Self::default();
        if let Some(storage) = cc.storage {
            if let Some(saved) = eframe::get_value::<ui_layout::UiLayoutSettings>(storage, "photoncam_ui_layout") {
                app.apply_ui_layout(saved);
            }
        }
        app
    }

    /// Records an open window's position and size so it persists across runs
    pub fn record_window_rect(&mut self, window_id: &str, rect: egui::Rect) {
        self.ui_layout.window_rects.insert(
            window_id.to_string(),
            ui_layout::WindowRect {
                x: rect.min.x,
                y: rect.min.y,
                width: rect.width(),
                height: rect.height(),
            },
        );
    }

    /// Gets the saved or default position and size for a window
    pub fn window_pos_size(
        &self,
        window_id: &str,
        default_pos: [f32; 2],
        default_size: [f32; 2],
    ) -> ([f32; 2], [f32; 2]) {
        if let Some(saved) = self.ui_layout.window_rects.get(window_id) {
            ([saved.x, saved.y], [saved.width, saved.height])
        } else {
            (default_pos, default_size)
        }
    }

    /// Captures the complete current UI layout state for saving
    pub fn capture_ui_layout(&self) -> ui_layout::UiLayoutSettings {
        let mut layout = self.ui_layout.clone();
        layout.show_properties_window = self.show_properties_window;
        layout.show_objects_window = self.show_objects_window;
        layout.show_job_window = self.show_job_window;
        layout.show_connection_window = self.show_connection_window;
        layout.show_settings_window = self.show_settings_window;
        layout.show_move_window = self.show_move_window;
        layout.show_camera_window = self.show_camera_window;
        layout.show_traffic_panel = self.show_traffic_panel;
        layout.show_tools_palette = self.show_tools_palette;
        layout.detach_tools_window = self.detach_tools_window;
        layout.show_preview_window = self.show_preview_window;
        layout.show_material_library = self.show_material_library;
        layout.show_nesting_window = self.show_nesting_window;
        layout.show_box_maker_window = self.show_box_maker_window;
        layout.show_living_hinge_window = self.show_living_hinge_window;
        layout.show_qr_code_window = self.show_qr_code_window;
        layout.show_variable_text_window = self.show_variable_text_window;
        layout.show_trace_window = self.show_trace_window;
        layout.show_material_test_window = self.show_material_test_window;
        layout.show_depth_test_window = self.show_depth_test_window;
        layout.show_grid_array_window = self.show_grid_array_window;
        layout.show_radial_array_window = self.show_radial_array_window;
        layout.show_offset_window = self.show_offset_window;
        layout.show_text_dialog = self.show_text_dialog;
        layout.show_device_profiles_window = self.show_device_profiles_window;
        layout.show_web_remote_window = self.show_web_remote_window;

        layout.length_unit = self.length_unit;
        layout.zoom = self.zoom;
        layout.pan_x = self.pan.x;
        layout.pan_y = self.pan.y;
        layout.jog_step = self.jog_step;
        layout.jog_speed = self.jog_speed;
        layout.hide_ok_in_traffic_log = self.hide_ok_in_traffic_log;
        layout.explain_traffic_codes = self.explain_traffic_codes;
        layout
    }

    /// Applies a saved UI layout to active application fields
    pub fn apply_ui_layout(&mut self, layout: ui_layout::UiLayoutSettings) {
        self.show_properties_window = layout.show_properties_window;
        self.show_objects_window = layout.show_objects_window;
        self.show_job_window = layout.show_job_window;
        self.show_connection_window = layout.show_connection_window;
        self.show_settings_window = layout.show_settings_window;
        self.show_move_window = layout.show_move_window;
        self.show_camera_window = layout.show_camera_window;
        self.show_traffic_panel = layout.show_traffic_panel;
        self.show_tools_palette = layout.show_tools_palette;
        self.detach_tools_window = layout.detach_tools_window;
        self.show_preview_window = layout.show_preview_window;
        self.show_material_library = layout.show_material_library;
        self.show_nesting_window = layout.show_nesting_window;
        self.show_box_maker_window = layout.show_box_maker_window;
        self.show_living_hinge_window = layout.show_living_hinge_window;
        self.show_qr_code_window = layout.show_qr_code_window;
        self.show_variable_text_window = layout.show_variable_text_window;
        self.show_trace_window = layout.show_trace_window;
        self.show_material_test_window = layout.show_material_test_window;
        self.show_depth_test_window = layout.show_depth_test_window;
        self.show_grid_array_window = layout.show_grid_array_window;
        self.show_radial_array_window = layout.show_radial_array_window;
        self.show_offset_window = layout.show_offset_window;
        self.show_text_dialog = layout.show_text_dialog;
        self.show_device_profiles_window = layout.show_device_profiles_window;
        self.show_web_remote_window = layout.show_web_remote_window;

        self.length_unit = layout.length_unit;
        self.zoom = layout.zoom;
        self.pan = egui::Vec2::new(layout.pan_x, layout.pan_y);
        self.jog_step = layout.jog_step;
        self.jog_speed = layout.jog_speed;
        self.hide_ok_in_traffic_log = layout.hide_ok_in_traffic_log;
        self.explain_traffic_codes = layout.explain_traffic_codes;
        self.ui_layout = layout;
    }

    /// Saves the current UI layout to disk immediately
    pub fn save_ui_layout_to_disk(&self) {
        let layout = self.capture_ui_layout();
        let _ = layout.save_to_disk();
    }

    /// Returns "Cmd" on macOS and "Ctrl" on Windows / Linux for native shortcut displays
    pub fn cmd_key() -> &'static str {
        if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        }
    }

    /// Translates a given static string key into the active language
    pub fn tr(&self, key: &'static str) -> &'static str {
        crate::i18n::t(key, self.device.settings.language)
    }

    /// Saves current document shapes onto the Undo stack (max 50 steps) and clears Redo
    pub fn push_undo(&mut self) {
        if self.undo_stack.len() >= 50 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.doc.shapes.clone());
        self.redo_stack.clear();
        self.doc_version = self.doc_version.wrapping_add(1);
    }

    /// Reverts the last document edit (Cmd+Z / Ctrl+Z)
    pub fn undo(&mut self) {
        if let Some(prev_shapes) = self.undo_stack.pop() {
            self.redo_stack.push(self.doc.shapes.clone());
            self.doc.shapes = prev_shapes;
            self.selected_shape_indices.retain(|&i| i < self.doc.shapes.len());
            self.image_textures.clear();
            self.doc_version = self.doc_version.wrapping_add(1);
            self.status_msg = format!("Undo ({} steps remaining)", self.undo_stack.len());
        } else {
            self.status_msg = "Nothing to undo".to_string();
        }
    }

    /// Re-applies an undone document edit (Cmd+Shift+Z / Ctrl+Y)
    pub fn redo(&mut self) {
        if let Some(next_shapes) = self.redo_stack.pop() {
            self.undo_stack.push(self.doc.shapes.clone());
            self.doc.shapes = next_shapes;
            self.selected_shape_indices.retain(|&i| i < self.doc.shapes.len());
            self.image_textures.clear();
            self.doc_version = self.doc_version.wrapping_add(1);
            self.status_msg = format!("Redo ({} steps remaining)", self.redo_stack.len());
        } else {
            self.status_msg = "Nothing to redo".to_string();
        }
    }

    /// Applies contour offset (Outward, Inward, or Both) to currently selected shapes
    pub fn apply_offset_selected(&mut self) {
        if self.selected_shape_indices.is_empty() {
            self.status_msg = "Select at least one shape to offset".to_string();
            return;
        }
        self.push_undo();

        let dist = self.offset_distance.abs().max(0.05);
        let mut new_shapes = Vec::new();
        for &idx in &self.selected_shape_indices {
            if idx < self.doc.shapes.len() {
                let s = &self.doc.shapes[idx];
                let layer = s.layer_index();
                if self.offset_both {
                    new_shapes.extend(s.offset_shape(dist, Some(layer)));
                    new_shapes.extend(s.offset_shape(-dist, Some(layer)));
                } else if self.offset_inward {
                    new_shapes.extend(s.offset_shape(-dist, Some(layer)));
                } else {
                    new_shapes.extend(s.offset_shape(dist, Some(layer)));
                }
            }
        }

        if new_shapes.is_empty() {
            self.status_msg = "Offset distance too small or inward offset collapsed shape".to_string();
            return;
        }

        if self.offset_delete_orig {
            let mut sorted = self.selected_shape_indices.clone();
            sorted.sort_unstable_by(|a, b| b.cmp(a));
            sorted.dedup();
            for idx in sorted {
                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                    self.doc.shapes.remove(idx);
                }
            }
        }

        let mut new_sel = Vec::new();
        for ns in new_shapes {
            self.doc.add_shape(ns);
            new_sel.push(self.doc.shapes.len() - 1);
        }
        self.selected_shape_indices = new_sel;
        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = format!(
            "Created {} offset contour(s) ({:.2} mm {})",
            self.selected_shape_indices.len(),
            dist,
            if self.offset_both {
                "both"
            } else if self.offset_inward {
                "inward"
            } else {
                "outward"
            }
        );
    }

    /// Duplicates selected shapes in a rectangular grid array
    pub fn apply_grid_array(&mut self) {
        if self.selected_shape_indices.is_empty() {
            self.status_msg = "Select at least one shape for Grid Array".to_string();
            return;
        }
        self.push_undo();

        let (delta_x, delta_y) = if self.grid_use_spacing {
            let mut bb = crate::geometry::BoundingBox::empty();
            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() {
                    bb = bb.union(&self.doc.shapes[idx].bounds());
                }
            }
            (bb.width() + self.grid_spacing_x, bb.height() + self.grid_spacing_y)
        } else {
            (self.grid_spacing_x, self.grid_spacing_y)
        };

        let new_indices = self.doc.create_grid_array(
            &self.selected_shape_indices,
            self.grid_columns,
            self.grid_rows,
            delta_x,
            delta_y,
        );
        self.selected_shape_indices.extend(new_indices);
        self.doc_version = self.doc_version.wrapping_add(1);
        self.image_textures.clear();
        self.status_msg = format!("Created {}x{} grid array", self.grid_columns, self.grid_rows);
    }

    /// Duplicates selected shapes in a radial circular array around a center point
    pub fn apply_radial_array(&mut self) {
        if self.selected_shape_indices.is_empty() {
            self.status_msg = "Select at least one shape for Radial Array".to_string();
            return;
        }
        self.push_undo();

        let center = if self.radial_center_auto {
            let mut bb = crate::geometry::BoundingBox::empty();
            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() {
                    bb = bb.union(&self.doc.shapes[idx].bounds());
                }
            }
            bb.center()
        } else {
            crate::geometry::Point2D::new(self.radial_center_x, self.radial_center_y)
        };

        let new_indices = self.doc.create_radial_array(
            &self.selected_shape_indices,
            self.radial_count,
            center,
            self.radial_total_angle,
            self.radial_rotate_copies,
        );
        self.selected_shape_indices.extend(new_indices);
        self.doc_version = self.doc_version.wrapping_add(1);
        self.image_textures.clear();
        self.status_msg = format!("Created radial array ({} copies)", self.radial_count);
    }

    /// Opens the Image Trace dialog and immediately computes real-time preview contours
    pub fn open_trace_dialog(&mut self) {
        let is_image_sel = self.selected_shape_indices.iter().any(|&i| i < self.doc.shapes.len() && matches!(self.doc.shapes[i], Shape::Image { .. }));
        if !is_image_sel {
            self.status_msg = "Please select a raster image on the canvas to trace".to_string();
            return;
        }
        self.show_trace_window = true;
        self.update_trace_preview();
    }

    /// Evaluates vector contours in real-time as threshold, blur, or tolerance sliders are adjusted
    pub fn update_trace_preview(&mut self) {
        self.trace_preview_polylines.clear();
        let Some(&idx) = self.selected_shape_indices.first() else {
            self.trace_source_shape_idx = None;
            return;
        };
        if idx >= self.doc.shapes.len() {
            self.trace_source_shape_idx = None;
            return;
        }

        let (pixels, w, h, channels, x, y, width_mm, height_mm) = match &self.doc.shapes[idx] {
            Shape::Image {
                x,
                y,
                width,
                height,
                raw_pixels,
                raw_width,
                raw_height,
                image_path,
                ..
            } => {
                let px_data = if let Some(p) = raw_pixels {
                    p.clone()
                } else if let Some(path) = image_path {
                    if let Ok(dyn_img) = image::open(path) {
                        dyn_img.to_rgba8().into_raw()
                    } else {
                        return;
                    }
                } else {
                    return;
                };
                let chans = if px_data.len() == (*raw_width as usize) * (*raw_height as usize) * 4 {
                    4
                } else if px_data.len() == (*raw_width as usize) * (*raw_height as usize) * 3 {
                    3
                } else {
                    1
                };
                (px_data, *raw_width as usize, *raw_height as usize, chans, *x, *y, *width, *height)
            }
            _ => {
                self.trace_source_shape_idx = None;
                return;
            }
        };

        let trace_opts = crate::trace::TraceOptions {
            mode: self.trace_mode,
            threshold: self.trace_threshold,
            invert: self.trace_invert,
            blur_radius: self.trace_blur_radius,
            min_area_mm2: self.trace_min_area,
            simplify_tolerance_mm: self.trace_tolerance,
        };

        self.trace_preview_polylines = crate::trace::trace_image_to_polylines(
            &pixels,
            w,
            h,
            channels,
            x,
            y,
            width_mm,
            height_mm,
            &trace_opts,
        );
        self.trace_source_shape_idx = Some(idx);
    }

    /// Traces the currently selected raster image into vector cut/engrave paths
    pub fn apply_trace_image(&mut self) {
        let Some(&idx) = self.selected_shape_indices.first() else {
            self.status_msg = "Select a raster image to trace".to_string();
            return;
        };
        if idx >= self.doc.shapes.len() {
            return;
        }

        if self.trace_preview_polylines.is_empty() {
            self.update_trace_preview();
        }

        if self.trace_preview_polylines.is_empty() {
            self.status_msg = "Image Trace: No vector contours found with current threshold".to_string();
            return;
        }

        self.push_undo();

        let polylines = std::mem::take(&mut self.trace_preview_polylines);
        self.trace_source_shape_idx = None;
        let target_layer = self.selected_layer;
        let count = polylines.len();
        let shapes: Vec<Shape> = polylines
            .into_iter()
            .map(|poly| Shape::Path {
                layer_index: target_layer,
                polyline: poly,
            })
            .collect();

        let vector_group = Shape::Group {
            layer_index: target_layer,
            shapes,
            locked: false,
        };

        if self.trace_delete_image {
            self.doc.shapes[idx] = vector_group;
            self.selected_shape_indices = vec![idx];
        } else {
            self.doc.add_shape(vector_group);
            let new_idx = self.doc.shapes.len() - 1;
            self.selected_shape_indices = vec![new_idx];
        }

        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = format!("Image Traced: Created {} vector paths on Layer {}", count, target_layer);
    }

    /// Generates a calibrated Power (%) vs. Speed (mm/s) Material Test Matrix on the canvas
    pub fn generate_material_test_grid(&mut self) {
        self.push_undo();
        let rows = self.mat_test_rows.clamp(2, 12);
        let cols = self.mat_test_cols.clamp(2, 12);
        let box_sz = self.mat_test_box_size.clamp(4.0, 40.0);
        let gap = (box_sz * 0.30).max(2.5);

        let min_spd = self.mat_test_min_speed.min(self.mat_test_max_speed).max(1.0);
        let max_spd = self.mat_test_max_speed.max(self.mat_test_min_speed).max(1.0);
        let min_pwr = self.mat_test_min_power.min(self.mat_test_max_power).clamp(1.0, 100.0);
        let max_pwr = self.mat_test_max_power.max(self.mat_test_min_power).clamp(1.0, 100.0);

        let start_x = 35.0;
        let start_y = 32.0;

        let mut new_sel = Vec::new();

        // Title & Axis labels on Layer 0 (Line Score)
        let label_layer = 0;
        if let Some(l0) = self.doc.layers.get_mut(&label_layer) {
            l0.name = "Labels (Score)".to_string();
            l0.cut_setting.mode = CutMode::Line;
            l0.cut_setting.speed = 60.0;
            l0.cut_setting.power_max = 35.0;
            l0.output = true;
        }

        let title_str = if self.mat_test_is_engrave {
            format!("MATERIAL TEST (GRAYSCALE / CERAMIC) {:.0}-{:.0}% | {:.0}-{:.0}mm/s", min_pwr, max_pwr, min_spd, max_spd)
        } else {
            format!("CUT TEST {:.0}-{:.0}% | {:.0}-{:.0}mm/s", min_pwr, max_pwr, min_spd, max_spd)
        };
        if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
            text: title_str,
            font_type: crate::text::FontType::SingleLine,
            font_size_mm: (box_sz * 0.38).clamp(3.0, 6.0),
            x: start_x,
            y: start_y - 14.0,
            align: crate::text::TextAlign::Left,
            letter_spacing: 1.0,
            line_spacing: 1.2,
        }) {
            let shapes: Vec<Shape> = polys
                .into_iter()
                .map(|polyline| Shape::Path { layer_index: label_layer, polyline })
                .collect();
            if !shapes.is_empty() {
                self.doc.add_shape(Shape::Group { layer_index: label_layer, shapes, locked: false });
                new_sel.push(self.doc.shapes.len() - 1);
            }
        }

        // Column header labels (Power %)
        for c in 0..cols {
            let t_col = if cols > 1 { c as f64 / (cols - 1) as f64 } else { 0.0 };
            let pwr = min_pwr + t_col * (max_pwr - min_pwr);
            let cx = start_x + c as f64 * (box_sz + gap);
            let cy = start_y - 4.5;
            if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                text: format!("{:.0}%", pwr),
                font_type: crate::text::FontType::SingleLine,
                font_size_mm: (box_sz * 0.32).clamp(2.5, 5.0),
                x: cx + box_sz * 0.1,
                y: cy,
                align: crate::text::TextAlign::Left,
                letter_spacing: 1.0,
                line_spacing: 1.2,
            }) {
                let shapes: Vec<Shape> = polys
                    .into_iter()
                    .map(|polyline| Shape::Path { layer_index: label_layer, polyline })
                    .collect();
                if !shapes.is_empty() {
                    self.doc.add_shape(Shape::Group { layer_index: label_layer, shapes, locked: false });
                    new_sel.push(self.doc.shapes.len() - 1);
                }
            }
        }

        // Rows (Speed mm/s)
        for r in 0..rows {
            let t_row = if rows > 1 { r as f64 / (rows - 1) as f64 } else { 0.0 };
            let spd = min_spd + t_row * (max_spd - min_spd);
            let ry = start_y + r as f64 * (box_sz + gap);

            // Row label on left
            if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                text: format!("{:.0}mm/s", spd),
                font_type: crate::text::FontType::SingleLine,
                font_size_mm: (box_sz * 0.30).clamp(2.5, 4.8),
                x: (start_x - 26.0).max(2.0),
                y: ry + box_sz * 0.65,
                align: crate::text::TextAlign::Left,
                letter_spacing: 1.0,
                line_spacing: 1.2,
            }) {
                let shapes: Vec<Shape> = polys
                    .into_iter()
                    .map(|polyline| Shape::Path { layer_index: label_layer, polyline })
                    .collect();
                if !shapes.is_empty() {
                    self.doc.add_shape(Shape::Group { layer_index: label_layer, shapes, locked: false });
                    new_sel.push(self.doc.shapes.len() - 1);
                }
            }

            if self.mat_test_is_engrave {
                // Use 1 layer per row (speed) configured as GrayscalePower (0%..100%) so all columns in this row
                // burn at the exact row speed with each column's exact analog PWM power!
                let layer_idx = (r + 1).min(29);
                let l = self.doc.get_or_create_layer(layer_idx);
                l.name = format!("Row {} ({:.0} mm/s)", r + 1, spd);
                l.output = true;
                l.cut_setting.mode = CutMode::Image;
                l.cut_setting.dither_mode = DitherMode::GrayscalePower;
                l.cut_setting.speed = spd;
                l.cut_setting.power_min = 0.0;
                l.cut_setting.power_max = 100.0;
                l.cut_setting.invert_image = false;
                l.cut_setting.overscan = 2.0;
                l.cut_setting.line_interval = 0.10;

                for c in 0..cols {
                    let t_col = if cols > 1 { c as f64 / (cols - 1) as f64 } else { 0.0 };
                    let pwr = min_pwr + t_col * (max_pwr - min_pwr);
                    let cx = start_x + c as f64 * (box_sz + gap);

                    // Darkness (1.0 - gray/255.0) = pwr / 100.0 -> gray = ((1.0 - pwr/100.0) * 255.0)
                    // Ensure gray <= 254 so GrayscalePower treats it as an active burn pixel
                    let gray_val = ((1.0 - (pwr / 100.0).clamp(0.01, 1.0)) * 254.0).round() as u8;
                    let mut raw_pixels = Vec::with_capacity(16 * 16 * 4);
                    for _ in 0..(16 * 16) {
                        raw_pixels.push(gray_val);
                        raw_pixels.push(gray_val);
                        raw_pixels.push(gray_val);
                        raw_pixels.push(255);
                    }

                    self.doc.add_shape(Shape::Image {
                        layer_index: layer_idx,
                        x: cx,
                        y: ry,
                        width: box_sz,
                        height: box_sz,
                        image_path: None,
                        raw_pixels: Some(raw_pixels),
                        raw_width: 16,
                        raw_height: 16,
                        brightness: 0.0,
                        contrast: 0.0,
                        gamma: 1.0,
                        sharpen: 0.0,
                    });
                    new_sel.push(self.doc.shapes.len() - 1);
                }
            } else {
                // Vector Cut mode: assign layers across the grid (up to 29 layers)
                for c in 0..cols {
                    let t_col = if cols > 1 { c as f64 / (cols - 1) as f64 } else { 0.0 };
                    let pwr = min_pwr + t_col * (max_pwr - min_pwr);
                    let cx = start_x + c as f64 * (box_sz + gap);
                    let layer_idx = ((r * cols + c) % 29) + 1;
                    let l = self.doc.get_or_create_layer(layer_idx);
                    l.name = format!("{:.0}mm/s {:.0}%", spd, pwr);
                    l.output = true;
                    l.cut_setting.mode = CutMode::Line;
                    l.cut_setting.speed = spd;
                    l.cut_setting.power_max = pwr;

                    self.doc.add_shape(Shape::Rect {
                        layer_index: layer_idx,
                        x: cx,
                        y: ry,
                        width: box_sz,
                        height: box_sz,
                        corner_radius: 0.0,
                    });
                    new_sel.push(self.doc.shapes.len() - 1);
                }
            }
        }

        self.selected_shape_indices = new_sel;
        self.image_textures.clear();
        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = format!(
            "Generated {}x{} Material Test Matrix ({:.0}-{:.0}% Power, {:.0}-{:.0} mm/s Speed)",
            rows, cols, min_pwr, max_pwr, min_spd, max_spd
        );
    }

    /// Generates a calibrated Engraving Depth Test (Stepped Wedge, Multi-Pass, or Interval Matrix)
    /// to measure physical material ablation depth with a caliper.
    pub fn generate_depth_test_grid(&mut self) {
        self.push_undo();

        let steps = self.depth_test_steps.clamp(2, 12);
        let box_w = self.depth_test_box_w.clamp(6.0, 50.0);
        let box_h = self.depth_test_box_h.clamp(6.0, 50.0);
        let gap_x = 4.0;
        let gap_y = 6.0;

        let start_x = 25.0;
        let start_y = 35.0;

        let label_layer = 0;
        let l0 = self.doc.get_or_create_layer(label_layer);
        l0.name = "Labels & Guides".to_string();
        l0.cut_setting.mode = CutMode::Line;
        l0.cut_setting.speed = 60.0;
        l0.cut_setting.power_max = 30.0;
        l0.output = true;

        let palette = [
            "#0080FF", "#00C040", "#FF8000", "#FF2020",
            "#9933FF", "#00CCCC", "#FF0080", "#CCCC00",
            "#808080", "#008080", "#800000", "#000080",
        ];

        let mut new_sel = Vec::new();

        match self.depth_test_type {
            DepthTestType::MultiPass => {
                let title = format!(
                    "MULTI-PASS DEPTH TEST — Power: {:.0}%, Speed: {:.0} mm/s, Interval: {:.2} mm{}",
                    self.depth_test_power,
                    self.depth_test_speed,
                    self.depth_test_interval,
                    if self.depth_test_cross_hatch { " (Cross-Hatch)" } else { "" }
                );
                if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                    text: title,
                    font_type: crate::text::FontType::SingleLine,
                    font_size_mm: 3.5,
                    x: start_x,
                    y: start_y - 12.0,
                    align: crate::text::TextAlign::Left,
                    letter_spacing: 1.0,
                    line_spacing: 1.2,
                }) {
                    for p in polys {
                        self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        new_sel.push(self.doc.shapes.len() - 1);
                    }
                }

                if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                    text: "Measure pocket depth with caliper in mm & record below:".to_string(),
                    font_type: crate::text::FontType::SingleLine,
                    font_size_mm: 2.4,
                    x: start_x,
                    y: start_y - 5.5,
                    align: crate::text::TextAlign::Left,
                    letter_spacing: 1.0,
                    line_spacing: 1.2,
                }) {
                    for p in polys {
                        self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                    }
                }

                for i in 0..steps {
                    let passes = i + 1;
                    let layer_idx = passes;
                    let l = self.doc.get_or_create_layer(layer_idx);
                    l.name = format!("{} Pass{}", passes, if passes == 1 { "" } else { "es" });
                    l.color_hex = palette[i % palette.len()].to_string();
                    l.output = true;
                    l.cut_setting.mode = CutMode::Fill;
                    l.cut_setting.speed = self.depth_test_speed;
                    l.cut_setting.power_max = self.depth_test_power;
                    l.cut_setting.num_passes = passes as u32;
                    l.cut_setting.line_interval = self.depth_test_interval;
                    l.cut_setting.cross_hatch = self.depth_test_cross_hatch;
                    l.cut_setting.scan_angle = 0.0;

                    let cx = start_x + i as f64 * (box_w + gap_x);
                    let cy = start_y;

                    self.doc.add_shape(Shape::Rect {
                        layer_index: layer_idx,
                        x: cx,
                        y: cy,
                        width: box_w,
                        height: box_h,
                        corner_radius: 0.0,
                    });
                    new_sel.push(self.doc.shapes.len() - 1);

                    if self.depth_test_cut_outline {
                        self.doc.add_shape(Shape::Rect {
                            layer_index: label_layer,
                            x: cx,
                            y: cy,
                            width: box_w,
                            height: box_h,
                            corner_radius: 0.0,
                        });
                    }

                    let pass_txt = format!("{}P", passes);
                    if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                        text: pass_txt,
                        font_type: crate::text::FontType::SingleLine,
                        font_size_mm: 3.0,
                        x: cx + box_w * 0.5,
                        y: cy + box_h + 4.5,
                        align: crate::text::TextAlign::Center,
                        letter_spacing: 1.0,
                        line_spacing: 1.2,
                    }) {
                        for p in polys {
                            self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        }
                    }

                    if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                        text: ".__mm".to_string(),
                        font_type: crate::text::FontType::SingleLine,
                        font_size_mm: 2.2,
                        x: cx + box_w * 0.5,
                        y: cy + box_h + 8.5,
                        align: crate::text::TextAlign::Center,
                        letter_spacing: 1.0,
                        line_spacing: 1.2,
                    }) {
                        for p in polys {
                            self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        }
                    }
                }
            }
            DepthTestType::PowerSteps => {
                let min_p = self.depth_test_min_power.min(self.depth_test_max_power).clamp(1.0, 100.0);
                let max_p = self.depth_test_max_power.max(self.depth_test_min_power).clamp(1.0, 100.0);

                let title = format!(
                    "POWER ENGRAVING DEPTH WEDGE — Speed: {:.0} mm/s, Interval: {:.2} mm{}",
                    self.depth_test_speed,
                    self.depth_test_interval,
                    if self.depth_test_cross_hatch { " (Cross-Hatch)" } else { "" }
                );
                if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                    text: title,
                    font_type: crate::text::FontType::SingleLine,
                    font_size_mm: 3.5,
                    x: start_x,
                    y: start_y - 12.0,
                    align: crate::text::TextAlign::Left,
                    letter_spacing: 1.0,
                    line_spacing: 1.2,
                }) {
                    for p in polys {
                        self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        new_sel.push(self.doc.shapes.len() - 1);
                    }
                }

                if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                    text: "Measure pocket depth with caliper in mm & record below:".to_string(),
                    font_type: crate::text::FontType::SingleLine,
                    font_size_mm: 2.4,
                    x: start_x,
                    y: start_y - 5.5,
                    align: crate::text::TextAlign::Left,
                    letter_spacing: 1.0,
                    line_spacing: 1.2,
                }) {
                    for p in polys {
                        self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                    }
                }

                for i in 0..steps {
                    let t = if steps > 1 { i as f64 / (steps - 1) as f64 } else { 0.0 };
                    let pwr = min_p + t * (max_p - min_p);
                    let layer_idx = i + 1;
                    let l = self.doc.get_or_create_layer(layer_idx);
                    l.name = format!("{:.0}% Power", pwr);
                    l.color_hex = palette[i % palette.len()].to_string();
                    l.output = true;
                    l.cut_setting.mode = CutMode::Fill;
                    l.cut_setting.speed = self.depth_test_speed;
                    l.cut_setting.power_max = pwr;
                    l.cut_setting.num_passes = 1;
                    l.cut_setting.line_interval = self.depth_test_interval;
                    l.cut_setting.cross_hatch = self.depth_test_cross_hatch;
                    l.cut_setting.scan_angle = 0.0;

                    let cx = start_x + i as f64 * (box_w + gap_x);
                    let cy = start_y;

                    self.doc.add_shape(Shape::Rect {
                        layer_index: layer_idx,
                        x: cx,
                        y: cy,
                        width: box_w,
                        height: box_h,
                        corner_radius: 0.0,
                    });
                    new_sel.push(self.doc.shapes.len() - 1);

                    if self.depth_test_cut_outline {
                        self.doc.add_shape(Shape::Rect {
                            layer_index: label_layer,
                            x: cx,
                            y: cy,
                            width: box_w,
                            height: box_h,
                            corner_radius: 0.0,
                        });
                    }

                    let pwr_txt = format!("{:.0}%", pwr);
                    if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                        text: pwr_txt,
                        font_type: crate::text::FontType::SingleLine,
                        font_size_mm: 3.0,
                        x: cx + box_w * 0.5,
                        y: cy + box_h + 4.5,
                        align: crate::text::TextAlign::Center,
                        letter_spacing: 1.0,
                        line_spacing: 1.2,
                    }) {
                        for p in polys {
                            self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        }
                    }

                    if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                        text: ".__mm".to_string(),
                        font_type: crate::text::FontType::SingleLine,
                        font_size_mm: 2.2,
                        x: cx + box_w * 0.5,
                        y: cy + box_h + 8.5,
                        align: crate::text::TextAlign::Center,
                        letter_spacing: 1.0,
                        line_spacing: 1.2,
                    }) {
                        for p in polys {
                            self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        }
                    }
                }
            }
            DepthTestType::PassesVsSpeed => {
                let speeds = [60.0, 80.0, 100.0, 120.0];
                let num_passes_cols = 5;

                let title = format!(
                    "PASSES vs. SPEED DEPTH MATRIX — Power: {:.0}%, Interval: {:.2} mm{}",
                    self.depth_test_power,
                    self.depth_test_interval,
                    if self.depth_test_cross_hatch { " (Cross-Hatch)" } else { "" }
                );
                if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                    text: title,
                    font_type: crate::text::FontType::SingleLine,
                    font_size_mm: 3.5,
                    x: start_x + 15.0,
                    y: start_y - 12.0,
                    align: crate::text::TextAlign::Left,
                    letter_spacing: 1.0,
                    line_spacing: 1.2,
                }) {
                    for p in polys {
                        self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        new_sel.push(self.doc.shapes.len() - 1);
                    }
                }

                for c in 0..num_passes_cols {
                    let cx = start_x + 24.0 + c as f64 * (box_w + gap_x);
                    if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                        text: format!("{} Pass{}", c + 1, if c == 0 { "" } else { "es" }),
                        font_type: crate::text::FontType::SingleLine,
                        font_size_mm: 2.8,
                        x: cx + box_w * 0.5,
                        y: start_y - 4.5,
                        align: crate::text::TextAlign::Center,
                        letter_spacing: 1.0,
                        line_spacing: 1.2,
                    }) {
                        for p in polys {
                            self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        }
                    }
                }

                for (r, &spd) in speeds.iter().enumerate() {
                    let ry = start_y + r as f64 * (box_h + gap_y + 4.0);

                    if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                        text: format!("{:.0} mm/s", spd),
                        font_type: crate::text::FontType::SingleLine,
                        font_size_mm: 2.6,
                        x: start_x + 20.0,
                        y: ry + box_h * 0.6,
                        align: crate::text::TextAlign::Right,
                        letter_spacing: 1.0,
                        line_spacing: 1.2,
                    }) {
                        for p in polys {
                            self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        }
                    }

                    for c in 0..num_passes_cols {
                        let passes = c + 1;
                        let layer_idx = (r * num_passes_cols + c + 1).min(29);
                        let l = self.doc.get_or_create_layer(layer_idx);
                        l.name = format!("{:.0}mm/s - {}P", spd, passes);
                        l.color_hex = palette[(r + c) % palette.len()].to_string();
                        l.output = true;
                        l.cut_setting.mode = CutMode::Fill;
                        l.cut_setting.speed = spd;
                        l.cut_setting.power_max = self.depth_test_power;
                        l.cut_setting.num_passes = passes as u32;
                        l.cut_setting.line_interval = self.depth_test_interval;
                        l.cut_setting.cross_hatch = self.depth_test_cross_hatch;
                        l.cut_setting.scan_angle = 0.0;

                        let cx = start_x + 24.0 + c as f64 * (box_w + gap_x);

                        self.doc.add_shape(Shape::Rect {
                            layer_index: layer_idx,
                            x: cx,
                            y: ry,
                            width: box_w,
                            height: box_h,
                            corner_radius: 0.0,
                        });
                        new_sel.push(self.doc.shapes.len() - 1);

                        if self.depth_test_cut_outline {
                            self.doc.add_shape(Shape::Rect {
                                layer_index: label_layer,
                                x: cx,
                                y: ry,
                                width: box_w,
                                height: box_h,
                                corner_radius: 0.0,
                            });
                        }
                    }
                }
            }
            DepthTestType::LineInterval => {
                let intervals = [0.05, 0.075, 0.10, 0.125, 0.15];

                let title = format!(
                    "SCAN INTERVAL & SURFACE TEXTURE TEST — Power: {:.0}%, Speed: {:.0} mm/s",
                    self.depth_test_power,
                    self.depth_test_speed
                );
                if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                    text: title,
                    font_type: crate::text::FontType::SingleLine,
                    font_size_mm: 3.5,
                    x: start_x,
                    y: start_y - 12.0,
                    align: crate::text::TextAlign::Left,
                    letter_spacing: 1.0,
                    line_spacing: 1.2,
                }) {
                    for p in polys {
                        self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        new_sel.push(self.doc.shapes.len() - 1);
                    }
                }

                if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                    text: "Find the smoothest pocket floor without ridges for this wood species:".to_string(),
                    font_type: crate::text::FontType::SingleLine,
                    font_size_mm: 2.4,
                    x: start_x,
                    y: start_y - 5.5,
                    align: crate::text::TextAlign::Left,
                    letter_spacing: 1.0,
                    line_spacing: 1.2,
                }) {
                    for p in polys {
                        self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                    }
                }

                for (i, &intv) in intervals.iter().enumerate() {
                    let layer_idx = i + 1;
                    let l = self.doc.get_or_create_layer(layer_idx);
                    l.name = format!("{:.3}mm Interval", intv);
                    l.color_hex = palette[i % palette.len()].to_string();
                    l.output = true;
                    l.cut_setting.mode = CutMode::Fill;
                    l.cut_setting.speed = self.depth_test_speed;
                    l.cut_setting.power_max = self.depth_test_power;
                    l.cut_setting.num_passes = 1;
                    l.cut_setting.line_interval = intv;
                    l.cut_setting.cross_hatch = self.depth_test_cross_hatch;
                    l.cut_setting.scan_angle = 0.0;

                    let cx = start_x + i as f64 * (box_w + gap_x);
                    let cy = start_y;

                    self.doc.add_shape(Shape::Rect {
                        layer_index: layer_idx,
                        x: cx,
                        y: cy,
                        width: box_w,
                        height: box_h,
                        corner_radius: 0.0,
                    });
                    new_sel.push(self.doc.shapes.len() - 1);

                    if self.depth_test_cut_outline {
                        self.doc.add_shape(Shape::Rect {
                            layer_index: label_layer,
                            x: cx,
                            y: cy,
                            width: box_w,
                            height: box_h,
                            corner_radius: 0.0,
                        });
                    }

                    let dpi = (25.4 / intv).round();
                    if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                        text: format!("{:.2}mm", intv),
                        font_type: crate::text::FontType::SingleLine,
                        font_size_mm: 3.0,
                        x: cx + box_w * 0.5,
                        y: cy + box_h + 4.5,
                        align: crate::text::TextAlign::Center,
                        letter_spacing: 1.0,
                        line_spacing: 1.2,
                    }) {
                        for p in polys {
                            self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        }
                    }

                    if let Ok(polys) = crate::text::generate_text_polylines(&crate::text::TextOptions {
                        text: format!("{:.0} DPI", dpi),
                        font_type: crate::text::FontType::SingleLine,
                        font_size_mm: 2.2,
                        x: cx + box_w * 0.5,
                        y: cy + box_h + 8.5,
                        align: crate::text::TextAlign::Center,
                        letter_spacing: 1.0,
                        line_spacing: 1.2,
                    }) {
                        for p in polys {
                            self.doc.add_shape(Shape::Path { layer_index: label_layer, polyline: p });
                        }
                    }
                }
            }
        }

        self.selected_shape_indices = new_sel;
        self.image_textures.clear();
        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = "Generated Engraving Depth Test on canvas!".to_string();
    }

    /// Automatically detects the rectangular tile/workpiece on the bed from the camera feed
    /// and centers (and optionally scales & rotates) the selected shapes onto it.
    pub fn auto_fit_to_camera_tile(&mut self, scale_to_fit: bool, margin_mm: f64) {
        let frame_opt = self
            .last_raw_frame
            .clone()
            .or_else(|| self.camera_manager.get_latest_frame());

        let Some(raw_frame) = frame_opt else {
            self.status_msg = "No camera image available! Click 'Update Overlay' (Cmd+U) or Start Stream first.".to_string();
            return;
        };

        let undistorted = self
            .lens_undistorter
            .undistort_frame(&raw_frame, &self.camera_calibration);

        let Some(tile) = crate::camera::detect_workpiece_on_bed(
            &undistorted,
            &self.camera_calibration,
            self.doc.width,
            self.doc.height,
        ) else {
            self.status_msg = "Could not detect a distinct tile/workpiece on the bed. Ensure good contrast between tile and bed.".to_string();
            return;
        };

        if self.selected_shape_indices.is_empty() && !self.doc.shapes.is_empty() {
            self.selected_shape_indices = (0..self.doc.shapes.len()).collect();
        }
        if self.selected_shape_indices.is_empty() {
            self.status_msg = format!(
                "Detected tile at ({:.1}, {:.1}) mm, size {:.1}x{:.1} mm (angle {:+.1}°), but no shapes to fit!",
                tile.center_x, tile.center_y, tile.width, tile.height, tile.angle_deg
            );
            return;
        }

        self.push_undo();

        if let Some(bounds) = self.selection_bounds() {
            let cur_center = bounds.center();
            let dx = tile.center_x - cur_center.x;
            let dy = tile.center_y - cur_center.y;

            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                    self.doc.shapes[idx].translate(dx, dy);
                }
            }

            if scale_to_fit {
                let target_w = (tile.width - margin_mm * 2.0).max(5.0);
                let target_h = (tile.height - margin_mm * 2.0).max(5.0);
                let sx = target_w / bounds.width().max(1e-4);
                let sy = target_h / bounds.height().max(1e-4);
                let s = if self.lock_aspect_ratio { sx.min(sy) } else { sx };
                let sy_final = if self.lock_aspect_ratio { s } else { sy };
                let center_pt = Point2D::new(tile.center_x, tile.center_y);
                for &idx in &self.selected_shape_indices {
                    if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                        self.doc.shapes[idx].scale(center_pt, s, sy_final);
                    }
                }
            }

            // If tile is rotated noticeably (> 0.5 deg) and not near 45 deg symmetry ambiguity, align vector shapes
            if tile.angle_deg.abs() > 0.5 && tile.angle_deg.abs() < 40.0 {
                let center_pt = Point2D::new(tile.center_x, tile.center_y);
                let rad = tile.angle_deg.to_radians();
                for &idx in &self.selected_shape_indices {
                    if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                        self.doc.shapes[idx].rotate(center_pt, rad);
                    }
                }
            }
        }

        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = format!(
            "Auto-fitted to detected tile: Center ({:.1}, {:.1}) mm, Size {:.1}x{:.1} mm, Angle {:+.1}°",
            tile.center_x, tile.center_y, tile.width, tile.height, tile.angle_deg
        );
    }

    pub fn load_sample_gear(&mut self, cx: f64, cy: f64, teeth: usize, r_inner: f64, r_outer: f64) {
        self.push_undo();
        // Center bore
        self.doc.add_shape(Shape::Circle {
            layer_index: 0,
            center: Point2D::new(cx, cy),
            radius: 12.0,
        });

        // 4 mounting holes
        for i in 0..4 {
            let a = (i as f64) * std::f64::consts::FRAC_PI_2;
            self.doc.add_shape(Shape::Circle {
                layer_index: 0,
                center: Point2D::new(cx + 25.0 * a.cos(), cy + 25.0 * a.sin()),
                radius: 4.0,
            });
        }

        // Outer gear perimeter
        let mut gear_pts = Vec::new();
        for i in 0..teeth {
            let a0 = (i as f64 / teeth as f64) * std::f64::consts::TAU;
            let a1 = a0 + (0.28 / teeth as f64) * std::f64::consts::TAU;
            let a2 = a0 + (0.50 / teeth as f64) * std::f64::consts::TAU;
            let a3 = a0 + (0.78 / teeth as f64) * std::f64::consts::TAU;

            gear_pts.push(Point2D::new(cx + r_inner * a0.cos(), cy + r_inner * a0.sin()));
            gear_pts.push(Point2D::new(cx + r_outer * a1.cos(), cy + r_outer * a1.sin()));
            gear_pts.push(Point2D::new(cx + r_outer * a2.cos(), cy + r_outer * a2.sin()));
            gear_pts.push(Point2D::new(cx + r_inner * a3.cos(), cy + r_inner * a3.sin()));
        }

        self.doc.add_shape(Shape::Path {
            layer_index: 0,
            polyline: Polyline::new(gear_pts, true),
        });
    }

    /// Loads a project file (.lbrn2, .lbrn, .svg, .png, .jpg, .bmp) into the editor
    pub fn open_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), String> {
        let p = path.as_ref();
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        self.image_textures.clear();

        match ext.as_str() {
            "lbrn2" | "lbrn" => {
                let mut doc = Lbrn2File::load_from_file(p)?;
                doc.width = self.device.settings.bed_width;
                doc.height = self.device.settings.bed_height;
                self.doc = doc;
                self.selected_shape_indices = if !self.doc.shapes.is_empty() { vec![0] } else { vec![] };
                if let Some(first_shape) = self.doc.shapes.first() {
                    self.selected_layer = first_shape.layer_index();
                } else if let Some(&first_layer) = self.doc.layers.keys().next() {
                    self.selected_layer = first_layer;
                } else {
                    self.selected_layer = 0;
                }
                self.zoom = 1.2;
                self.pan = egui::Vec2::new(80.0, 60.0);
                self.status_msg = format!(
                    "Loaded: {} ({} shapes, {} layers)",
                    p.file_name().unwrap_or_default().to_string_lossy(),
                    self.doc.shapes.len(),
                    self.doc.layers.len()
                );
                Ok(())
            }
            "svg" => {
                let mut doc = SvgFormat::load_from_file(p)?;
                doc.width = self.device.settings.bed_width;
                doc.height = self.device.settings.bed_height;
                self.doc = doc;
                self.selected_shape_indices = if !self.doc.shapes.is_empty() { vec![0] } else { vec![] };
                if let Some(first_shape) = self.doc.shapes.first() {
                    self.selected_layer = first_shape.layer_index();
                } else if let Some(&first_layer) = self.doc.layers.keys().next() {
                    self.selected_layer = first_layer;
                } else {
                    self.selected_layer = 0;
                }
                self.zoom = 1.2;
                self.pan = egui::Vec2::new(80.0, 60.0);
                self.status_msg = format!(
                    "Loaded SVG: {} ({} shapes, {} layers)",
                    p.file_name().unwrap_or_default().to_string_lossy(),
                    self.doc.shapes.len(),
                    self.doc.layers.len()
                );
                Ok(())
            }
            "dxf" => {
                let mut doc = crate::formats::DxfFormat::load_from_file(p)?;
                doc.width = self.device.settings.bed_width;
                doc.height = self.device.settings.bed_height;
                self.doc = doc;
                self.selected_shape_indices = if !self.doc.shapes.is_empty() { vec![0] } else { vec![] };
                if let Some(first_shape) = self.doc.shapes.first() {
                    self.selected_layer = first_shape.layer_index();
                } else if let Some(&first_layer) = self.doc.layers.keys().next() {
                    self.selected_layer = first_layer;
                } else {
                    self.selected_layer = 0;
                }
                self.zoom = 1.2;
                self.pan = egui::Vec2::new(80.0, 60.0);
                self.status_msg = format!(
                    "Loaded DXF: {} ({} shapes, {} layers)",
                    p.file_name().unwrap_or_default().to_string_lossy(),
                    self.doc.shapes.len(),
                    self.doc.layers.len()
                );
                Ok(())
            }
            "pdf" => {
                let mut doc = crate::formats::PdfFormat::load_from_file(p)?;
                doc.width = self.device.settings.bed_width;
                doc.height = self.device.settings.bed_height;
                self.doc = doc;
                self.selected_shape_indices = if !self.doc.shapes.is_empty() { vec![0] } else { vec![] };
                if let Some(first_shape) = self.doc.shapes.first() {
                    self.selected_layer = first_shape.layer_index();
                } else if let Some(&first_layer) = self.doc.layers.keys().next() {
                    self.selected_layer = first_layer;
                } else {
                    self.selected_layer = 0;
                }
                self.zoom = 1.2;
                self.pan = egui::Vec2::new(80.0, 60.0);
                self.status_msg = format!(
                    "Loaded PDF: {} ({} shapes, {} layers)",
                    p.file_name().unwrap_or_default().to_string_lossy(),
                    self.doc.shapes.len(),
                    self.doc.layers.len()
                );
                Ok(())
            }
            "png" | "jpg" | "jpeg" | "bmp" => {
                let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Image");
                self.doc = Document::new(name, self.device.settings.bed_width, self.device.settings.bed_height);
                self.import_image(p)?;
                self.zoom = 1.2;
                self.pan = egui::Vec2::new(80.0, 60.0);
                Ok(())
            }
            "glb" | "gltf" | "stl" => {
                let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("3D_Relief");
                self.doc = Document::new(name, self.device.settings.bed_width, self.device.settings.bed_height);
                self.import_3d_mesh(p)?;
                self.zoom = 1.2;
                self.pan = egui::Vec2::new(80.0, 60.0);
                Ok(())
            }
            _ => Err(format!("Unsupported file format: .{}", ext)),
        }
    }

    /// Loads the built-in sample AutoCAD DXF mechanical mounting bracket
    pub fn load_sample_dxf(&mut self) {
        self.push_undo();
        let sample_str = include_str!("../../samples/sample_bracket.dxf");
        match crate::formats::DxfFormat::load_from_str(sample_str) {
            Ok(mut doc) => {
                doc.width = self.device.settings.bed_width;
                doc.height = self.device.settings.bed_height;
                self.doc = doc;
                self.selected_shape_indices = if !self.doc.shapes.is_empty() { vec![0] } else { vec![] };
                if let Some(first_shape) = self.doc.shapes.first() {
                    self.selected_layer = first_shape.layer_index();
                } else if let Some(&first_layer) = self.doc.layers.keys().next() {
                    self.selected_layer = first_layer;
                }
                self.zoom = 1.2;
                self.pan = egui::Vec2::new(80.0, 60.0);
                self.status_msg = format!("Loaded Sample AutoCAD DXF ({} shapes, {} layers)", self.doc.shapes.len(), self.doc.layers.len());
            }
            Err(e) => {
                self.status_msg = format!("Failed to load sample DXF: {}", e);
            }
        }
    }

    /// Loads the built-in sample Adobe / Vector PDF test card
    pub fn load_sample_pdf(&mut self) {
        self.push_undo();
        let sample_bytes = include_bytes!("../../samples/sample_vector.pdf");
        match crate::formats::PdfFormat::load_from_bytes(sample_bytes) {
            Ok(mut doc) => {
                doc.width = self.device.settings.bed_width;
                doc.height = self.device.settings.bed_height;
                self.doc = doc;
                self.selected_shape_indices = if !self.doc.shapes.is_empty() { vec![0] } else { vec![] };
                if let Some(first_shape) = self.doc.shapes.first() {
                    self.selected_layer = first_shape.layer_index();
                } else if let Some(&first_layer) = self.doc.layers.keys().next() {
                    self.selected_layer = first_layer;
                }
                self.zoom = 1.2;
                self.pan = egui::Vec2::new(80.0, 60.0);
                self.status_msg = format!("Loaded Sample Vector PDF ({} shapes, {} layers)", self.doc.shapes.len(), self.doc.layers.len());
            }
            Err(e) => {
                self.status_msg = format!("Failed to load sample PDF: {}", e);
            }
        }
    }

    /// Imports a raster image (PNG, JPG, BMP) as a photographic dither engraving shape into the document
    pub fn import_image<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), String> {
        let p = path.as_ref();
        let dyn_img = image::open(p).map_err(|e| format!("Failed to open image {}: {}", p.display(), e))?;
        let rgba = dyn_img.to_rgba8();
        let (raw_w, raw_h) = (rgba.width(), rgba.height());
        let raw_pixels = rgba.into_raw();

        self.push_undo();

        // Calculate initial physical size (default max 100mm, preserving aspect ratio)
        let aspect = raw_h as f64 / raw_w as f64;
        let mut width_mm = 100.0_f64.min(self.device.settings.bed_width * 0.7);
        let mut height_mm = width_mm * aspect;
        if height_mm > self.device.settings.bed_height * 0.7 {
            height_mm = self.device.settings.bed_height * 0.7;
            width_mm = height_mm / aspect;
        }

        // Center nicely on laser bed
        let x = ((self.device.settings.bed_width - width_mm) * 0.5).max(10.0);
        let y = ((self.device.settings.bed_height - height_mm) * 0.5).max(10.0);

        // Find or create an Image layer configured for Atkinson dither engraving
        let image_layer_idx = 1;
        if let Some(l) = self.doc.layers.get_mut(&image_layer_idx) {
            l.cut_setting.mode = CutMode::Image;
            l.cut_setting.dither_mode = DitherMode::Atkinson;
            l.cut_setting.speed = 100.0;
            l.cut_setting.power_max = 60.0;
            l.cut_setting.line_interval = 0.08;
        } else {
            let mut l = Layer::new(image_layer_idx, "Engrave Image", "#00A0FF");
            l.cut_setting.mode = CutMode::Image;
            l.cut_setting.dither_mode = DitherMode::Atkinson;
            l.cut_setting.speed = 100.0;
            l.cut_setting.power_max = 60.0;
            l.cut_setting.line_interval = 0.08;
            self.doc.layers.insert(image_layer_idx, l);
        }

        let shape = Shape::Image {
            layer_index: image_layer_idx,
            x,
            y,
            width: width_mm,
            height: height_mm,
            image_path: Some(p.to_string_lossy().to_string()),
            raw_pixels: Some(raw_pixels),
            raw_width: raw_w,
            raw_height: raw_h,
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
            sharpen: 0.0,
        };

        self.doc.add_shape(shape);
        let new_idx = self.doc.shapes.len() - 1;
        self.selected_shape_indices = vec![new_idx];
        self.selected_layer = image_layer_idx;
        self.image_textures.remove(&new_idx);
        self.status_msg = format!(
            "Imported image: {} ({}x{} px, {:.1}x{:.1} mm)",
            p.file_name().unwrap_or_default().to_string_lossy(),
            raw_w,
            raw_h,
            width_mm,
            height_mm
        );
        Ok(())
    }

    /// Loads a 3D model (.glb, .gltf, .stl), projects it to an orthographic heightmap,
    /// and configures it automatically for 3D Sliced Relief Engraving!
    pub fn import_3d_mesh<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), String> {
        let p = path.as_ref();
        let mesh = crate::formats::Mesh3D::load_file(p)?;
        let num_triangles = mesh.triangles.len();

        let (min_pt, max_pt) = mesh.bounds();
        let dx = (max_pt[0] - min_pt[0]).abs();
        let dy = (max_pt[1] - min_pt[1]).abs();
        let dz = (max_pt[2] - min_pt[2]).abs();

        let (raw_pixels, raw_w, raw_h) = mesh.render_heightmap(1024, crate::formats::ProjectionAxis::Auto)?;

        self.push_undo();

        let mut dims = [dx, dy, dz];
        dims.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let natural_w = dims[2] as f64;

        let aspect = raw_h as f64 / raw_w as f64;
        let (width_mm, height_mm) = if natural_w >= 10.0 && natural_w <= self.device.settings.bed_width * 0.7 {
            (natural_w, natural_w * aspect)
        } else {
            let w = 50.0_f64.min(self.device.settings.bed_width * 0.7);
            (w, w * aspect)
        };

        let x = ((self.device.settings.bed_width - width_mm) * 0.5).max(10.0);
        let y = ((self.device.settings.bed_height - height_mm) * 0.5).max(10.0);

        let relief_layer_idx = 1;
        if let Some(l) = self.doc.layers.get_mut(&relief_layer_idx) {
            l.name = "3D Sliced Relief".to_string();
            l.color_hex = "#FF9900".to_string();
            l.cut_setting.mode = CutMode::Image;
            l.cut_setting.dither_mode = DitherMode::Sliced3D;
            l.cut_setting.relief_slices = 16;
            l.cut_setting.relief_material_thickness = 3.0;
            l.cut_setting.relief_depth_mm = 1.2;
            l.cut_setting.relief_alternate_angle = true;
            l.cut_setting.speed = 100.0;
            l.cut_setting.power_max = 70.0;
            l.cut_setting.line_interval = 0.08;
        } else {
            let mut l = Layer::new(relief_layer_idx, "3D Sliced Relief", "#FF9900");
            l.cut_setting.mode = CutMode::Image;
            l.cut_setting.dither_mode = DitherMode::Sliced3D;
            l.cut_setting.relief_slices = 16;
            l.cut_setting.relief_material_thickness = 3.0;
            l.cut_setting.relief_depth_mm = 1.2;
            l.cut_setting.relief_alternate_angle = true;
            l.cut_setting.speed = 100.0;
            l.cut_setting.power_max = 70.0;
            l.cut_setting.line_interval = 0.08;
            self.doc.layers.insert(relief_layer_idx, l);
        }

        let shape = Shape::Image {
            layer_index: relief_layer_idx,
            x,
            y,
            width: width_mm,
            height: height_mm,
            image_path: Some(p.to_string_lossy().to_string()),
            raw_pixels: Some(raw_pixels),
            raw_width: raw_w as u32,
            raw_height: raw_h as u32,
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
            sharpen: 0.0,
        };

        self.doc.add_shape(shape);
        let new_idx = self.doc.shapes.len() - 1;
        self.selected_shape_indices = vec![new_idx];
        self.selected_layer = relief_layer_idx;
        self.image_textures.remove(&new_idx);
        self.status_msg = format!(
            "Loaded 3D Model: {} ({} triangles -> {:.1}x{:.1} mm 3D Relief)",
            p.file_name().unwrap_or_default().to_string_lossy(),
            num_triangles,
            width_mm,
            height_mm
        );
        Ok(())
    }

    /// Imports vector shapes from a DXF, PDF, SVG, or LBRN2 file and appends them to the current project
    pub fn import_vector_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), String> {
        let p = path.as_ref();
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        let imported_doc = match ext.as_str() {
            "dxf" => crate::formats::DxfFormat::load_from_file(p)?,
            "pdf" => crate::formats::PdfFormat::load_from_file(p)?,
            "svg" => crate::formats::SvgFormat::load_from_file(p)?,
            "lbrn2" | "lbrn" => crate::formats::Lbrn2File::load_from_file(p)?,
            _ => return Err(format!("Unsupported vector format: .{}", ext)),
        };

        self.push_undo();
        let shapes_count = imported_doc.shapes.len();
        let mut new_indices = Vec::new();

        // Merge imported layers into current document
        for (layer_idx, layer) in imported_doc.layers {
            if !self.doc.layers.contains_key(&layer_idx) {
                self.doc.layers.insert(layer_idx, layer);
            }
        }

        // Add imported shapes to current document
        for s in imported_doc.shapes {
            self.doc.add_shape(s);
            new_indices.push(self.doc.shapes.len() - 1);
        }

        self.selected_shape_indices = new_indices;
        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = format!(
            "Imported: {} ({} vector shapes added)",
            p.file_name().unwrap_or_default().to_string_lossy(),
            shapes_count
        );
        Ok(())
    }

    /// Asynchronously prompts the user to open any supported project or graphic file
    pub fn prompt_open_project(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter(
                        "All Supported Files (*.lbrn2, *.svg, *.dxf, *.pdf, *.png, *.jpg, *.bmp, *.glb, *.stl)",
                        &[
                            "lbrn2", "LBRN2", "lbrn", "LBRN", "svg", "SVG", "dxf", "DXF", "pdf", "PDF",
                            "png", "PNG", "jpg", "JPG", "jpeg", "JPEG", "bmp", "BMP",
                            "glb", "GLB", "gltf", "GLTF", "stl", "STL",
                        ],
                    )
                    .add_filter("3D Models for Relief (*.glb, *.gltf, *.stl)", &["glb", "GLB", "gltf", "GLTF", "stl", "STL"])
                    .add_filter("CAD Vector Drawing (*.dxf)", &["dxf", "DXF"])
                    .add_filter("Adobe / Vector PDF (*.pdf)", &["pdf", "PDF"])
                    .add_filter("LightBurn Projects (*.lbrn2, *.lbrn)", &["lbrn2", "LBRN2", "lbrn", "LBRN"])
                    .add_filter("Vector SVG (*.svg)", &["svg", "SVG"])
                    .add_filter(
                        "Raster Images (*.png, *.jpg, *.jpeg, *.bmp)",
                        &["png", "PNG", "jpg", "JPG", "jpeg", "JPEG", "bmp", "BMP"],
                    )
                    .pick_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::OpenProject(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to import an image or vector graphic into the current document
    pub fn prompt_import_image(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter(
                        "All Graphic, Vector & 3D Files (*.dxf, *.pdf, *.svg, *.png, *.jpg, *.glb, *.stl)",
                        &[
                            "dxf", "DXF", "pdf", "PDF", "svg", "SVG", "png", "PNG", "jpg", "JPG", "jpeg",
                            "JPEG", "bmp", "BMP", "glb", "GLB", "gltf", "GLTF", "stl", "STL",
                        ],
                    )
                    .add_filter("3D Models for Relief (*.glb, *.gltf, *.stl)", &["glb", "GLB", "gltf", "GLTF", "stl", "STL"])
                    .add_filter("CAD Vector Drawing (*.dxf)", &["dxf", "DXF"])
                    .add_filter("Adobe / Vector PDF (*.pdf)", &["pdf", "PDF"])
                    .add_filter("Vector SVG (*.svg)", &["svg", "SVG"])
                    .add_filter(
                        "Raster Images (*.png, *.jpg, *.jpeg, *.bmp)",
                        &["png", "PNG", "jpg", "JPG", "jpeg", "JPEG", "bmp", "BMP"],
                    )
                    .pick_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::ImportImage(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to save the current project without blocking the UI thread or freezing winit
    pub fn prompt_save_project(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        let default_name = if self.doc.name.is_empty() || self.doc.name == "Untitled" || self.doc.name == "LaserBurn_Project" || self.doc.name == "PhotonCAM_Project" {
            "project.lbrn2".to_string()
        } else if self.doc.name.ends_with(".lbrn2") {
            self.doc.name.clone()
        } else {
            format!("{}.lbrn2", self.doc.name)
        };
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("LightBurn Project (*.lbrn2)", &["lbrn2"])
                    .set_file_name(&default_name)
                    .save_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::SaveProject(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to export optimized G-Code
    pub fn prompt_export_gcode(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        let default_name = format!("{}.gcode", self.doc.name.trim_end_matches(".lbrn2"));
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("G-Code (*.gcode, *.nc)", &["gcode", "nc"])
                    .set_file_name(&default_name)
                    .save_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::ExportGCode(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to export Ruida binary .rd
    pub fn prompt_export_ruida(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        let default_name = format!("{}.rd", self.doc.name.trim_end_matches(".lbrn2"));
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("Ruida Binary (*.rd)", &["rd"])
                    .set_file_name(&default_name)
                    .save_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::ExportRuida(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to browse for a font file
    pub fn prompt_browse_font(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("Fonts (*.ttf, *.otf, *.ttc)", &["ttf", "otf", "ttc"])
                    .pick_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::BrowseFont(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to export all settings, camera calibration, and layer materials into a profile file
    pub fn prompt_export_settings(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("PhotonCAM / LaserBurn Settings Profile (*.json, *.lbpref)", &["json", "lbpref"])
                    .set_file_name("PhotonCAM_Profile.json")
                    .save_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::ExportSettings(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to import and restore settings from an exported profile or settings file
    pub fn prompt_import_settings(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("PhotonCAM / LaserBurn Settings (*.json, *.lbpref)", &["json", "lbpref"])
                    .pick_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::ImportSettings(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to export the Material Library to a JSON file
    pub fn prompt_export_material_library(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("PhotonCAM Material Library (*.json)", &["json"])
                    .set_file_name("PhotonCAM_materials.json")
                    .save_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::ExportMaterialLibrary(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Asynchronously prompts the user to import a Material Library from JSON or LightBurn .clb
    pub fn prompt_import_material_library(&self, ctx: &egui::Context) {
        let tx = self.file_dialog_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            pollster::block_on(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("Material Libraries (*.json, *.clb)", &["json", "clb"])
                    .pick_file()
                    .await
                {
                    let _ = tx.send(FileDialogAction::ImportMaterialLibrary(handle.path().to_path_buf()));
                    ctx.request_repaint();
                }
            });
        });
    }

    /// Fast, non-blocking cached simulation estimate for total job time and distances
    pub fn get_job_estimate(&mut self) -> crate::simulator::SimulationStats {
        if self.doc_version != self.last_sim_version {
            let sim = crate::simulator::Simulator::new();
            self.cached_estimate = sim.estimate_document(&self.doc, self.device.settings.rapid_speed);
            self.last_sim_version = self.doc_version;
        }
        self.cached_estimate.clone()
    }

    /// Combined bounding box of all currently selected shapes
    pub fn selection_bounds(&self) -> Option<BoundingBox> {
        let mut bb = BoundingBox::empty();
        let mut found = false;
        for &idx in &self.selected_shape_indices {
            if idx < self.doc.shapes.len() {
                bb = bb.union(&self.doc.shapes[idx].bounds());
                found = true;
            }
        }
        if found {
            Some(bb)
        } else {
            None
        }
    }

    /// Returns true if the shape at the given index is locked directly or belongs to a locked layer
    pub fn is_shape_locked(&self, shape_idx: usize) -> bool {
        if let Some(shape) = self.doc.shapes.get(shape_idx) {
            if shape.is_locked() {
                return true;
            }
            if let Some(layer) = self.doc.layers.get(&shape.layer_index()) {
                if layer.locked {
                    return true;
                }
            }
        }
        false
    }

    /// Smart hit testing: finds best shape matching a click point.
    /// Tests if click is inside the shape or near its outline.
    /// Prioritizes smallest enclosing shape so clicking a hole inside a gear selects the hole!
    /// Skips locked shapes and shapes on locked layers so background templates don't block clicks.
    pub fn find_best_shape_at_point(&self, pt: &Point2D, tolerance: f64) -> Option<usize> {
        let mut candidates: Vec<(usize, f64)> = Vec::new(); // (index, area)

        // Pass 1: exact hit test (inside shape or within tolerance of outline)
        for (idx, shape) in self.doc.shapes.iter().enumerate() {
            if self.is_shape_locked(idx) {
                continue;
            }
            if shape.hit_test(pt, tolerance) {
                let b = shape.bounds();
                let area = (b.width() * b.height()).max(1.0);
                candidates.push((idx, area));
            }
        }

        // Pass 2: slightly broader tolerance (3x) for easy clicking near thin lines / borders
        if candidates.is_empty() {
            for (idx, shape) in self.doc.shapes.iter().enumerate() {
                if self.is_shape_locked(idx) {
                    continue;
                }
                if shape.hit_test(pt, tolerance * 3.0) {
                    let b = shape.bounds();
                    let area = (b.width() * b.height()).max(1.0);
                    candidates.push((idx, area));
                }
            }
        }

        if candidates.is_empty() {
            return None;
        }

        // Sort by area ascending: smallest shape first (selects hole before outer gear)
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        Some(candidates[0].0)
    }

    /// Checks if a document point hits any of the 8 resize handles of the selection
    fn test_handles(&self, doc_pt: &Point2D, b: &BoundingBox, handle_radius: f64) -> DragHandle {
        let cx = (b.min_x + b.max_x) * 0.5;
        let cy = (b.min_y + b.max_y) * 0.5;

        let handles = [
            (DragHandle::TopLeft, Point2D::new(b.min_x, b.min_y)),
            (DragHandle::Top, Point2D::new(cx, b.min_y)),
            (DragHandle::TopRight, Point2D::new(b.max_x, b.min_y)),
            (DragHandle::Right, Point2D::new(b.max_x, cy)),
            (DragHandle::BottomRight, Point2D::new(b.max_x, b.max_y)),
            (DragHandle::Bottom, Point2D::new(cx, b.max_y)),
            (DragHandle::BottomLeft, Point2D::new(b.min_x, b.max_y)),
            (DragHandle::Left, Point2D::new(b.min_x, cy)),
        ];

        for (handle, pt) in handles {
            if doc_pt.distance_to(&pt) <= handle_radius {
                return handle;
            }
        }
        DragHandle::None
    }

    /// Groups currently selected shapes into a single Shape::Group
    pub fn group_selected(&mut self) {
        if self.selected_shape_indices.len() < 2 {
            self.status_msg = "Select at least 2 shapes to group".to_string();
            return;
        }
        self.push_undo();

        // Sort indices descending so removal doesn't shift earlier indices
        let mut sorted = self.selected_shape_indices.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        sorted.dedup();

        let mut group_shapes = Vec::new();
        let layer_idx = self.doc.shapes[sorted[0]].layer_index();

        for idx in sorted {
            if idx < self.doc.shapes.len() {
                group_shapes.push(self.doc.shapes.remove(idx));
            }
        }
        group_shapes.reverse();

        self.doc.shapes.push(Shape::Group {
            layer_index: layer_idx,
            shapes: group_shapes,
            locked: false,
        });

        self.selected_shape_indices = vec![self.doc.shapes.len() - 1];
        self.status_msg = "Shapes locked together in a Group".to_string();
    }

    /// Ungroups selected Shape::Group back into individual shapes
    pub fn ungroup_selected(&mut self) {
        if self.selected_shape_indices.is_empty() {
            return;
        }
        self.push_undo();
        let mut new_selection = Vec::new();
        let mut sorted = self.selected_shape_indices.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        sorted.dedup();

        for idx in sorted {
            if idx < self.doc.shapes.len() {
                if let Shape::Group { shapes, .. } = self.doc.shapes.remove(idx) {
                    for s in shapes {
                        self.doc.shapes.push(s);
                        new_selection.push(self.doc.shapes.len() - 1);
                    }
                }
            }
        }

        if !new_selection.is_empty() {
            self.selected_shape_indices = new_selection;
            self.status_msg = "Group(s) unpacked into individual shapes".to_string();
        } else {
            self.status_msg = "No groups found in current selection to ungroup".to_string();
        }
    }

    /// Locks or unlocks selected shape(s) in place
    pub fn toggle_lock_selected(&mut self) {
        if self.selected_shape_indices.is_empty() {
            return;
        }
        self.push_undo();

        let any_locked = self.selected_shape_indices.iter().any(|&i| {
            i < self.doc.shapes.len() && self.doc.shapes[i].is_locked()
        });

        if any_locked {
            // Unlock selected shapes
            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() {
                    if let Shape::Group { shapes, locked, .. } = &mut self.doc.shapes[idx] {
                        *locked = false;
                        if shapes.len() == 1 {
                            let inner = shapes.remove(0);
                            self.doc.shapes[idx] = inner;
                        }
                    }
                }
            }
            self.status_msg = "Selection unlocked (can be moved and edited)".to_string();
        } else {
            // Lock selected shapes
            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() {
                    if let Shape::Group { ref mut locked, .. } = self.doc.shapes[idx] {
                        *locked = true;
                    } else {
                        let orig = self.doc.shapes[idx].clone();
                        let layer = orig.layer_index();
                        self.doc.shapes[idx] = Shape::Group {
                            layer_index: layer,
                            shapes: vec![orig],
                            locked: true,
                        };
                    }
                }
            }
            self.status_msg = "Selection locked in place (cannot be moved accidentally)".to_string();
        }
    }

    /// Performs a 2D Boolean operation (Union / Weld, Subtract A - B, or Intersect A ∩ B)
    /// on the selected shapes, or joins open polylines end-to-end when Union is used on open paths.
    pub fn boolean_selected(&mut self, op: crate::geometry::BooleanOp) {
        let valid_indices: Vec<usize> = self
            .selected_shape_indices
            .iter()
            .copied()
            .filter(|&i| i < self.doc.shapes.len())
            .collect();

        // Allow self-union on a single Group (e.g. overlapping text / grouped shapes)
        if valid_indices.len() == 1 && op == crate::geometry::BooleanOp::Union {
            let idx = valid_indices[0];
            if let Shape::Group { ref shapes, locked, layer_index } = self.doc.shapes[idx].clone() {
                if shapes.len() >= 2 {
                    self.push_undo();
                    let mut acc = shapes[0].to_polylines();
                    for s in &shapes[1..] {
                        let b = s.to_polylines();
                        acc = crate::geometry::boolean_op_polylines(&acc, &b, crate::geometry::BooleanOp::Union);
                    }
                    if !acc.is_empty() {
                        let new_shape = if acc.len() == 1 {
                            Shape::Path {
                                layer_index,
                                polyline: acc.into_iter().next().unwrap(),
                            }
                        } else {
                            Shape::Group {
                                layer_index,
                                shapes: acc
                                    .into_iter()
                                    .map(|polyline| Shape::Path { layer_index, polyline })
                                    .collect(),
                                locked,
                            }
                        };
                        self.doc.shapes[idx] = new_shape;
                        self.doc_version = self.doc_version.wrapping_add(1);
                        self.status_msg = "Welded group shapes into unified contour".to_string();
                        return;
                    }
                }
            }
        }

        if valid_indices.len() < 2 {
            self.status_msg = "Select at least 2 shapes (Shift+Click) for Boolean operations".to_string();
            return;
        }

        self.push_undo();

        let first_idx = valid_indices[0];
        let layer = self.doc.shapes[first_idx].layer_index();

        // Check if any selected shape has closed contours
        let any_closed = valid_indices.iter().any(|&i| {
            self.doc.shapes[i]
                .to_polylines()
                .iter()
                .any(|p| p.is_closed && p.points.len() >= 3)
        });

        if any_closed || op != crate::geometry::BooleanOp::Union {
            let mut acc = self.doc.shapes[first_idx].to_polylines();
            for &idx in &valid_indices[1..] {
                let b_polys = self.doc.shapes[idx].to_polylines();
                acc = crate::geometry::boolean_op_polylines(&acc, &b_polys, op);
            }

            // Remove original selected shapes in descending index order
            let mut to_remove = valid_indices.clone();
            to_remove.sort_unstable_by(|a, b| b.cmp(a));
            to_remove.dedup();
            for idx in to_remove {
                if idx < self.doc.shapes.len() {
                    self.doc.shapes.remove(idx);
                }
            }

            if acc.is_empty() {
                self.selected_shape_indices.clear();
                self.doc_version = self.doc_version.wrapping_add(1);
                self.status_msg = match op {
                    crate::geometry::BooleanOp::Union => "Boolean Union produced empty result".to_string(),
                    crate::geometry::BooleanOp::Subtract => "Boolean Subtract removed entire shape".to_string(),
                    crate::geometry::BooleanOp::Intersect => "Boolean Intersect: shapes do not overlap".to_string(),
                };
                return;
            }

            let count = acc.len();
            let new_shape = if count == 1 {
                Shape::Path {
                    layer_index: layer,
                    polyline: acc.into_iter().next().unwrap(),
                }
            } else {
                Shape::Group {
                    layer_index: layer,
                    shapes: acc
                        .into_iter()
                        .map(|polyline| Shape::Path {
                            layer_index: layer,
                            polyline,
                        })
                        .collect(),
                    locked: false,
                }
            };

            self.doc.shapes.push(new_shape);
            self.selected_shape_indices = vec![self.doc.shapes.len() - 1];
            self.doc_version = self.doc_version.wrapping_add(1);
            self.status_msg = match op {
                crate::geometry::BooleanOp::Union => format!("Boolean Union / Weld complete ({} contour(s))", count),
                crate::geometry::BooleanOp::Subtract => format!("Boolean Subtract (A - B) complete ({} contour(s))", count),
                crate::geometry::BooleanOp::Intersect => format!("Boolean Intersect (A ∩ B) complete ({} contour(s))", count),
            };
            return;
        }

        // Open polyline end-to-end join fallback
        let mut polylines = Vec::new();
        let mut sorted = valid_indices;
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        sorted.dedup();

        for idx in sorted {
            if idx < self.doc.shapes.len() {
                let s = self.doc.shapes.remove(idx);
                for p in s.to_polylines() {
                    polylines.push(p);
                }
            }
        }

        if polylines.is_empty() {
            return;
        }

        let mut current = polylines.remove(0);
        let tolerance = 1.0;

        while !polylines.is_empty() {
            let last_pt = *current.points.last().unwrap();
            let mut found_next = None;

            for (i, p) in polylines.iter().enumerate() {
                if p.points.first().unwrap().distance_to(&last_pt) <= tolerance {
                    found_next = Some((i, false));
                    break;
                } else if p.points.last().unwrap().distance_to(&last_pt) <= tolerance {
                    found_next = Some((i, true));
                    break;
                }
            }

            if let Some((idx, should_reverse)) = found_next {
                let mut next_poly = polylines.remove(idx);
                if should_reverse {
                    next_poly.reverse();
                }
                current.points.extend_from_slice(&next_poly.points[1..]);
            } else {
                break;
            }
        }

        if current.points.len() > 2 && current.points.first().unwrap().distance_to(current.points.last().unwrap()) <= tolerance {
            current.is_closed = true;
        }

        self.doc.shapes.push(Shape::Path {
            layer_index: layer,
            polyline: current,
        });

        for p in polylines {
            self.doc.shapes.push(Shape::Path {
                layer_index: layer,
                polyline: p,
            });
        }

        self.selected_shape_indices = vec![self.doc.shapes.len() - 1];
        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = "Joined open paths successfully".to_string();
    }

    /// Welds / unions selected shapes or joins open polylines that touch end-to-end
    pub fn weld_selected(&mut self) {
        self.boolean_selected(crate::geometry::BooleanOp::Union);
    }

    /// Rotates currently selected shapes around their collective center by angle in degrees
    pub fn rotate_selected(&mut self, angle_deg: f64) {
        if self.selected_shape_indices.is_empty() {
            return;
        }
        self.push_undo();
        if let Some(bounds) = self.selection_bounds() {
            let c = bounds.center();
            let rad = angle_deg.to_radians();
            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                    self.doc.shapes[idx].rotate(c, rad);
                }
            }
            self.status_msg = format!("Rotated selection by {:+.1}°", angle_deg);
        }
    }

    /// Flips currently selected shapes horizontally around their collective center
    pub fn flip_selected_h(&mut self) {
        if self.selected_shape_indices.is_empty() {
            return;
        }
        self.push_undo();
        if let Some(bounds) = self.selection_bounds() {
            let c = bounds.center();
            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                    self.doc.shapes[idx].scale(c, -1.0, 1.0);
                }
            }
            self.status_msg = "Flipped selection horizontally".to_string();
        }
    }

    /// Flips currently selected shapes vertically around their collective center
    pub fn flip_selected_v(&mut self) {
        if self.selected_shape_indices.is_empty() {
            return;
        }
        self.push_undo();
        if let Some(bounds) = self.selection_bounds() {
            let c = bounds.center();
            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                    self.doc.shapes[idx].scale(c, 1.0, -1.0);
                }
            }
            self.status_msg = "Flipped selection vertically".to_string();
        }
    }

    /// Converts selected primitive shapes (Rect, Circle) into editable vector Paths
    pub fn convert_selected_to_path(&mut self) {
        if self.selected_shape_indices.is_empty() {
            return;
        }
        self.push_undo();
        for &idx in &self.selected_shape_indices {
            if idx < self.doc.shapes.len() {
                let polys = self.doc.shapes[idx].to_polylines();
                let layer_idx = self.doc.shapes[idx].layer_index();
                if let Some(poly) = polys.into_iter().next() {
                    self.doc.shapes[idx] = Shape::Path {
                        layer_index: layer_idx,
                        polyline: poly,
                    };
                }
            }
        }
        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = "Converted selection to editable vector Path".to_string();
    }

    /// Deletes the currently selected node from the active path
    pub fn delete_selected_node(&mut self) {
        let Some(node_idx) = self.selected_node_index else { return; };
        let Some(&shape_idx) = self.selected_shape_indices.first() else { return; };
        if shape_idx >= self.doc.shapes.len() { return; }

        let can_delete = if let Shape::Path { ref polyline, .. } = self.doc.shapes[shape_idx] {
            let min_pts = if polyline.is_closed { 3 } else { 2 };
            polyline.points.len() > min_pts && node_idx < polyline.points.len()
        } else {
            false
        };

        if !can_delete {
            self.status_msg = "Cannot delete node: path requires at least minimum vertices".to_string();
            return;
        }

        self.push_undo();
        if let Shape::Path { ref mut polyline, .. } = self.doc.shapes[shape_idx] {
            polyline.points.remove(node_idx);
            self.selected_node_index = if polyline.points.is_empty() {
                None
            } else {
                Some(node_idx.min(polyline.points.len() - 1))
            };
            self.doc_version = self.doc_version.wrapping_add(1);
            self.status_msg = format!("Deleted node #{}", node_idx + 1);
        }
    }

    /// Inserts a new node into the active path near the given point
    pub fn insert_node_at_point(&mut self, pt: Point2D) -> bool {
        let Some(&shape_idx) = self.selected_shape_indices.first() else { return false; };
        if shape_idx >= self.doc.shapes.len() { return false; }

        let best_seg = if let Shape::Path { ref polyline, .. } = self.doc.shapes[shape_idx] {
            let n = polyline.points.len();
            if n < 2 { return false; }
            let num_segs = if polyline.is_closed { n } else { n - 1 };

            let mut seg = None;
            let mut min_d = f64::MAX;

            for i in 0..num_segs {
                let p1 = polyline.points[i];
                let p2 = if i + 1 < n { polyline.points[i + 1] } else { polyline.points[0] };
                let d = crate::geometry::point_to_segment_distance(&pt, &p1, &p2);
                if d < min_d {
                    min_d = d;
                    seg = Some(i);
                }
            }
            seg
        } else {
            None
        };

        if let Some(seg_idx) = best_seg {
            self.push_undo();
            if let Shape::Path { ref mut polyline, .. } = self.doc.shapes[shape_idx] {
                let insert_idx = seg_idx + 1;
                polyline.points.insert(insert_idx, pt);
                self.selected_node_index = Some(insert_idx);
                self.doc_version = self.doc_version.wrapping_add(1);
                self.status_msg = format!("Inserted new node at segment #{}", seg_idx + 1);
                return true;
            }
        }
        false
    }

    /// Toggles whether the selected path is closed or open
    pub fn toggle_selected_path_closed(&mut self) {
        let Some(&shape_idx) = self.selected_shape_indices.first() else { return; };
        if shape_idx >= self.doc.shapes.len() { return; }

        if matches!(self.doc.shapes[shape_idx], Shape::Path { .. }) {
            self.push_undo();
            if let Shape::Path { ref mut polyline, .. } = self.doc.shapes[shape_idx] {
                polyline.is_closed = !polyline.is_closed;
                self.doc_version = self.doc_version.wrapping_add(1);
                let state = if polyline.is_closed { "Closed" } else { "Open" };
                self.status_msg = format!("Toggled path state to: {}", state);
            }
        }
    }

    /// Copies selected shapes to the internal clipboard
    pub fn copy_selected(&mut self) {
        if self.selected_shape_indices.is_empty() {
            self.status_msg = "Nothing selected to copy".to_string();
            return;
        }
        self.clipboard_shapes.clear();
        for &idx in &self.selected_shape_indices {
            if idx < self.doc.shapes.len() {
                let mut s = self.doc.shapes[idx].clone();
                s.set_locked(false); // Pasted copies should start unlocked
                self.clipboard_shapes.push(s);
            }
        }
        let count = self.clipboard_shapes.len();
        self.status_msg = format!("Copied {} shape(s) to clipboard", count);
    }

    /// Cuts selected shapes to the clipboard and removes them from the document
    pub fn cut_selected(&mut self) {
        if self.selected_shape_indices.is_empty() {
            self.status_msg = "Nothing selected to cut".to_string();
            return;
        }
        self.copy_selected();
        self.delete_selected();
        let count = self.clipboard_shapes.len();
        self.status_msg = format!("Cut {} shape(s) to clipboard", count);
    }

    /// Pastes shapes from the clipboard into the document with a slight offset (+5mm)
    pub fn paste(&mut self) {
        if self.clipboard_shapes.is_empty() {
            self.status_msg = "Clipboard is empty".to_string();
            return;
        }
        self.push_undo();
        const PASTE_OFFSET: f64 = 5.0;
        let mut new_selection = Vec::new();
        for s in &mut self.clipboard_shapes {
            s.translate(PASTE_OFFSET, PASTE_OFFSET);
            self.doc.add_shape(s.clone());
            new_selection.push(self.doc.shapes.len() - 1);
        }
        let count = new_selection.len();
        self.selected_shape_indices = new_selection;
        self.status_msg = format!("Pasted {} shape(s)", count);
    }

    /// Duplicates selected shapes with a slight offset (+5mm) and selects the duplicates
    pub fn duplicate_selected(&mut self) {
        if self.selected_shape_indices.is_empty() {
            self.status_msg = "Nothing selected to duplicate".to_string();
            return;
        }
        self.push_undo();
        const DUP_OFFSET: f64 = 5.0;
        let mut new_selection = Vec::new();
        for &idx in &self.selected_shape_indices {
            if idx < self.doc.shapes.len() {
                let mut dup = self.doc.shapes[idx].clone();
                dup.set_locked(false);
                dup.translate(DUP_OFFSET, DUP_OFFSET);
                self.doc.add_shape(dup);
                new_selection.push(self.doc.shapes.len() - 1);
            }
        }
        let count = new_selection.len();
        self.selected_shape_indices = new_selection;
        self.status_msg = format!("Duplicated {} shape(s)", count);
    }

    /// Deletes currently selected shapes from the document (protects locked shapes)
    pub fn delete_selected(&mut self) {
        if self.selected_shape_indices.is_empty() {
            return;
        }
        self.push_undo();
        let mut sorted = self.selected_shape_indices.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        sorted.dedup();

        let mut deleted_count = 0;
        let mut locked_count = 0;
        for idx in sorted {
            if idx < self.doc.shapes.len() {
                if self.is_shape_locked(idx) {
                    locked_count += 1;
                } else {
                    self.doc.shapes.remove(idx);
                    deleted_count += 1;
                }
            }
        }
        if locked_count > 0 && deleted_count == 0 {
            self.status_msg = "Cannot delete locked shape(s) [LOCKED] (Unlock with Cmd+L first)".to_string();
        } else {
            self.selected_shape_indices.clear();
            if locked_count > 0 {
                self.status_msg = format!("Deleted {} shape(s); {} locked shape(s) preserved", deleted_count, locked_count);
            } else {
                self.status_msg = format!("Deleted {} shape(s)", deleted_count);
            }
        }
    }

    /// Converts user text into vector geometry and adds it to the document
    pub fn add_text_to_document(&mut self) {
        if self.text_input.trim().is_empty() {
            return;
        }
        self.push_undo();

        let font_type = if self.text_is_singleline {
            crate::text::FontType::SingleLine
        } else {
            let path = if self.text_font_path.is_empty() {
                crate::text::default_system_font()
            } else {
                self.text_font_path.clone()
            };
            crate::text::FontType::TrueType { path }
        };

        let opt = crate::text::TextOptions {
            text: self.text_input.clone(),
            font_type,
            font_size_mm: self.text_font_size,
            x: self.text_pos_x,
            y: self.text_pos_y,
            align: self.text_align,
            letter_spacing: 1.0,
            line_spacing: 1.2,
        };

        let polylines_res = if self.text_on_path_enabled {
            if let Some(&sel_idx) = self.selected_shape_indices.first() {
                if sel_idx < self.doc.shapes.len() {
                    let path_polys = self.doc.shapes[sel_idx].to_polylines();
                    if let Some(target_poly) = path_polys.first() {
                        let on_path = crate::text::TextOnPathOptions {
                            start_offset_mm: self.text_on_path_offset,
                            perpendicular_offset_mm: self.text_on_path_perpendicular,
                            align: self.text_align,
                            reverse_direction: self.text_on_path_reverse,
                        };
                        crate::text::generate_text_on_path(&opt, target_poly, &on_path)
                    } else {
                        crate::text::generate_text_polylines(&opt)
                    }
                } else {
                    crate::text::generate_text_polylines(&opt)
                }
            } else {
                crate::text::generate_text_polylines(&opt)
            }
        } else {
            crate::text::generate_text_polylines(&opt)
        };

        match polylines_res {
            Ok(polylines) => {
                if polylines.is_empty() {
                    self.status_msg = "No geometry generated for text".to_string();
                    return;
                }

                let layer_idx = self.selected_layer;
                let char_shapes: Vec<Shape> = polylines
                    .into_iter()
                    .map(|poly| Shape::Path {
                        layer_index: layer_idx,
                        polyline: poly,
                    })
                    .collect();

                let count = char_shapes.len();
                let text_group = Shape::Group {
                    layer_index: layer_idx,
                    shapes: char_shapes,
                    locked: false,
                };

                self.doc.add_shape(text_group);
                let new_idx = self.doc.shapes.len() - 1;
                self.selected_shape_indices = vec![new_idx];
                self.status_msg = format!("Added text: \"{}\" ({} paths, group #{})", self.text_input, count, new_idx + 1);
            }
            Err(e) => {
                self.status_msg = format!("Error generating text: {}", e);
            }
        }
    }

    /// Starts a bounding box framing test with weak light
    pub fn start_box_frame(&mut self) {
        let b = if let Some(bounds) = self.selection_bounds() {
            bounds
        } else {
            self.doc.bounds()
        };

        if b.width() > 0.0 && b.height() > 0.0 {
            self.device.start_box_frame_test(b.min_x, b.min_y, b.max_x, b.max_y);
            self.status_msg = format!("Running box framing test with weak beam: {:.1}x{:.1}mm", b.width(), b.height());
        } else {
            self.status_msg = "No shapes available for framing".to_string();
        }
    }

    /// Starts a contour trace framing test with weak light along exact vector paths
    pub fn start_contour_frame(&mut self) {
        let mut points = Vec::new();

        if !self.selected_shape_indices.is_empty() {
            for &idx in &self.selected_shape_indices {
                if idx < self.doc.shapes.len() {
                    for poly in self.doc.shapes[idx].to_polylines() {
                        points.extend_from_slice(&poly.points);
                        if poly.is_closed && !poly.points.is_empty() {
                            points.push(poly.points[0]);
                        }
                    }
                }
            }
        } else {
            for shape in &self.doc.shapes {
                for poly in shape.to_polylines() {
                    points.extend_from_slice(&poly.points);
                    if poly.is_closed && !poly.points.is_empty() {
                        points.push(poly.points[0]);
                    }
                }
            }
        }

        if !points.is_empty() {
            self.device.start_contour_frame_test(points);
            self.status_msg = "Running contour framing test with weak beam...".to_string();
        } else {
            self.status_msg = "No cut paths available to test".to_string();
        }
    }

    /// Convert document coordinate (Point2D in mm) to screen position (egui::Pos2),
    /// strictly preserving natural left-to-right and top-to-bottom orientation (never mirrored).
    pub fn doc_to_screen(&self, pt: Point2D, canvas_origin: egui::Pos2) -> egui::Pos2 {
        let bed_w = self.doc.width as f32 * self.zoom;
        let bed_h = self.doc.height as f32 * self.zoom;
        let bed_rect = egui::Rect::from_min_size(canvas_origin, egui::Vec2::new(bed_w, bed_h));

        egui::pos2(
            bed_rect.left() + pt.x as f32 * self.zoom,
            bed_rect.top() + pt.y as f32 * self.zoom,
        )
    }

    /// Convert screen position (egui::Pos2) to document coordinates (Point2D in mm),
    /// reversing zoom and pan without mirroring.
    pub fn screen_to_doc(&self, screen_pos: egui::Pos2, canvas_origin: egui::Pos2) -> Point2D {
        let bed_w = self.doc.width as f32 * self.zoom;
        let bed_h = self.doc.height as f32 * self.zoom;
        let bed_rect = egui::Rect::from_min_size(canvas_origin, egui::Vec2::new(bed_w, bed_h));

        Point2D::new(
            ((screen_pos.x - bed_rect.left()) / self.zoom) as f64,
            ((screen_pos.y - bed_rect.top()) / self.zoom) as f64,
        )
    }

    /// Calculate adaptive grid line spacing and measurement label spacing based on zoom level:
    /// Returns `(minor_step_mm, major_step_mm, label_step_mm)`.
    pub fn grid_steps_for_zoom(zoom: f32) -> (f64, f64, f64) {
        if zoom >= 10.0 {
            (0.5, 2.0, 2.0)
        } else if zoom >= 5.5 {
            (1.0, 5.0, 5.0)
        } else if zoom >= 2.8 {
            (2.0, 10.0, 10.0)
        } else if zoom >= 1.3 {
            (5.0, 20.0, 20.0)
        } else if zoom >= 0.6 {
            (10.0, 50.0, 50.0)
        } else {
            (50.0, 100.0, 100.0)
        }
    }

    /// Returns adaptive grid subdivisions and ruler steps respecting the active unit system (mm vs inches)
    pub fn grid_steps_for_zoom_unit(zoom: f32, unit: LengthUnit) -> (f64, f64, f64) {
        match unit {
            LengthUnit::Millimeters => Self::grid_steps_for_zoom(zoom),
            LengthUnit::Inches => {
                let (minor_in, major_in, label_in) = if zoom >= 10.0 {
                    (0.05, 0.25, 0.25)
                } else if zoom >= 5.5 {
                    (0.1, 0.5, 0.5)
                } else if zoom >= 2.8 {
                    (0.25, 1.0, 1.0)
                } else if zoom >= 1.3 {
                    (0.5, 1.0, 1.0)
                } else if zoom >= 0.6 {
                    (0.5, 2.0, 2.0)
                } else {
                    (1.0, 5.0, 5.0)
                };
                (minor_in * 25.4, major_in * 25.4, label_in * 25.4)
            }
        }
    }

    /// Paint live camera background overlay onto the laser bed canvas with perspective, scaling, and rotation
    pub fn paint_camera_on_bed(
        &self,
        painter: &egui::Painter,
        canvas_origin: egui::Pos2,
        texture: &egui::TextureHandle,
    ) {
        let cal = &self.camera_calibration;
        let doc_w = self.doc.width;
        let doc_h = self.doc.height;
        let alpha = (cal.opacity.clamp(0.05, 1.0) * 255.0) as u8;
        let color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha);

        // High-precision 4-Point Homography perspective mesh
        if cal.use_homography {
            if let Some(ref inv_h) = cal.inv_homography_matrix {
                let nx = 20;
                let ny = 20;
                let mut mesh = egui::Mesh::with_texture(texture.id());

                for k in 0..=ny {
                    let ty = k as f64 / ny as f64;
                    let doc_y = doc_h * ty;
                    for j in 0..=nx {
                        let tx = j as f64 / nx as f64;
                        let doc_x = doc_w * tx;

                        let screen_pos = self.doc_to_screen(Point2D::new(doc_x, doc_y), canvas_origin);
                        let (mut u, mut v) = crate::camera::project_homography(inv_h, doc_x, doc_y);

                        if cal.flip_h {
                            u = 1.0 - u;
                        }
                        if cal.flip_v {
                            v = 1.0 - v;
                        }

                        mesh.vertices.push(egui::epaint::Vertex {
                            pos: screen_pos,
                            uv: egui::pos2(u as f32, v as f32),
                            color,
                        });
                    }
                }

                for k in 0..ny {
                    for j in 0..nx {
                        let top_left = (k * (nx + 1) + j) as u32;
                        let top_right = top_left + 1;
                        let bottom_left = ((k + 1) * (nx + 1) + j) as u32;
                        let bottom_right = bottom_left + 1;

                        mesh.indices.extend_from_slice(&[
                            top_left, top_right, bottom_right,
                            top_left, bottom_right, bottom_left,
                        ]);
                    }
                }

                painter.add(egui::Shape::mesh(mesh));
                return;
            }
        }

        // Fallback: 4-corner affine transform
        let cx = doc_w * 0.5 + cal.offset_x_mm;
        let cy = doc_h * 0.5 + cal.offset_y_mm;

        let eff_scale_x = (cal.uniform_scale * cal.scale_x).max(0.01);
        let eff_scale_y = (cal.uniform_scale * cal.scale_y).max(0.01);

        let half_w = (doc_w * 0.5 * eff_scale_x).max(1.0);
        let half_h = (doc_h * 0.5 * eff_scale_y).max(1.0);

        let rot_rad = (cal.rotation_deg as f64).to_radians();
        let cos_a = rot_rad.cos();
        let sin_a = rot_rad.sin();

        // 4 local corners relative to center: TL, TR, BR, BL
        let corners = [
            (-half_w, -half_h),
            (half_w, -half_h),
            (half_w, half_h),
            (-half_w, half_h),
        ];

        let mut screen_corners = [egui::Pos2::ZERO; 4];
        for (i, &(lx, ly)) in corners.iter().enumerate() {
            let doc_x = cx + lx * cos_a - ly * sin_a;
            let doc_y = cy + lx * sin_a + ly * cos_a;
            screen_corners[i] = self.doc_to_screen(Point2D::new(doc_x, doc_y), canvas_origin);
        }

        let (u_min, u_max) = if cal.flip_h { (1.0, 0.0) } else { (0.0, 1.0) };
        let (v_min, v_max) = if cal.flip_v { (1.0, 0.0) } else { (0.0, 1.0) };

        let uvs = [
            egui::pos2(u_min, v_min),
            egui::pos2(u_max, v_min),
            egui::pos2(u_max, v_max),
            egui::pos2(u_min, v_max),
        ];

        let mut mesh = egui::Mesh::with_texture(texture.id());
        for i in 0..4 {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: screen_corners[i],
                uv: uvs[i],
                color,
            });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);

        painter.add(egui::Shape::mesh(mesh));
    }

    pub fn bed_snapshot_path() -> Option<std::path::PathBuf> {
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
        let dir = std::path::PathBuf::from(home).join(".config").join("laser_burn");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join("last_bed_snapshot.png"))
    }

    pub fn save_bed_snapshot_to_disk(&self, frame: &crate::camera::CameraFrame) {
        if let Some(path) = Self::bed_snapshot_path() {
            if let Some(img) = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(frame.width, frame.height, frame.rgba.clone()) {
                let _ = img.save(&path);
            }
        }
    }

    pub fn load_bed_snapshot_from_disk() -> Option<crate::camera::CameraFrame> {
        let path = Self::bed_snapshot_path()?;
        if !path.exists() {
            return None;
        }
        let img = image::open(&path).ok()?.to_rgba8();
        let (width, height) = img.dimensions();
        Some(crate::camera::CameraFrame {
            width,
            height,
            rgba: img.into_raw(),
            frame_number: 1,
        })
    }

    /// Trigger 1-click bed overlay update from camera (starts stream if needed, captures snapshot, saves to disk, and stops stream)
    pub fn trigger_update_overlay(&mut self) {
        if self.oneshot_snapshot.is_some() {
            return;
        }

        self.camera_calibration.show_on_canvas = true;
        let _ = self.camera_calibration.save();

        if self.camera_manager.is_streaming {
            if let Some(f) = self.camera_manager.take_snapshot() {
                self.save_bed_snapshot_to_disk(&f);
                self.last_raw_frame = Some(f);
                self.last_applied_calibration = None;
                self.status_msg = "Bed overlay updated.".to_string();
            }
        } else {
            let initial = self.camera_manager.get_latest_frame().map(|f| f.frame_number).unwrap_or(0);
            self.camera_manager.start_stream();
            self.oneshot_snapshot = Some(OneShotSnapshot {
                started_at: std::time::Instant::now(),
                was_streaming: false,
                initial_frame_number: initial,
            });
            self.status_msg = "Capturing fresh bed overlay photo from camera...".to_string();
        }
    }

    /// Calculate the 4 target coordinates in document mm space:
    /// 0: Top-Left (inset, inset)
    /// 1: Top-Right (width - inset, inset)
    /// 2: Bottom-Right (width - inset, height - inset)
    /// 3: Bottom-Left (inset, height - inset)
    pub fn alignment_target_doc_coords(&self) -> [(f64, f64); 4] {
        let inset = self.camera_calibration.target_inset_mm.clamp(10.0, self.doc.width.min(self.doc.height) * 0.45);
        let w = self.doc.width;
        let h = self.doc.height;
        [
            (inset, inset),           // 0: Top-Left
            (w - inset, inset),       // 1: Top-Right
            (w - inset, h - inset),   // 2: Bottom-Right
            (inset, h - inset),       // 3: Bottom-Left
        ]
    }

    /// Move the physical laser head directly over target index 0..3 with weak light spot turned on
    pub fn jog_to_alignment_target(&mut self, idx: usize) {
        let targets = self.alignment_target_doc_coords();
        if idx < 4 {
            let (doc_x, doc_y) = targets[idx];
            if !self.device.is_weak_light_on {
                self.device.set_weak_light(true);
            }
            self.device.go_to_point(Point2D::new(doc_x, doc_y));
            self.status_msg = format!("Laser positioned at Target {} (X: {:.1}, Y: {:.1} mm). Place paper under the laser spot!", idx + 1, doc_x, doc_y);
        }
    }

    /// Generate rapid G-code for 4 bullseye alignment targets (crosshairs + concentric circles)
    pub fn generate_alignment_targets_gcode(&self) -> String {
        let targets = self.alignment_target_doc_coords();
        let speed = self.camera_calibration.burn_speed.clamp(100.0, 10000.0);
        let max_s = self.device.settings.max_s_value;
        let power_s = (max_s * (self.camera_calibration.burn_power_percent.clamp(1.0, 100.0) as f64 / 100.0)).round();

        let mut lines = Vec::new();
        lines.push("; PhotonCAM 4-Point Camera Alignment Targets".to_string());
        lines.push("G21 ; millimeters".to_string());
        lines.push("G90 ; absolute coordinates".to_string());
        lines.push("M5 ; ensure laser off".to_string());

        for (idx, &(doc_x, doc_y)) in targets.iter().enumerate() {
            let m_pt = self.device.doc_to_machine(Point2D::new(doc_x, doc_y));
            let mx = m_pt.x;
            let my = m_pt.y;

            lines.push(format!("; Target {} at doc X{:.1} Y{:.1} -> machine X{:.1} Y{:.1}", idx + 1, doc_x, doc_y, mx, my));
            lines.push(format!("G0 X{:.3} Y{:.3} F{:.0}", mx, my, self.device.settings.rapid_speed));

            // Horizontal crosshair (10mm)
            lines.push(format!("G0 X{:.3} Y{:.3}", mx - 5.0, my));
            lines.push(format!("M3 S{:.0}", power_s));
            lines.push(format!("G1 X{:.3} Y{:.3} F{:.0}", mx + 5.0, my, speed));
            lines.push("M5".to_string());

            // Vertical crosshair (10mm)
            lines.push(format!("G0 X{:.3} Y{:.3}", mx, my - 5.0));
            lines.push(format!("M3 S{:.0}", power_s));
            lines.push(format!("G1 X{:.3} Y{:.3} F{:.0}", mx, my + 5.0, speed));
            lines.push("M5".to_string());

            // Inner circle (diameter 6mm)
            lines.push(format!("G0 X{:.3} Y{:.3}", mx + 3.0, my));
            lines.push(format!("M3 S{:.0}", power_s));
            lines.push(format!("G2 X{:.3} Y{:.3} I-3.0 J0.0 F{:.0}", mx + 3.0, my, speed));
            lines.push("M5".to_string());

            // Outer circle (diameter 12mm)
            lines.push(format!("G0 X{:.3} Y{:.3}", mx + 6.0, my));
            lines.push(format!("M3 S{:.0}", power_s));
            lines.push(format!("G2 X{:.3} Y{:.3} I-6.0 J0.0 F{:.0}", mx + 6.0, my, speed));
            lines.push("M5".to_string());
        }

        // Return to home/origin
        lines.push("M5 ; laser off".to_string());
        lines.push(format!("G0 X0 Y0 F{:.0}", self.device.settings.rapid_speed));
        lines.join("\n")
    }

    /// Burn the 4 alignment targets onto the scrap papers placed on the bed
    pub fn burn_alignment_targets(&mut self) {
        let gcode = self.generate_alignment_targets_gcode();
        self.device.start_job(&gcode, 30.0);
        self.status_msg = "Burning 4 alignment targets onto scrap papers...".to_string();
    }

    /// Screen position of the Origin point (0, 0) - positioned at the machine's origin corner
    pub fn origin_screen_pos(&self, canvas_origin: egui::Pos2) -> egui::Pos2 {
        let bed_w = self.doc.width as f32 * self.zoom;
        let bed_h = self.doc.height as f32 * self.zoom;
        let bed_rect = egui::Rect::from_min_size(canvas_origin, egui::Vec2::new(bed_w, bed_h));

        match self.device.settings.origin_corner {
            crate::device::OriginCorner::TopLeft => bed_rect.left_top(),
            crate::device::OriginCorner::TopRight => bed_rect.right_top(),
            crate::device::OriginCorner::BottomLeft => bed_rect.left_bottom(),
            crate::device::OriginCorner::BottomRight => bed_rect.right_bottom(),
        }
    }

    /// Convert machine coordinate (Point2D relative to machine origin corner) to screen position (egui::Pos2)
    pub fn machine_to_screen(&self, pt: Point2D, canvas_origin: egui::Pos2) -> egui::Pos2 {
        let bed_w = self.doc.width as f32 * self.zoom;
        let bed_h = self.doc.height as f32 * self.zoom;
        let bed_rect = egui::Rect::from_min_size(canvas_origin, egui::Vec2::new(bed_w, bed_h));

        match self.device.settings.origin_corner {
            crate::device::OriginCorner::TopLeft => egui::pos2(
                bed_rect.left() + pt.x as f32 * self.zoom,
                bed_rect.top() + pt.y as f32 * self.zoom,
            ),
            crate::device::OriginCorner::TopRight => egui::pos2(
                bed_rect.right() - pt.x as f32 * self.zoom,
                bed_rect.top() + pt.y as f32 * self.zoom,
            ),
            crate::device::OriginCorner::BottomLeft => egui::pos2(
                bed_rect.left() + pt.x as f32 * self.zoom,
                bed_rect.bottom() - pt.y as f32 * self.zoom,
            ),
            crate::device::OriginCorner::BottomRight => egui::pos2(
                bed_rect.right() - pt.x as f32 * self.zoom,
                bed_rect.bottom() - pt.y as f32 * self.zoom,
            ),
        }
    }

    /// Screen position of the physical laser head, relative to the machine's origin corner
    pub fn machine_head_screen_pos(&self, canvas_origin: egui::Pos2) -> egui::Pos2 {
        self.machine_to_screen(
            Point2D::new(self.device.machine_x, self.device.machine_y),
            canvas_origin,
        )
    }

    /// Starts real laser cutting / engraving job
    pub fn start_job(&mut self) {
        if self.doc.shapes.is_empty() {
            self.status_msg = "No shapes to cut!".to_string();
            return;
        }

        if self.device.status != crate::device::ConnectionStatus::Connected {
            self.status_msg = self.tr("Connect laser first (Connection Settings)!").to_string();
            self.show_connection_window = true;
            return;
        }

        let mut opt = CutOptimizer::new();
        opt.default_kerf = self.device.settings.default_kerf;
        let job = opt.optimize(&self.doc);

        let mut cfg = GCodeConfig::default();
        cfg.max_s_value = self.device.settings.max_s_value;
        cfg.rapid_speed = self.device.settings.rapid_speed;
        cfg.bed_width = self.device.settings.bed_width;
        cfg.bed_height = self.device.settings.bed_height;
        cfg.mirror_x = self.device.settings.origin_corner == crate::device::OriginCorner::TopRight
            || self.device.settings.origin_corner == crate::device::OriginCorner::BottomRight;
        cfg.mirror_y = self.device.settings.origin_corner == crate::device::OriginCorner::BottomLeft
            || self.device.settings.origin_corner == crate::device::OriginCorner::BottomRight;
        let gen = GCodeGenerator::new(cfg);
        let gcode = gen.generate(&self.doc, &job);

        let est = self.get_job_estimate();
        self.device.start_job(&gcode, est.total_estimated_time_sec);
        self.show_job_window = true;
        self.status_msg = format!("Cut job started ({} paths, ~{})...", job.paths.len(), est.formatted_time);
    }

    /// Theme-aware green accent color for labels on either Dark or Light backgrounds
    pub(crate) fn color_green(&self) -> egui::Color32 {
        if self.device.settings.dark_mode {
            egui::Color32::from_rgb(75, 225, 105)
        } else {
            egui::Color32::from_rgb(0, 135, 35)
        }
    }

    /// Theme-aware orange/amber accent color for labels on either Dark or Light backgrounds
    pub(crate) fn color_orange(&self) -> egui::Color32 {
        if self.device.settings.dark_mode {
            egui::Color32::from_rgb(255, 185, 50)
        } else {
            egui::Color32::from_rgb(195, 80, 0)
        }
    }

    pub fn start_web_remote(&mut self) {
        if self.web_remote_active {
            return;
        }
        let stopper = std::sync::Arc::new(std::sync::Mutex::new(true));
        self.web_remote_stopper = Some(std::sync::Arc::clone(&stopper));
        self.web_remote_active = true;
        let port = self.web_remote_port;

        std::thread::spawn(move || {
            let mut server = crate::server::BridgeServer::new(port, Some(2323));
            server.running = stopper;
            let _ = server.run();
        });
        self.status_msg = format!("Web Remote Server started on port {}", self.web_remote_port);
    }

    pub fn stop_web_remote(&mut self) {
        if let Some(stopper) = &self.web_remote_stopper {
            if let Ok(mut run) = stopper.lock() {
                *run = false;
            }
        }
        self.web_remote_active = false;
        self.web_remote_stopper = None;
        self.status_msg = "Web Remote Server stopped.".to_string();
    }

    /// Theme-aware blue/cyan accent color for labels on either Dark or Light backgrounds
    pub(crate) fn color_blue(&self) -> egui::Color32 {
        if self.device.settings.dark_mode {
            egui::Color32::from_rgb(85, 200, 255)
        } else {
            egui::Color32::from_rgb(0, 115, 200)
        }
    }

    /// Theme-aware red accent color for labels on either Dark or Light backgrounds
    pub(crate) fn color_red(&self) -> egui::Color32 {
        if self.device.settings.dark_mode {
            egui::Color32::from_rgb(255, 95, 95)
        } else {
            egui::Color32::from_rgb(200, 25, 25)
        }
    }

    /// Opens the Save Preset dialog, pre-populating fields with active layer info
    pub fn open_save_layer_dialog(&mut self) {
        if let Some(ref cat) = self.mat_lib_selected_category {
            self.save_mat_category = cat.clone();
        } else if self.save_mat_category.is_empty() {
            self.save_mat_category = "PhotonCAM Oletus (10W-20W)".to_string();
        }
        if let Some(layer) = self.doc.layers.get(&self.selected_layer) {
            if !layer.name.is_empty() && layer.name != format!("L{:02}", self.selected_layer) {
                self.save_mat_name = layer.name.clone();
            } else {
                self.save_mat_name = "Custom Material".to_string();
            }
            self.save_mat_op_name = match layer.cut_setting.mode {
                crate::document::CutMode::Line => "Cut".to_string(),
                crate::document::CutMode::Fill => "Engrave".to_string(),
                crate::document::CutMode::OffsetFill => "Offset Fill Engrave".to_string(),
                crate::document::CutMode::Image => "Photo Engrave".to_string(),
            };
        }
        self.show_save_material_dialog = true;
    }

    /// Renders the Material Library & Database window
    pub fn show_material_library_window(&mut self, ctx: &egui::Context) {
        if !self.show_material_library {
            return;
        }

        let mut is_open = true;
        let mut op_to_apply: Option<crate::material::MaterialOperation> = None;
        let mut op_to_delete: Option<(String, String, f64, String)> = None;
        let mut test_grid_params: Option<(f64, f64, bool)> = None;
        let mut open_save_dialog = false;
        let mut do_reset_defaults = false;
        let mut new_selection: Option<(String, String, f64, String)> = None;
        let mut do_prompt_export = false;
        let mut do_prompt_import = false;

        let mut search = std::mem::take(&mut self.mat_lib_search);
        let mut cat_filter = std::mem::take(&mut self.mat_lib_category_filter);
        let selected_cat = self.mat_lib_selected_category.clone();
        let selected_mat = self.mat_lib_selected_material.clone();
        let selected_thick = self.mat_lib_selected_thickness;
        let selected_op = self.mat_lib_selected_op.clone();

        let active_layer_label = if let Some(layer) = self.doc.layers.get(&self.selected_layer) {
            layer.display_label()
        } else {
            format!("Layer L{:02}", self.selected_layer)
        };

        let color_blue = self.color_blue();
        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let (def_pos, def_size) = self.window_pos_size("material_library_dialog", [100.0, 60.0], [820.0, 560.0]);

        let resp = egui::Window::new(tr("Material Library & Database"))
            .id(egui::Id::new("material_library_dialog"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .min_width(680.0)
            .min_height(460.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                // Top Bar: Search, Category Filter, and Action Buttons
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    ui.add(
                        egui::TextEdit::singleline(&mut search)
                            .hint_text("Search materials, operations...")
                            .desired_width(170.0),
                    );
                    if !search.is_empty() {
                        if ui.small_button("✖").clicked() {
                            search.clear();
                        }
                    }

                    ui.separator();

                    let categories = self.material_lib.categories();
                    egui::ComboBox::from_id_salt("mat_lib_cat_filter_cb")
                        .selected_text(&cat_filter)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut cat_filter, "All Categories".to_string(), "All Categories");
                            for cat in &categories {
                                ui.selectable_value(&mut cat_filter, cat.clone(), cat);
                            }
                        });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("↺ Reset Defaults").on_hover_text("Restore factory default materials (Plywood, Tiles, Acrylic, Slate, etc.)").clicked() {
                            do_reset_defaults = true;
                        }
                        if ui.button("📤 Export JSON").on_hover_text("Export this material database to a .json file").clicked() {
                            do_prompt_export = true;
                        }
                        if ui.button("📥 Import (.json / .clb)").on_hover_text("Import materials from PhotonCAM JSON or LightBurn .clb library").clicked() {
                            do_prompt_import = true;
                        }
                        if ui.button("➕ Save Active Layer").on_hover_text("Save current selected layer's cut settings as a new preset").clicked() {
                            open_save_dialog = true;
                        }
                    });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                let search_term = search.trim().to_lowercase();

                // 2-Panel Layout: Left Tree List, Right Detail & Actions
                ui.horizontal_top(|ui| {
                    // LEFT COLUMN
                    ui.vertical(|ui| {
                        ui.set_width(320.0);
                        ui.heading("Materials");
                        ui.add_space(2.0);

                        let scroll_height = ui.available_height().max(420.0);
                        egui::ScrollArea::vertical()
                            .id_salt("mat_lib_tree_scroll")
                            .auto_shrink([false, false])
                            .min_scrolled_height(380.0)
                            .max_height(scroll_height)
                            .show(ui, |ui| {
                                if self.material_lib.materials.is_empty() {
                                    ui.label("No materials in database.");
                                    return;
                                }

                                if !search_term.is_empty() {
                                    // Flattened search result list
                                    let mut match_count = 0;
                                    for mat in &self.material_lib.materials {
                                        if cat_filter != "All Categories" && !mat.category.eq_ignore_ascii_case(&cat_filter) {
                                            continue;
                                        }
                                        for thick in &mat.thicknesses {
                                            for op in &thick.operations {
                                                let text = format!("{} {} {} {}", mat.name, mat.category, thick.label, op.name).to_lowercase();
                                                if text.contains(&search_term) {
                                                    match_count += 1;
                                                    let is_sel = selected_cat.as_deref() == Some(&mat.category)
                                                        && selected_mat.as_deref() == Some(&mat.name)
                                                        && selected_thick == Some(thick.thickness_mm)
                                                        && selected_op.as_deref() == Some(&op.name);

                                                    let icon = match op.cut_setting.mode {
                                                        crate::document::CutMode::Line => "[Line]",
                                                        crate::document::CutMode::Fill => "[Fill]",
                                                        crate::document::CutMode::OffsetFill => "[Offset]",
                                                        crate::document::CutMode::Image => "[Image]",
                                                    };

                                                    if ui.selectable_label(is_sel, format!("{} {}", icon, op.name)).clicked() {
                                                        new_selection = Some((mat.category.clone(), mat.name.clone(), thick.thickness_mm, op.name.clone()));
                                                    }
                                                    ui.label(egui::RichText::new(format!("  {} • {} • {}", mat.category, mat.name, thick.label)).small().weak());
                                                }
                                            }
                                        }
                                    }
                                    if match_count == 0 {
                                        ui.label(egui::RichText::new("No matching presets found.").weak());
                                    }
                                } else {
                                    // Categorized hierarchy tree
                                    let mut categories_map: std::collections::BTreeMap<String, Vec<&crate::material::MaterialEntry>> = std::collections::BTreeMap::new();
                                    for mat in &self.material_lib.materials {
                                        if cat_filter != "All Categories" && !mat.category.eq_ignore_ascii_case(&cat_filter) {
                                            continue;
                                        }
                                        categories_map.entry(mat.category.clone()).or_default().push(mat);
                                    }

                                    for (category, mats) in categories_map {
                                        let cat_label = format!("📁 {} ({} materiaalia)", category, mats.len());
                                        ui.collapsing(egui::RichText::new(cat_label).strong().color(color_blue), |ui| {
                                            for mat in mats {
                                                let mat_label = format!("📦 {}", mat.name);
                                                ui.collapsing(egui::RichText::new(mat_label).strong(), |ui| {
                                                    for thick in &mat.thicknesses {
                                                        ui.horizontal(|ui| {
                                                            ui.add_space(4.0);
                                                            ui.label(egui::RichText::new(&thick.label).small().weak());
                                                        });
                                                        for op in &thick.operations {
                                                            let is_sel = selected_cat.as_deref() == Some(&category)
                                                                && selected_mat.as_deref() == Some(&mat.name)
                                                                && selected_thick == Some(thick.thickness_mm)
                                                                && selected_op.as_deref() == Some(&op.name);

                                                            ui.horizontal(|ui| {
                                                                ui.add_space(12.0);
                                                                let icon = match op.cut_setting.mode {
                                                                    crate::document::CutMode::Line => "[Line]",
                                                                    crate::document::CutMode::Fill => "[Fill]",
                                                                    crate::document::CutMode::OffsetFill => "[Offset]",
                                                                    crate::document::CutMode::Image => "[Image]",
                                                                };
                                                                if ui.selectable_label(is_sel, format!("{} {}", icon, op.name)).clicked() {
                                                                    new_selection = Some((category.clone(), mat.name.clone(), thick.thickness_mm, op.name.clone()));
                                                                }
                                                            });
                                                        }
                                                    }
                                                });
                                            }
                                        });
                                    }
                                }
                            });
                    });

                    ui.separator();

                    // RIGHT COLUMN
                    ui.vertical(|ui| {
                        ui.set_min_width(360.0);
                        let selected_data = if let (Some(ref mat_name), Some(thick_mm), Some(ref op_name)) = (
                            &selected_mat,
                            selected_thick,
                            &selected_op,
                        ) {
                            let mat_opt = if let Some(ref cat) = selected_cat {
                                self.material_lib.find_material_in_category(cat, mat_name)
                            } else {
                                self.material_lib.find_material(mat_name)
                            };
                            if let Some(mat) = mat_opt {
                                if let Some(thick) = mat.find_thickness(thick_mm) {
                                    if let Some(op) = thick.find_operation(op_name) {
                                        Some((mat.clone(), thick.clone(), op.clone()))
                                    } else { None }
                                } else { None }
                            } else { None }
                        } else {
                            None
                        };

                        if let Some((mat, thick, op)) = selected_data {
                            ui.horizontal(|ui| {
                                ui.heading(&op.name);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("🗑 Delete Preset").on_hover_text("Delete this preset from library").clicked() {
                                        op_to_delete = Some((mat.category.clone(), mat.name.clone(), thick.thickness_mm, op.name.clone()));
                                    }
                                });
                            });
                            ui.label(egui::RichText::new(format!("{} • {} ({})", mat.name, thick.label, mat.category)).small().weak());

                            ui.add_space(6.0);
                            ui.separator();
                            ui.add_space(4.0);

                            // Cut Parameters Grid
                            egui::Grid::new("mat_lib_detail_grid")
                                .num_columns(2)
                                .spacing([12.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("Cut Mode:").strong());
                                    let mode_desc = match op.cut_setting.mode {
                                        crate::document::CutMode::Line => "Line (Vector Cut / Outline Score)",
                                        crate::document::CutMode::Fill => "Fill (Vector Raster Engrave)",
                                        crate::document::CutMode::OffsetFill => "Offset Fill (Concentric Contour Engrave)",
                                        crate::document::CutMode::Image => "Image (Dithered Photographic Burn)",
                                    };
                                    ui.label(mode_desc);
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Laser Speed:").strong());
                                    ui.label(format!("{:.1} mm/s  ({:.0} mm/min)", op.cut_setting.speed, op.cut_setting.speed * 60.0));
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Laser Power:").strong());
                                    if (op.cut_setting.power_max - op.cut_setting.power_min).abs() > 0.01 && op.cut_setting.mode == crate::document::CutMode::Image {
                                        ui.label(format!("{:.1}% - {:.1}% (Grayscale PWM)", op.cut_setting.power_min, op.cut_setting.power_max));
                                    } else {
                                        ui.label(format!("{:.1}%", op.cut_setting.power_max));
                                    }
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Passes:").strong());
                                    ui.label(format!("{}", op.cut_setting.num_passes));
                                    ui.end_row();

                                    ui.label(egui::RichText::new("Air Assist:").strong());
                                    if op.cut_setting.air_assist {
                                        ui.label(egui::RichText::new("Enabled (ON)").color(egui::Color32::from_rgb(100, 220, 100)));
                                    } else {
                                        ui.label(egui::RichText::new("Disabled (OFF)").weak());
                                    }
                                    ui.end_row();

                                    if op.cut_setting.kerf > 0.0 {
                                        ui.label(egui::RichText::new("Kerf Compensation:").strong());
                                        ui.label(format!("{:.3} mm (±{:.3} mm offset)", op.cut_setting.kerf, op.cut_setting.kerf * 0.5));
                                        ui.end_row();
                                    }

                                    if op.cut_setting.mode == crate::document::CutMode::Fill || op.cut_setting.mode == crate::document::CutMode::Image {
                                        ui.label(egui::RichText::new("Line Interval:").strong());
                                        let dpi = if op.cut_setting.line_interval > 0.001 { (25.4 / op.cut_setting.line_interval).round() as i32 } else { 318 };
                                        ui.label(format!("{:.3} mm (~{} DPI)", op.cut_setting.line_interval, dpi));
                                        ui.end_row();
                                    }

                                    if op.cut_setting.mode == crate::document::CutMode::Image {
                                        ui.label(egui::RichText::new("Dither Algorithm:").strong());
                                        ui.label(format!("{:?}", op.cut_setting.dither_mode));
                                        ui.end_row();
                                    }
                                });

                            if !op.notes.is_empty() {
                                ui.add_space(8.0);
                                ui.group(|ui| {
                                    ui.label(egui::RichText::new("Tips & Operator Notes:").strong());
                                    ui.label(egui::RichText::new(&op.notes).small());
                                });
                            }

                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(6.0);

                            // Action Buttons
                            let apply_btn = egui::Button::new(
                                egui::RichText::new(format!(" ⬇ Apply to Active Layer ({}) ", active_layer_label))
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            ).fill(egui::Color32::from_rgb(34, 139, 34));

                            if ui.add(apply_btn).on_hover_text("Copy these speed, power, pass, and kerf settings to your currently active layer").clicked() {
                                op_to_apply = Some(op.clone());
                            }

                            ui.add_space(6.0);

                            if ui.button("Generate Material Test Grid Around Settings")
                                .on_hover_text("Open Material Test Generator prepopulated with speed and power centered around this preset")
                                .clicked()
                            {
                                test_grid_params = Some((op.cut_setting.speed, op.cut_setting.power_max, op.cut_setting.mode != crate::document::CutMode::Line));
                            }
                        } else {
                            ui.vertical_centered(|ui| {
                                ui.add_space(80.0);
                                ui.heading("Select a Preset");
                                ui.label("Click on any material operation on the left to preview its cut settings, apply it to your layer, or calibrate with a test grid.");
                                ui.add_space(20.0);
                                let total_mats = self.material_lib.materials.len();
                                let total_ops: usize = self.material_lib.materials.iter().map(|m| m.thicknesses.iter().map(|t| t.operations.len()).sum::<usize>()).sum();
                                ui.label(egui::RichText::new(format!("Current database: {} materials, {} calibrated presets", total_mats, total_ops)).weak().small());
                            });
                        }
                    });
                });
            });

        if let Some(r) = resp {
            self.record_window_rect("material_library_dialog", r.response.rect);
        }
        self.mat_lib_search = search;
        self.mat_lib_category_filter = cat_filter;
        self.show_material_library = is_open;

        if let Some((cat, mat, thick, op)) = new_selection {
            self.mat_lib_selected_category = Some(cat);
            self.mat_lib_selected_material = Some(mat);
            self.mat_lib_selected_thickness = Some(thick);
            self.mat_lib_selected_op = Some(op);
        }

        if let Some(op) = op_to_apply {
            if let Some(layer) = self.doc.layers.get_mut(&self.selected_layer) {
                op.apply_to_layer(layer);
                self.doc_version = self.doc_version.wrapping_add(1);
                self.image_textures.clear();
                self.status_msg = format!("Applied preset \"{}\" to Layer L{:02}", op.name, self.selected_layer);
            }
        }

        if let Some((cat, mat, thick, op)) = op_to_delete {
            self.material_lib.delete_operation_in_category(&cat, &mat, thick, &op);
            let _ = self.material_lib.save();
            self.mat_lib_selected_op = None;
            self.status_msg = format!("Deleted preset \"{}\" from Material Library", op);
        }

        if let Some((spd, pwr, is_engrave)) = test_grid_params {
            self.mat_test_min_speed = (spd * 0.5).max(1.0).round();
            self.mat_test_max_speed = (spd * 1.5).round();
            self.mat_test_min_power = (pwr * 0.5).max(10.0).round();
            self.mat_test_max_power = (pwr * 1.25).min(100.0).round();
            self.mat_test_is_engrave = is_engrave;
            self.show_material_test_window = true;
            self.status_msg = "Configured Material Test Grid centered around preset settings".to_string();
        }

        if open_save_dialog {
            self.open_save_layer_dialog();
        }

        if do_reset_defaults {
            self.material_lib = crate::material::MaterialLibrary::default_library();
            let _ = self.material_lib.save();
            self.status_msg = "Material library reset to factory defaults".to_string();
        }

        if do_prompt_export {
            self.prompt_export_material_library(ctx);
        }

        if do_prompt_import {
            self.prompt_import_material_library(ctx);
        }
    }

    /// Renders the Save Layer to Material Library modal dialog
    pub fn show_save_material_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_save_material_dialog {
            return;
        }

        let mut is_open = true;
        let mut do_save = false;
        let mut cancel_clicked = false;

        let layer_info = if let Some(layer) = self.doc.layers.get(&self.selected_layer) {
            format!("Layer L{:02} ({:.1} mm/s, {:.0}% Power, {} Passes)", self.selected_layer, layer.cut_setting.speed, layer.cut_setting.power_max, layer.cut_setting.num_passes)
        } else {
            "No active layer".to_string()
        };

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let (def_pos, def_size) = self.window_pos_size("save_material_dialog", [150.0, 100.0], [380.0, 360.0]);

        let resp = egui::Window::new(tr("Save Layer to Material Library"))
            .id(egui::Id::new("save_material_dialog"))
            .open(&mut is_open)
            .collapsible(true)
            .resizable(false)
            .default_pos(def_pos)
            .default_size(def_size)
            .show(ctx, |ui| {
                ui.heading("Save Preset to Library");
                ui.label(egui::RichText::new("Saves the active layer's cut speed, power, passes, and air assist into your material database.").small().weak());
                ui.add_space(6.0);

                ui.label(egui::RichText::new(layer_info).strong().color(egui::Color32::from_rgb(100, 200, 255)));
                ui.add_space(6.0);

                egui::Grid::new("save_mat_grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Category:");
                        ui.add(egui::TextEdit::singleline(&mut self.save_mat_category).desired_width(180.0));
                        ui.end_row();

                        ui.label("Material Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.save_mat_name).desired_width(180.0));
                        ui.end_row();

                        ui.label("Thickness (mm):");
                        ui.add(egui::DragValue::new(&mut self.save_mat_thickness_mm).speed(0.1).range(0.0..=50.0).suffix(" mm"));
                        ui.end_row();

                        ui.label("Thickness Label:");
                        ui.add(egui::TextEdit::singleline(&mut self.save_mat_thickness_label).desired_width(180.0));
                        ui.end_row();

                        ui.label("Operation Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.save_mat_op_name).desired_width(180.0));
                        ui.end_row();

                        ui.label("Notes / Tips:");
                        ui.add(egui::TextEdit::multiline(&mut self.save_mat_notes).desired_rows(3).desired_width(180.0));
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    let save_btn = egui::Button::new(egui::RichText::new(" Save Preset ").strong().color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(34, 139, 34));
                    if ui.add(save_btn).clicked() {
                        do_save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        if let Some(r) = resp {
            self.record_window_rect("save_material_dialog", r.response.rect);
        }

        if cancel_clicked {
            is_open = false;
        }

        if do_save {
            if let Some(layer) = self.doc.layers.get(&self.selected_layer) {
                let op = crate::material::MaterialOperation::from_layer(&self.save_mat_op_name, layer, &self.save_mat_notes);
                self.material_lib.add_or_update_operation(
                    &self.save_mat_category,
                    &self.save_mat_name,
                    self.save_mat_thickness_mm,
                    &self.save_mat_thickness_label,
                    op,
                );
                let _ = self.material_lib.save();
                self.status_msg = format!("Saved preset \"{}\" to Material Library!", self.save_mat_op_name);
                self.mat_lib_selected_category = Some(self.save_mat_category.clone());
                self.mat_lib_selected_material = Some(self.save_mat_name.clone());
                self.mat_lib_selected_thickness = Some(self.save_mat_thickness_mm);
                self.mat_lib_selected_op = Some(self.save_mat_op_name.clone());
                is_open = false;
            }
        }

        self.show_save_material_dialog = is_open;
    }

    /// Aligns selected shapes relative to each other or centers the selection to the laser bed
    pub fn align_selection(&mut self, mode: AlignMode) {
        if self.selected_shape_indices.is_empty() {
            return;
        }

        if mode == AlignMode::CenterToBed {
            let mut combined = BoundingBox::empty();
            for &idx in &self.selected_shape_indices {
                if let Some(shape) = self.doc.shapes.get(idx) {
                    combined = combined.union(&shape.bounds());
                }
            }
            if !combined.is_empty() {
                let bed_cx = self.doc.width / 2.0;
                let bed_cy = self.doc.height / 2.0;
                let sel_cx = (combined.min_x + combined.max_x) / 2.0;
                let sel_cy = (combined.min_y + combined.max_y) / 2.0;
                let dx = bed_cx - sel_cx;
                let dy = bed_cy - sel_cy;

                self.push_undo();
                for &idx in &self.selected_shape_indices {
                    if let Some(shape) = self.doc.shapes.get_mut(idx) {
                        if !shape.is_locked() {
                            shape.translate(dx, dy);
                        }
                    }
                }
                self.doc_version = self.doc_version.wrapping_add(1);
                self.status_msg = self.tr("Centered selection to bed (P)").to_string();
            }
            return;
        }

        if self.selected_shape_indices.len() < 2 {
            self.status_msg = self.tr("Select at least 2 shapes to align").to_string();
            return;
        }

        let mut combined = BoundingBox::empty();
        for &idx in &self.selected_shape_indices {
            if let Some(shape) = self.doc.shapes.get(idx) {
                combined = combined.union(&shape.bounds());
            }
        }
        if combined.is_empty() {
            return;
        }

        self.push_undo();
        for &idx in &self.selected_shape_indices {
            if let Some(shape) = self.doc.shapes.get_mut(idx) {
                if shape.is_locked() {
                    continue;
                }
                let b = shape.bounds();
                let (dx, dy) = match mode {
                    AlignMode::Left => (combined.min_x - b.min_x, 0.0),
                    AlignMode::Right => (combined.max_x - b.max_x, 0.0),
                    AlignMode::Top => (0.0, combined.min_y - b.min_y),
                    AlignMode::Bottom => (0.0, combined.max_y - b.max_y),
                    AlignMode::CenterH => ((combined.min_x + combined.max_x) / 2.0 - (b.min_x + b.max_x) / 2.0, 0.0),
                    AlignMode::CenterV => (0.0, (combined.min_y + combined.max_y) / 2.0 - (b.min_y + b.max_y) / 2.0),
                    AlignMode::CenterToBed => (0.0, 0.0),
                };
                shape.translate(dx, dy);
            }
        }
        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = self.tr("Aligned shapes successfully").to_string();
    }

    /// Distributes selected shapes evenly horizontally or vertically based on their center coordinates
    pub fn distribute_selection(&mut self, mode: DistributeMode) {
        if self.selected_shape_indices.len() < 3 {
            self.status_msg = self.tr("Select at least 3 shapes to distribute").to_string();
            return;
        }

        let mut items: Vec<(usize, f64)> = self.selected_shape_indices
            .iter()
            .copied()
            .filter(|&idx| idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked())
            .map(|idx| {
                let b = self.doc.shapes[idx].bounds();
                let center = match mode {
                    DistributeMode::Horizontal => (b.min_x + b.max_x) / 2.0,
                    DistributeMode::Vertical => (b.min_y + b.max_y) / 2.0,
                };
                (idx, center)
            })
            .collect();

        if items.len() < 3 {
            self.status_msg = self.tr("Select at least 3 shapes to distribute").to_string();
            return;
        }

        items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let first_center = items.first().unwrap().1;
        let last_center = items.last().unwrap().1;
        let count = items.len();
        let step = (last_center - first_center) / (count - 1) as f64;

        self.push_undo();
        for (i, &(idx, current_center)) in items.iter().enumerate() {
            let target_center = first_center + (i as f64) * step;
            let delta = target_center - current_center;
            let (dx, dy) = match mode {
                DistributeMode::Horizontal => (delta, 0.0),
                DistributeMode::Vertical => (0.0, delta),
            };
            if let Some(shape) = self.doc.shapes.get_mut(idx) {
                shape.translate(dx, dy);
            }
        }
        self.doc_version = self.doc_version.wrapping_add(1);
        self.status_msg = self.tr("Distributed shapes successfully").to_string();
    }

    /// Renders the Visual Cut Preview Simulator Window
    pub fn show_preview_simulator_window(&mut self, ctx: &egui::Context) {
        if !self.show_preview_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);

        if self.preview_job.is_none() || self.preview_job_version != self.doc_version {
            self.preview_job = Some(PreviewJobData::build(&self.doc, self.device.settings.default_kerf));
            self.preview_job_version = self.doc_version;
        }

        let job = match self.preview_job.as_ref() {
            Some(j) => j.clone(),
            None => return,
        };

        if self.preview_playing && job.total_time_sec > 0.0 {
            let dt = ctx.input(|i| i.stable_dt as f64);
            self.preview_progress += (dt * self.preview_speed) / job.total_time_sec;
            if self.preview_progress >= 1.0 {
                self.preview_progress = 1.0;
                self.preview_playing = false;
            }
            ctx.request_repaint();
        }

        let mut is_open = true;
        let (def_pos, def_size) = self.window_pos_size("cut_preview_dialog", [120.0, 60.0], [720.0, 530.0]);
        let resp = egui::Window::new(tr("Laser Cut Preview & Simulation"))
            .id(egui::Id::new("cut_preview_dialog"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .resizable(true)
            .show(ctx, |ui| {
                // Top control bar: Play, Pause, Rewind, Speeds
                ui.horizontal(|ui| {
                    let play_label = if self.preview_playing { tr("Pause") } else { tr("Play") };
                    let play_btn = egui::Button::new(
                        egui::RichText::new(format!("▶ {}", play_label)).strong().color(egui::Color32::WHITE),
                    ).fill(if self.preview_playing { egui::Color32::from_rgb(200, 140, 20) } else { egui::Color32::from_rgb(34, 139, 34) });

                    if ui.add(play_btn).clicked() {
                        if self.preview_progress >= 1.0 {
                            self.preview_progress = 0.0;
                        }
                        self.preview_playing = !self.preview_playing;
                    }

                    if ui.button(tr("Rewind")).clicked() {
                        self.preview_progress = 0.0;
                        self.preview_playing = false;
                    }

                    if ui.button("⟳").on_hover_text("Recalculate Toolpath").clicked() {
                        self.preview_job = None;
                    }

                    ui.separator();

                    ui.label(tr("Playback Speed:"));
                    for &spd in &[0.5, 1.0, 2.0, 5.0, 10.0, 20.0] {
                        let text = format!("{:.1}x", spd);
                        if ui.selectable_label((self.preview_speed - spd).abs() < 1e-3, text).clicked() {
                            self.preview_speed = spd;
                        }
                    }
                });

                ui.add_space(4.0);

                // Scrub slider & time display
                ui.horizontal(|ui| {
                    let cur_time = self.preview_progress * job.total_time_sec;
                    let total_time = job.total_time_sec;
                    let time_str = format!(
                        "{:02}:{:02} / {:02}:{:02} ({:.1}%)",
                        (cur_time / 60.0) as u32,
                        (cur_time % 60.0) as u32,
                        (total_time / 60.0) as u32,
                        (total_time % 60.0) as u32,
                        self.preview_progress * 100.0,
                    );
                    ui.label(egui::RichText::new(time_str).strong().monospace());

                    let slider = egui::Slider::new(&mut self.preview_progress, 0.0..=1.0)
                        .show_value(false);
                    ui.add(slider);
                });

                ui.add_space(2.0);

                // Stats & Legend
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "✂ {}: {:.1} mm  |  ✈ {}: {:.1} mm  |  ⏱ {}: {:.1}s",
                            tr("Cut Travel"),
                            job.total_cut_distance,
                            tr("Rapid Travel"),
                            job.total_rapid_distance,
                            tr("Simulated Time:"),
                            job.total_time_sec,
                        ))
                        .small()
                        .weak(),
                    );
                });

                ui.separator();

                // Interactive Simulator Canvas Viewport
                let available = ui.available_size_before_wrap();
                let (response, painter) = ui.allocate_painter(available, egui::Sense::hover());
                let canvas_rect = response.rect;

                if canvas_rect.width() > 10.0 && canvas_rect.height() > 10.0 {
                    let bed_w = self.doc.width.max(10.0);
                    let bed_h = self.doc.height.max(10.0);
                    let margin = 24.0;
                    let scale = ((canvas_rect.width() - margin * 2.0) / bed_w as f32)
                        .min((canvas_rect.height() - margin * 2.0) / bed_h as f32)
                        .max(0.05);

                    let rendered_w = bed_w as f32 * scale;
                    let rendered_h = bed_h as f32 * scale;
                    let origin = egui::pos2(
                        canvas_rect.left() + (canvas_rect.width() - rendered_w) * 0.5,
                        canvas_rect.top() + (canvas_rect.height() - rendered_h) * 0.5,
                    );

                    let to_scr = |pt: Point2D| -> egui::Pos2 {
                        egui::pos2(origin.x + pt.x as f32 * scale, origin.y + pt.y as f32 * scale)
                    };

                    // Draw bed background and border
                    let bed_screen_rect = egui::Rect::from_min_size(origin, egui::vec2(rendered_w, rendered_h));
                    painter.rect_filled(bed_screen_rect, 4.0, egui::Color32::from_rgb(18, 22, 28));
                    painter.rect_stroke(bed_screen_rect, 4.0, egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(50, 70, 95)), egui::StrokeKind::Outside);

                    // Origin marker (0, 0)
                    let p0 = to_scr(Point2D::ZERO);
                    painter.circle_filled(p0, 3.5, egui::Color32::from_rgb(0, 190, 255));

                    let cur_sim_time = self.preview_progress * job.total_time_sec;
                    let mut laser_head = Point2D::ZERO;
                    let mut is_laser_active = false;

                    // Draw toolpath moves
                    for m in &job.moves {
                        let m_start = m.start_time;
                        let m_end = m.start_time + m.duration;

                        if cur_sim_time >= m_end {
                            laser_head = m.to;
                            let p1 = to_scr(m.from);
                            let p2 = to_scr(m.to);
                            if p1.distance_sq(p2) > 0.04 {
                                if m.is_cut {
                                    painter.line_segment([p1, p2], egui::Stroke::new(1.8_f32, m.color));
                                } else {
                                    painter.line_segment([p1, p2], egui::Stroke::new(0.8_f32, egui::Color32::from_rgba_unmultiplied(80, 140, 200, 70)));
                                }
                            }
                        } else if cur_sim_time >= m_start {
                            let frac = if m.duration > 1e-6 {
                                ((cur_sim_time - m_start) / m.duration).clamp(0.0, 1.0)
                            } else {
                                1.0
                            };
                            let cur_pt = Point2D::new(
                                m.from.x + (m.to.x - m.from.x) * frac,
                                m.from.y + (m.to.y - m.from.y) * frac,
                            );
                            laser_head = cur_pt;
                            is_laser_active = m.is_cut;

                            let p1 = to_scr(m.from);
                            let p_mid = to_scr(cur_pt);
                            let p2 = to_scr(m.to);

                            if m.is_cut {
                                if p1.distance_sq(p_mid) > 0.04 {
                                    painter.line_segment([p1, p_mid], egui::Stroke::new(2.0_f32, m.color));
                                }
                                if p_mid.distance_sq(p2) > 0.04 {
                                    let dim_color = egui::Color32::from_rgba_unmultiplied(m.color.r() / 3, m.color.g() / 3, m.color.b() / 3, 90);
                                    painter.line_segment([p_mid, p2], egui::Stroke::new(1.0_f32, dim_color));
                                }
                            } else {
                                if p1.distance_sq(p2) > 0.04 {
                                    painter.line_segment([p1, p2], egui::Stroke::new(0.8_f32, egui::Color32::from_rgba_unmultiplied(80, 140, 200, 50)));
                                }
                            }
                        } else {
                            if m.is_cut {
                                let p1 = to_scr(m.from);
                                let p2 = to_scr(m.to);
                                if p1.distance_sq(p2) > 0.04 {
                                    let dim_color = egui::Color32::from_rgba_unmultiplied(m.color.r() / 3, m.color.g() / 3, m.color.b() / 3, 70);
                                    painter.line_segment([p1, p2], egui::Stroke::new(1.0_f32, dim_color));
                                }
                            }
                        }
                    }

                    // Render animated Laser Head
                    let head_pos = to_scr(laser_head);
                    if is_laser_active {
                        painter.circle_filled(head_pos, 10.0, egui::Color32::from_rgba_unmultiplied(255, 60, 40, 70));
                        painter.circle_filled(head_pos, 5.0, egui::Color32::from_rgb(255, 80, 50));
                        painter.circle_filled(head_pos, 2.0, egui::Color32::WHITE);
                    } else {
                        painter.circle_filled(head_pos, 4.0, egui::Color32::from_rgb(0, 220, 255));
                        painter.circle_stroke(head_pos, 6.0, egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(0, 220, 255, 120)));
                    }

                    // Crosshairs on laser head
                    painter.line_segment(
                        [egui::pos2(head_pos.x - 7.0, head_pos.y), egui::pos2(head_pos.x + 7.0, head_pos.y)],
                        egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
                    );
                    painter.line_segment(
                        [egui::pos2(head_pos.x, head_pos.y - 7.0), egui::pos2(head_pos.x, head_pos.y + 7.0)],
                        egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
                    );

                    // Coordinate readout
                    let coord_text = format!("X: {:.1} mm  Y: {:.1} mm", laser_head.x, laser_head.y);
                    painter.text(
                        egui::pos2(origin.x + 8.0, origin.y + rendered_h - 18.0),
                        egui::Align2::LEFT_BOTTOM,
                        coord_text,
                        egui::FontId::monospace(12.0),
                        egui::Color32::from_rgb(180, 210, 240),
                    );
                }
            });

        if let Some(r) = resp {
            self.record_window_rect("cut_preview_dialog", r.response.rect);
        }
        if !is_open {
            self.show_preview_window = false;
            self.preview_playing = false;
        }
    }

    /// Renders the Smart 2D Auto-Nesting & Material Packing dialog window
    pub fn show_nesting_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_nesting_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);

        let mut is_open = true;
        let mut do_apply = false;
        let mut do_run = false;
        let (def_pos, def_size) = self.window_pos_size("auto_nesting_dialog", [140.0, 70.0], [440.0, 460.0]);

        let resp = egui::Window::new(tr("Auto-Nest Materials Optimizer"))
            .id(egui::Id::new("auto_nesting_dialog"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading(tr("Auto-Nest (Material Packing)"));
                ui.label(
                    egui::RichText::new("Packs shapes tightly onto the sheet to minimize scrap waste.")
                        .small()
                        .weak(),
                );
                ui.add_space(6.0);

                egui::Grid::new("nesting_params_grid")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(tr("Sheet / Bed Size:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.nesting_options.sheet_width).speed(1.0).range(20.0..=3000.0).suffix(" mm"));
                            ui.label("×");
                            ui.add(egui::DragValue::new(&mut self.nesting_options.sheet_height).speed(1.0).range(20.0..=3000.0).suffix(" mm"));
                            if ui.small_button(tr("Use Bed Size")).clicked() {
                                self.nesting_options.sheet_width = self.doc.width;
                                self.nesting_options.sheet_height = self.doc.height;
                            }
                        });
                        ui.end_row();

                        ui.label(tr("Part Spacing:"));
                        ui.add(egui::DragValue::new(&mut self.nesting_options.part_spacing).speed(0.1).range(0.1..=50.0).suffix(" mm"));
                        ui.end_row();

                        ui.label(tr("Margin:"));
                        ui.add(egui::DragValue::new(&mut self.nesting_options.sheet_margin).speed(0.1).range(0.0..=50.0).suffix(" mm"));
                        ui.end_row();

                        ui.label(tr("Allowed Rotations:"));
                        egui::ComboBox::from_id_salt("nesting_rotations_combo")
                            .selected_text(self.nesting_options.allowed_rotations.label())
                            .show_ui(ui, |ui| {
                                for &rot in crate::nesting::AllowedRotations::ALL {
                                    ui.selectable_value(&mut self.nesting_options.allowed_rotations, rot, rot.label());
                                }
                            });
                        ui.end_row();

                        ui.label(tr("Nest Target:"));
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.nesting_options.only_selected, true, tr("Selected Shapes Only"));
                            ui.radio_value(&mut self.nesting_options.only_selected, false, tr("All Shapes on Canvas"));
                        });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                let has_selection = !self.selected_shape_indices.is_empty();
                let can_run = if self.nesting_options.only_selected { has_selection } else { !self.doc.shapes.is_empty() };

                ui.horizontal(|ui| {
                    let run_btn = egui::Button::new(
                        egui::RichText::new(tr("Run Auto-Nesting")).strong().color(egui::Color32::WHITE),
                    ).fill(egui::Color32::from_rgb(0, 120, 200));

                    if ui.add_enabled(can_run, run_btn).clicked() {
                        do_run = true;
                    }

                    if !can_run && self.nesting_options.only_selected {
                        ui.label(egui::RichText::new(tr("Select shapes to nest or switch to All Shapes")).small().color(egui::Color32::from_rgb(255, 160, 50)));
                    }
                });

                if let Some(ref res) = self.nesting_result {
                    ui.add_space(8.0);
                    egui::Frame::group(ui.style()).fill(egui::Color32::from_rgba_unmultiplied(30, 40, 55, 120)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(tr("Placed:")).strong());
                            let count_text = format!("{}/{} ({:.0}%)", res.placed_count, res.total_parts, (res.placed_count as f64 / res.total_parts.max(1) as f64) * 100.0);
                            ui.label(egui::RichText::new(count_text).color(egui::Color32::from_rgb(100, 220, 100)).strong());

                            if res.unplaced_count > 0 {
                                ui.separator();
                                ui.label(egui::RichText::new(tr("Unplaced:")).strong());
                                ui.label(egui::RichText::new(format!("{}", res.unplaced_count)).color(egui::Color32::from_rgb(255, 100, 100)).strong());
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(tr("Material Utilization:")).strong());
                            ui.label(egui::RichText::new(format!("{:.1}%", res.material_utilization_pct)).color(egui::Color32::from_rgb(255, 200, 60)).strong());

                            ui.separator();
                            ui.label(egui::RichText::new(tr("Used Area:")).strong());
                            ui.label(format!("{:.1} × {:.1} mm", res.used_bounds.width(), res.used_bounds.height()));
                        });
                    });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let apply_btn = egui::Button::new(
                            egui::RichText::new(tr("Apply Nesting")).strong().color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(34, 139, 34));

                        if ui.add(apply_btn).clicked() {
                            do_apply = true;
                        }

                        if ui.button("Cancel").clicked() {
                            self.nesting_result = None;
                        }
                    });
                }
            });

        if let Some(r) = resp {
            self.record_window_rect("auto_nesting_dialog", r.response.rect);
        }
        if !is_open {
            self.show_nesting_window = false;
        }

        if do_run {
            let indices: Vec<usize> = if self.nesting_options.only_selected {
                self.selected_shape_indices.clone()
            } else {
                (0..self.doc.shapes.len()).collect()
            };

            let res = crate::nesting::run_nesting(&self.doc.shapes, &indices, &self.nesting_options);
            self.nesting_result = Some(res);
        }

        if do_apply {
            if let Some(res) = self.nesting_result.take() {
                self.push_undo();
                crate::nesting::apply_nesting_result(&mut self.doc, &res);
                self.doc_version = self.doc_version.wrapping_add(1);
                self.status_msg = format!(
                    "Auto-nested {}/{} parts with {:.1}% material efficiency!",
                    res.placed_count, res.total_parts, res.material_utilization_pct
                );
                self.show_nesting_window = false;
            }
        }
    }

    /// Renders the Engraving Depth Test Generator modal dialog
    pub fn show_depth_test_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_depth_test_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);

        let mut is_open = true;
        let mut do_gen = false;
        let mut do_close = false;
        let (def_pos, def_size) = self.window_pos_size("depth_test_dialog", [160.0, 80.0], [450.0, 480.0]);

        let resp = egui::Window::new(tr("Engraving Depth Test Generator"))
            .id(egui::Id::new("depth_test_dialog"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading(tr("Engraving Depth Test (Caliper Calibration)"));
                ui.label(
                    egui::RichText::new(tr("Generates test pockets with step-wise passes or power levels to measure depth with a digital caliper."))
                        .small()
                        .weak(),
                );
                ui.add_space(6.0);

                ui.group(|ui| {
                    ui.label(egui::RichText::new(tr("Test Type:")).strong());
                    ui.radio_value(&mut self.depth_test_type, DepthTestType::MultiPass, tr("1. Multi-Pass Depth Test (1 - 8 passes)"));
                    ui.label(egui::RichText::new(tr("   Measures how many passes are needed for target depth (e.g. 1.0 mm or 1.5 mm)")).small().weak());

                    ui.radio_value(&mut self.depth_test_type, DepthTestType::PowerSteps, tr("2. Power Steps (15% - 85%)"));
                    ui.label(egui::RichText::new(tr("   Measures engraving depth for varying laser power at constant speed")).small().weak());

                    ui.radio_value(&mut self.depth_test_type, DepthTestType::PassesVsSpeed, tr("3. Passes vs. Speed Matrix"));
                    ui.label(egui::RichText::new(tr("   2D grid of depth across varying passes and speeds")).small().weak());

                    ui.radio_value(&mut self.depth_test_type, DepthTestType::LineInterval, tr("4. Line Interval Density (0.05 - 0.15 mm)"));
                    ui.label(egui::RichText::new(tr("   Tests hatch resolution to find the smoothest pocket bottom without scan ridges")).small().weak());
                });

                ui.add_space(6.0);
                egui::Grid::new("depth_test_params_grid")
                    .num_columns(3)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        match self.depth_test_type {
                            DepthTestType::MultiPass | DepthTestType::PowerSteps => {
                                ui.label(tr("Number of Steps:"));
                                ui.add(egui::DragValue::new(&mut self.depth_test_steps).range(2..=12).suffix(tr(" pcs")));
                                ui.label(egui::RichText::new(tr("Number of pockets")).small().weak());
                                ui.end_row();
                            }
                            _ => {}
                        }

                        ui.label(tr("Pocket Size (W × H):"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.depth_test_box_w).speed(0.5).range(6.0..=40.0).suffix(" mm"));
                            ui.label("×");
                            ui.add(egui::DragValue::new(&mut self.depth_test_box_h).speed(0.5).range(6.0..=40.0).suffix(" mm"));
                        });
                        ui.label(egui::RichText::new(tr("Sized for caliper depth probe")).small().weak());
                        ui.end_row();

                        ui.label(tr("Engraving Speed:"));
                        ui.add(egui::DragValue::new(&mut self.depth_test_speed).speed(5.0).range(10.0..=500.0).suffix(" mm/s"));
                        ui.label(egui::RichText::new(format!("~{:.0} mm/min", self.depth_test_speed * 60.0)).small().weak());
                        ui.end_row();

                        if self.depth_test_type == DepthTestType::PowerSteps {
                            ui.label(tr("Power Range:"));
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut self.depth_test_min_power).speed(1.0).range(5.0..=100.0).suffix("%"));
                                ui.label("–");
                                ui.add(egui::DragValue::new(&mut self.depth_test_max_power).speed(1.0).range(5.0..=100.0).suffix("%"));
                            });
                            ui.label("");
                            ui.end_row();
                        } else if self.depth_test_type != DepthTestType::PassesVsSpeed {
                            ui.label(tr("Laser Power:"));
                            ui.add(egui::DragValue::new(&mut self.depth_test_power).speed(1.0).range(5.0..=100.0).suffix(" %"));
                            ui.label("");
                            ui.end_row();
                        }

                        if self.depth_test_type != DepthTestType::LineInterval {
                            ui.label(tr("Line Interval:"));
                            ui.add(egui::DragValue::new(&mut self.depth_test_interval).speed(0.005).range(0.02..=0.30).suffix(" mm"));
                            let dpi = if self.depth_test_interval > 0.001 { 25.4 / self.depth_test_interval } else { 0.0 };
                            ui.label(egui::RichText::new(format!("~{:.0} DPI", dpi)).small().weak());
                            ui.end_row();
                        }

                        ui.label(tr("Cross-Hatch:"));
                        ui.checkbox(&mut self.depth_test_cross_hatch, tr("Cross-Hatch (0° / 90°)"))
                            .on_hover_text(tr("Flattens the pocket bottom by removing grain ridges"));
                        ui.label(egui::RichText::new(tr("Smoother bottom")).small().weak());
                        ui.end_row();

                        ui.label(tr("Clean Outline:"));
                        ui.checkbox(&mut self.depth_test_cut_outline, tr("Perimeter Score"))
                            .on_hover_text(tr("Scores a crisp outline around each pocket for clean caliper placement"));
                        ui.label(egui::RichText::new(tr("Sharp edges")).small().weak());
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    let gen_btn = egui::Button::new(
                        egui::RichText::new(format!(" {} ", tr("Generate Depth Test Grid"))).strong().color(egui::Color32::WHITE),
                    ).fill(egui::Color32::from_rgb(34, 139, 34));
                    if ui.add(gen_btn).clicked() {
                        do_gen = true;
                        do_close = true;
                    }
                    if ui.button(tr("Close")).clicked() {
                        do_close = true;
                    }
                });
            });

        if let Some(r) = resp {
            self.record_window_rect("depth_test_dialog", r.response.rect);
        }
        if do_gen {
            self.generate_depth_test_grid();
        }
        if do_close || !is_open {
            self.show_depth_test_window = false;
        }
    }
}

#[cfg(feature = "gui")]
impl eframe::App for LaserBurnApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply Dark / Light theme visuals whenever changed
        if ctx.style().visuals.dark_mode != self.device.settings.dark_mode {
            ctx.set_visuals(if self.device.settings.dark_mode {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
        }

        // Poll incoming responses / status from controller (including real-time ? status)
        self.device.poll_serial();

        // Motion simulation for smooth moves in offline / demo mode
        let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
        self.device.update_motion_simulation(dt);

        // Step active laser job if running
        if self.device.is_job_running {
            self.device.step_job();
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
        }

        // Step framing test motion if active
        if self.device.is_framing_active {
            self.device.step_frame_test();
            ctx.request_repaint_after(std::time::Duration::from_millis(20));
        }

        // Continuous smooth repaint when camera video stream is active (~30-40 FPS)
        if self.camera_manager.is_streaming {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }

        // Advance One-Shot Bed Snapshot capture
        if let Some(ref snapshot_state) = self.oneshot_snapshot {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));

            let elapsed = snapshot_state.started_at.elapsed();
            let latest = self.camera_manager.get_latest_frame();
            let has_new_frame = latest.as_ref().map_or(false, |f| f.frame_number > snapshot_state.initial_frame_number + 2);

            // Wait ~350ms (or 2+ new frames) for camera auto-exposure to stabilize, max 1800ms
            if (has_new_frame && elapsed >= std::time::Duration::from_millis(350)) || elapsed >= std::time::Duration::from_millis(1800) {
                let was_streaming = snapshot_state.was_streaming;
                if let Some(frame) = self.camera_manager.take_snapshot() {
                    self.save_bed_snapshot_to_disk(&frame);
                    self.last_raw_frame = Some(frame);
                    self.last_applied_calibration = None; // Force texture refresh
                    self.status_msg = "Bed overlay updated from camera.".to_string();
                } else {
                    self.status_msg = "Could not capture camera frame.".to_string();
                }

                if !was_streaming {
                    self.camera_manager.stop_stream();
                }
                self.oneshot_snapshot = None;
            }
        }

        // Fetch and undistort incoming camera frame or re-process when calibration changes
        let latest_frame = self.camera_manager.get_latest_frame();
        if let Some(ref f) = latest_frame {
            self.last_raw_frame = Some(f.clone());
        }

        let is_new_frame = latest_frame.as_ref().map_or(false, |f| f.frame_number != self.last_camera_frame_number);
        let cal_changed = self.last_applied_calibration.as_ref() != Some(&self.camera_calibration);

        if is_new_frame || (cal_changed && self.last_raw_frame.is_some()) {
            if let Some(ref raw) = self.last_raw_frame {
                let undistorted = self.lens_undistorter.undistort_frame(raw, &self.camera_calibration);
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [undistorted.width as usize, undistorted.height as usize],
                    &undistorted.rgba,
                );
                self.camera_texture = Some(ctx.load_texture(
                    "camera_live_texture",
                    color_image,
                    egui::TextureOptions::LINEAR,
                ));
                if let Some(ref f) = latest_frame {
                    self.last_camera_frame_number = f.frame_number;
                }
                self.last_applied_calibration = Some(self.camera_calibration.clone());
            }
        }

        // Advance stop/abort sequence smoothly if active
        if self.device.abort_phase.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
        }

        // Advance mechanical homing & zero-point calibration sequence if active
        if self.device.homing_phase.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
        }

        // Continuously repaint while physical machine is moving or active!
        if self.device.is_moving() {
            ctx.request_repaint();
        } else if self.device.is_physically_connected() {
            // Keep polling serial port and updating live indicators (switches, position) at 20 Hz
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        // Drain any completed asynchronous native file dialog results
        while let Ok(action) = self.file_dialog_rx.try_recv() {
            match action {
                FileDialogAction::OpenProject(path) => {
                    if let Err(e) = self.open_file(&path) {
                        self.status_msg = format!("Open Error: {}", e);
                    }
                }
                FileDialogAction::ImportImage(path) => {
                    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                    if matches!(ext.as_str(), "dxf" | "pdf" | "svg" | "lbrn2" | "lbrn") {
                        if let Err(e) = self.import_vector_file(&path) {
                            self.status_msg = format!("Import Error: {}", e);
                        }
                    } else if matches!(ext.as_str(), "glb" | "gltf" | "stl") {
                        if let Err(e) = self.import_3d_mesh(&path) {
                            self.status_msg = format!("3D Import Error: {}", e);
                        }
                    } else if let Err(e) = self.import_image(&path) {
                        self.status_msg = format!("Import Error: {}", e);
                    }
                }
                FileDialogAction::SaveProject(path) => {
                    let save_path = if path.extension().is_none() {
                        path.with_extension("lbrn2")
                    } else {
                        path
                    };
                    if let Err(e) = Lbrn2File::save_to_file(&self.doc, &save_path) {
                        self.status_msg = format!("Save Error: {}", e);
                    } else {
                        self.doc.name = save_path.file_stem().and_then(|s| s.to_str()).unwrap_or("project").to_string();
                        self.status_msg = format!("Saved: {}", save_path.file_name().unwrap_or_default().to_string_lossy());
                    }
                }
                FileDialogAction::ExportGCode(path) => {
                    let save_path = if path.extension().is_none() {
                        path.with_extension("gcode")
                    } else {
                        path
                    };
                    let mut opt = CutOptimizer::new();
                    opt.default_kerf = self.device.settings.default_kerf;
                    let job = opt.optimize(&self.doc);
                    let gen = GCodeGenerator::new(GCodeConfig::default());
                    let gcode = gen.generate(&self.doc, &job);
                    if let Err(e) = std::fs::write(&save_path, gcode) {
                        self.status_msg = format!("Export Error: {}", e);
                    } else {
                        self.status_msg = format!("Exported G-Code to {}", save_path.file_name().unwrap_or_default().to_string_lossy());
                    }
                }
                FileDialogAction::ExportRuida(path) => {
                    let save_path = if path.extension().is_none() {
                        path.with_extension("rd")
                    } else {
                        path
                    };
                    let mut opt = CutOptimizer::new();
                    opt.default_kerf = self.device.settings.default_kerf;
                    let job = opt.optimize(&self.doc);
                    let gen = RuidaGenerator::new();
                    let bytes = gen.generate_rd(&self.doc, &job);
                    if let Err(e) = std::fs::write(&save_path, bytes) {
                        self.status_msg = format!("Export Error: {}", e);
                    } else {
                        self.status_msg = format!("Exported Ruida to {}", save_path.file_name().unwrap_or_default().to_string_lossy());
                    }
                }
                FileDialogAction::BrowseFont(path) => {
                    self.text_font_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Custom").to_string();
                    self.text_font_path = path.to_string_lossy().to_string();
                }
                FileDialogAction::ExportSettings(path) => {
                    let save_path = if path.extension().is_none() {
                        path.with_extension("json")
                    } else {
                        path
                    };
                    let profile = SettingsProfile {
                        version: 1,
                        device: self.device.settings.clone(),
                        camera: Some(self.camera_calibration.clone()),
                        layers: Some(self.doc.layers.clone()),
                    };
                    match serde_json::to_string_pretty(&profile) {
                        Ok(json) => {
                            if let Err(e) = std::fs::write(&save_path, json) {
                                self.status_msg = format!("Export Error: {}", e);
                            } else {
                                self.status_msg = format!("Settings profile exported to {}", save_path.file_name().unwrap_or_default().to_string_lossy());
                            }
                        }
                        Err(e) => {
                            self.status_msg = format!("Serialize Error: {}", e);
                        }
                    }
                }
                FileDialogAction::ImportSettings(path) => {
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            if let Ok(profile) = serde_json::from_str::<SettingsProfile>(&content) {
                                self.device.settings = profile.device;
                                let _ = self.device.settings.save();
                                self.doc.width = self.device.settings.bed_width;
                                self.doc.height = self.device.settings.bed_height;

                                if let Some(cam) = profile.camera {
                                    self.camera_calibration = cam;
                                    let _ = self.camera_calibration.save();
                                    if self.camera_calibration.homography_points.len() == 4 {
                                        for i in 0..4 {
                                            let pt = &self.camera_calibration.homography_points[i];
                                            self.wizard_points[i] = Some((pt.camera_u, pt.camera_v));
                                        }
                                    }
                                }
                                if let Some(layers) = profile.layers {
                                    self.doc.layers = layers;
                                    let _ = Document::save_default_layers(&self.doc.layers);
                                }
                                self.status_msg = format!("Settings profile imported from {}", path.file_name().unwrap_or_default().to_string_lossy());
                            } else if let Ok(dev) = serde_json::from_str::<crate::device::DeviceSettings>(&content) {
                                self.device.settings = dev;
                                let _ = self.device.settings.save();
                                self.doc.width = self.device.settings.bed_width;
                                self.doc.height = self.device.settings.bed_height;
                                self.status_msg = format!("Device settings imported from {}", path.file_name().unwrap_or_default().to_string_lossy());
                            } else if let Ok(layers) = serde_json::from_str::<std::collections::BTreeMap<usize, crate::document::Layer>>(&content) {
                                self.doc.layers = layers;
                                let _ = Document::save_default_layers(&self.doc.layers);
                                self.status_msg = format!("Material palette imported from {}", path.file_name().unwrap_or_default().to_string_lossy());
                            } else {
                                self.status_msg = "Error: Unrecognized settings file format".to_string();
                            }
                        }
                        Err(e) => {
                            self.status_msg = format!("Import Error: {}", e);
                        }
                    }
                }
                FileDialogAction::ExportMaterialLibrary(path) => {
                    match self.material_lib.save_to_path(&path) {
                        Ok(_) => {
                            self.status_msg = format!("Material library exported to {}", path.file_name().unwrap_or_default().to_string_lossy());
                        }
                        Err(e) => {
                            self.status_msg = format!("Export Error: {}", e);
                        }
                    }
                }
                FileDialogAction::ImportMaterialLibrary(path) => {
                    match crate::material::MaterialLibrary::load_from_any_path(&path) {
                        Ok(imported) => {
                            let count = imported.materials.len();
                            self.material_lib.merge(imported);
                            let _ = self.material_lib.save();
                            self.status_msg = format!("Imported & merged {} materials from {}", count, path.file_name().unwrap_or_default().to_string_lossy());
                        }
                        Err(e) => {
                            self.status_msg = format!("Import Error: {}", e);
                        }
                    }
                }
            }
        }

        // Drag-and-drop file loading support (.lbrn2, .lbrn, .svg, .dxf, .pdf, images)
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for df in dropped {
            if let Some(ref path) = df.path {
                if let Err(e) = self.open_file(path) {
                    self.status_msg = format!("Open Error: {}", e);
                }
            }
        }

        // -------------------------------------------------------------
        // KEYBOARD SHORTCUTS & ARROW KEYS HANDLING
        // -------------------------------------------------------------
        if !ctx.wants_keyboard_input() {
            // Cmd+Z (Undo) and Cmd+Shift+Z / Ctrl+Y (Redo)
            if ctx.input(|i| (i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Z)) || (i.modifiers.command && i.key_pressed(egui::Key::Y))) {
                self.redo();
            } else if ctx.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z)) {
                self.undo();
            }

            // Cmd+O (Open Project)
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
                self.prompt_open_project(ctx);
            }

            // Cmd+I (Import Image)
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::I)) {
                self.prompt_import_image(ctx);
            }

            // Cmd+S (Save Project)
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
                self.prompt_save_project(ctx);
            }

            // Cmd+A (Select All)
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) {
                self.selected_shape_indices = (0..self.doc.shapes.len()).collect();
                self.status_msg = format!("Selected all {} shape(s)", self.doc.shapes.len());
            }

            // Cmd+C (Copy)
            if ctx.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::C)) || i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
                self.copy_selected();
            }

            // Cmd+V (Paste)
            if ctx.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::V)) || i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))) {
                self.paste();
            }

            // Cmd+X (Cut)
            if ctx.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::X)) || i.events.iter().any(|e| matches!(e, egui::Event::Cut))) {
                self.cut_selected();
            }

            // Cmd+D (Duplicate)
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::D)) {
                self.duplicate_selected();
            }

            // Delete / Backspace key to delete
            if ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
                if self.active_tool == ActiveTool::NodeEdit && self.selected_node_index.is_some() {
                    self.delete_selected_node();
                } else {
                    self.delete_selected();
                }
            }

            // N (Toggle Node Edit Tool)
            if !ctx.input(|i| i.modifiers.command) && ctx.input(|i| i.key_pressed(egui::Key::N)) {
                if self.active_tool == ActiveTool::NodeEdit {
                    self.active_tool = ActiveTool::Select;
                    self.selected_node_index = None;
                } else {
                    self.active_tool = ActiveTool::NodeEdit;
                    self.convert_selected_to_path();
                }
            }

            // C in NodeEdit mode (Toggle closed/open path)
            if self.active_tool == ActiveTool::NodeEdit && !ctx.input(|i| i.modifiers.command) && ctx.input(|i| i.key_pressed(egui::Key::C)) {
                self.toggle_selected_path_closed();
            }

            // Cmd+T (Open Text Tool)
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::T)) {
                if self.text_pos_x == 50.0 && self.text_pos_y == 50.0 {
                    self.text_pos_x = self.doc.width * 0.25;
                    self.text_pos_y = self.doc.height * 0.35;
                }
                self.show_text_dialog = true;
            }

            // Cmd+M (Toggle Move & Laser Control Window)
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::M)) {
                self.show_move_window = !self.show_move_window;
            }

            // Cmd+Comma (Toggle Settings Window)
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma)) {
                self.show_settings_window = !self.show_settings_window;
            }

            if self.device.settings.enable_camera_support {
                // Cmd+K (Toggle Camera Window)
                if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
                    self.show_camera_window = !self.show_camera_window;
                }

                // Cmd+Shift+C (Toggle Camera Background on Grid Canvas)
                if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::C)) {
                    self.camera_calibration.show_on_canvas = !self.camera_calibration.show_on_canvas;
                    let _ = self.camera_calibration.save();
                    self.status_msg = if self.camera_calibration.show_on_canvas {
                        "Camera background enabled on bed canvas".to_string()
                    } else {
                        "Camera background hidden".to_string()
                    };
                }

                // Cmd+U or Cmd+Shift+U (1-Click Update Bed Overlay Snapshot from Camera)
                if ctx.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::U)) || (i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::U))) {
                    self.trigger_update_overlay();
                }
            }

            // Alt+P (Toggle Visual Cut Preview Simulator Window)
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::P)) {
                self.show_preview_window = !self.show_preview_window;
            }

            // Keyboard Arrow Handling: Direct Laser Jogging vs Shape Nudging
            let wants_keyboard = ctx.wants_keyboard_input();
            if !wants_keyboard {
                let cmd_down = ctx.input(|i| i.modifiers.command);
                let shift_down = ctx.input(|i| i.modifiers.shift);
                let alt_down = ctx.input(|i| i.modifiers.alt);

                let mut arrow_x: f64 = 0.0;
                let mut arrow_y: f64 = 0.0;

                if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                    arrow_x -= 1.0;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                    arrow_x += 1.0;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    arrow_y += 1.0; // Y+ on laser bed
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    arrow_y -= 1.0; // Y- on laser bed
                }

                if arrow_x.abs() > 0.0 || arrow_y.abs() > 0.0 {
                    // Holding Cmd OR keyboard_jog_laser is false AND shapes are selected: nudge vector shapes!
                    let nudge_shapes = (cmd_down || !self.keyboard_jog_laser) && !self.selected_shape_indices.is_empty();

                    if nudge_shapes {
                        self.push_undo();
                        let step: f64 = if shift_down { 10.0 } else if alt_down { 0.1 } else { 1.0 };
                        let dx = arrow_x * step;
                        let dy = -arrow_y * step; // visual Y inversion for canvas
                        for &idx in &self.selected_shape_indices {
                            if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                self.doc.shapes[idx].translate(dx, dy);
                            }
                        }
                        self.status_msg = format!("Nudged selection by X: {:.1}, Y: {:.1} mm (Cmd+Arrow)", dx, dy);
                    } else if self.keyboard_jog_laser {
                        // Direct Laser Head Jogging via keyboard arrows!
                        let step = if shift_down {
                            (self.jog_step * 5.0).max(10.0) // Shift = fast coarse move
                        } else if alt_down {
                            (self.jog_step * 0.1).max(0.1)  // Alt/Option = fine move
                        } else {
                            self.jog_step
                        };
                        self.device.jog_arrow(arrow_x, arrow_y, step, self.jog_speed);
                        self.status_msg = format!(
                            "Laser Jog (Keyboard): X: {:.2}, Y: {:.2} mm (Step: {:.1}mm)",
                            self.device.work_x, self.device.work_y, step
                        );
                        ctx.request_repaint();
                    }
                }
            }

            if !self.selected_shape_indices.is_empty() {
                // Cmd+G (Group), Cmd+U (Ungroup), Cmd+L (Lock/Unlock)
                if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::G)) {
                    self.group_selected();
                }
                if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::U)) {
                    self.ungroup_selected();
                }
                if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::L)) {
                    self.toggle_lock_selected();
                }
                // Rotate selection:
                // Counter-clockwise (CCW): Q, Comma (,), OpenBracket ([), or Option+8
                // Clockwise (CW): E, Period (.), CloseBracket (]), or Option+9
                // Shift = 5°, Cmd = 90°, plain = 1°
                let shift_down = ctx.input(|i| i.modifiers.shift);
                let cmd_down = ctx.input(|i| i.modifiers.command);
                let alt_down = ctx.input(|i| i.modifiers.alt);

                let rot_ccw = (!cmd_down && ctx.input(|i| i.key_pressed(egui::Key::Q)))
                    || (!cmd_down && ctx.input(|i| i.key_pressed(egui::Key::Comma)))
                    || ctx.input(|i| i.key_pressed(egui::Key::OpenBracket))
                    || (alt_down && ctx.input(|i| i.key_pressed(egui::Key::Num8)));

                let rot_cw = (!cmd_down && ctx.input(|i| i.key_pressed(egui::Key::E)))
                    || (!cmd_down && ctx.input(|i| i.key_pressed(egui::Key::Period)))
                    || ctx.input(|i| i.key_pressed(egui::Key::CloseBracket))
                    || (alt_down && ctx.input(|i| i.key_pressed(egui::Key::Num9)));

                if rot_ccw {
                    let angle = if shift_down { -5.0 } else if cmd_down { -90.0 } else { -1.0 };
                    self.rotate_selected(angle);
                }
                if rot_cw {
                    let angle = if shift_down { 5.0 } else if cmd_down { 90.0 } else { 1.0 };
                    self.rotate_selected(angle);
                }

                // P (Center to Bed)
                if !cmd_down && !alt_down && ctx.input(|i| i.key_pressed(egui::Key::P)) {
                    self.align_selection(AlignMode::CenterToBed);
                }
            }
        }

        // -------------------------------------------------------------
        // TOP MENU & TOOLBAR
        // -------------------------------------------------------------
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            let cmd = Self::cmd_key();
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            egui::menu::bar(ui, |ui| {
                ui.menu_button(tr("File"), |ui| {
                    if ui.button(tr("New Project")).clicked() {
                        self.push_undo();
                        self.doc = Document::new("Untitled", self.device.settings.bed_width, self.device.settings.bed_height);
                        self.selected_shape_indices.clear();
                        self.selected_layer = 0;
                        ui.close_menu();
                    }
                    if ui.button(format!("{} ({}+O)", self.tr("Open File / Project..."), cmd)).clicked() {
                        self.prompt_open_project(ctx);
                        ui.close_menu();
                    }
                    if ui.button(format!("{} ({}+I)", self.tr("Import / Merge File (DXF, PDF, SVG, Image)..."), cmd)).clicked() {
                        self.prompt_import_image(ctx);
                        ui.close_menu();
                    }
                    if ui.button(format!("{} ({}+S)", self.tr("Save Project (.lbrn2)..."), cmd)).clicked() {
                        self.prompt_save_project(ctx);
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.menu_button(self.tr("Sample Files"), |ui| {
                        if ui.button(self.tr("AutoCAD DXF (Mechanical Bracket)")).clicked() {
                            self.load_sample_dxf();
                            ui.close_menu();
                        }
                        if ui.button(self.tr("Vector PDF (Laser Test Card)")).clicked() {
                            self.load_sample_pdf();
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    if ui.button(self.tr("Export G-Code (GRBL)...")).clicked() {
                        self.prompt_export_gcode(ctx);
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Export Ruida (.rd)...")).clicked() {
                        self.prompt_export_ruida(ctx);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(self.tr("Export Settings Profile...")).clicked() {
                        self.prompt_export_settings(ctx);
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Import Settings Profile...")).clicked() {
                        self.prompt_import_settings(ctx);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(self.tr("Connection Settings...")).clicked() {
                        self.show_connection_window = true;
                        ui.close_menu();
                    }
                    if ui.button(format!("{} ({}+,)", self.tr("Settings..."), cmd)).clicked() {
                        self.show_settings_window = true;
                        ui.close_menu();
                    }
                });

                ui.menu_button(self.tr("Edit"), |ui| {
                    let has_selection = !self.selected_shape_indices.is_empty();
                    let has_clipboard = !self.clipboard_shapes.is_empty();
                    let can_undo = !self.undo_stack.is_empty();
                    let can_redo = !self.redo_stack.is_empty();

                    if ui.add_enabled(can_undo, egui::Button::new(format!("{} ({}+Z)", self.tr("Undo"), cmd))).clicked() {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui.add_enabled(can_redo, egui::Button::new(format!("{} ({}+Shift+Z)", self.tr("Redo"), cmd))).clicked() {
                        self.redo();
                        ui.close_menu();
                    }
                    ui.separator();

                    if ui.add_enabled(has_selection, egui::Button::new(format!("{} ({}+X)", self.tr("Cut"), cmd))).clicked() {
                        self.cut_selected();
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_selection, egui::Button::new(format!("{} ({}+C)", self.tr("Copy"), cmd))).clicked() {
                        self.copy_selected();
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_clipboard, egui::Button::new(format!("{} ({}+V)", self.tr("Paste"), cmd))).clicked() {
                        self.paste();
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_selection, egui::Button::new(format!("{} ({}+D)", self.tr("Duplicate"), cmd))).clicked() {
                        self.duplicate_selected();
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_selection, egui::Button::new(format!("{} (Del)", self.tr("Delete")))).clicked() {
                        self.delete_selected();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.add_enabled(has_selection, egui::Button::new(self.tr("Offset Shapes (Contour)...")))
                        .on_hover_text("Create outward or inward offset contours around selected shapes")
                        .clicked()
                    {
                        self.show_offset_window = true;
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(format!("{} ({}+G)", self.tr("Group"), cmd))).clicked() {
                        self.group_selected();
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_selection, egui::Button::new(format!("{} ({}+U)", self.tr("Ungroup"), cmd))).clicked() {
                        self.ungroup_selected();
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_selection, egui::Button::new(format!("{} ({}+L)", self.tr("Toggle Lock"), cmd))).clicked() {
                        self.toggle_lock_selected();
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_selection, egui::Button::new(self.tr("Boolean Union / Weld"))).clicked() {
                        self.boolean_selected(crate::geometry::BooleanOp::Union);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(self.tr("Boolean Subtract (A - B)"))).clicked() {
                        self.boolean_selected(crate::geometry::BooleanOp::Subtract);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(self.tr("Boolean Intersect"))).clicked() {
                        self.boolean_selected(crate::geometry::BooleanOp::Intersect);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.add_enabled(has_selection, egui::Button::new(self.tr("Center to Bed (P)"))).clicked() {
                        self.align_selection(AlignMode::CenterToBed);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(self.tr("Align Left"))).clicked() {
                        self.align_selection(AlignMode::Left);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(self.tr("Align Center Horizontal"))).clicked() {
                        self.align_selection(AlignMode::CenterH);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(self.tr("Align Right"))).clicked() {
                        self.align_selection(AlignMode::Right);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(self.tr("Align Top"))).clicked() {
                        self.align_selection(AlignMode::Top);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(self.tr("Align Center Vertical"))).clicked() {
                        self.align_selection(AlignMode::CenterV);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 2, egui::Button::new(self.tr("Align Bottom"))).clicked() {
                        self.align_selection(AlignMode::Bottom);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 3, egui::Button::new(self.tr("Distribute Horizontally"))).clicked() {
                        self.distribute_selection(DistributeMode::Horizontal);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.selected_shape_indices.len() >= 3, egui::Button::new(self.tr("Distribute Vertically"))).clicked() {
                        self.distribute_selection(DistributeMode::Vertical);
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.doc.shapes.is_empty(), egui::Button::new(self.tr("Auto-Nest Shapes (Material Packing)...")))
                        .on_hover_text("Pack parts tightly onto the sheet to minimize scrap waste")
                        .clicked()
                    {
                        self.show_nesting_window = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.add_enabled(has_selection, egui::Button::new(self.tr("Grid Array (Matrix)...")))
                        .on_hover_text("Duplicate selected shape(s) in a rectangular grid of rows and columns")
                        .clicked()
                    {
                        self.show_grid_array_window = true;
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_selection, egui::Button::new(self.tr("Radial Array (Circular)...")))
                        .on_hover_text("Duplicate selected shape(s) in a circular pattern around a center point")
                        .clicked()
                    {
                        self.show_radial_array_window = true;
                        ui.close_menu();
                    }
                    let is_image_sel = self.selected_shape_indices.iter().any(|&i| i < self.doc.shapes.len() && matches!(self.doc.shapes[i], Shape::Image { .. }));
                    if ui.add_enabled(is_image_sel, egui::Button::new(self.tr("Trace Image (Bitmap to Vector)...")))
                        .on_hover_text("Vectorize raster image into closed cutting / engraving paths")
                        .clicked()
                    {
                        self.open_trace_dialog();
                        ui.close_menu();
                    }
                    if ui.button(format!("{} ({}+T)", self.tr("Add Text..."), cmd)).clicked() {
                        self.show_text_dialog = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(format!("{} ({}+A)", self.tr("Select All"), cmd)).clicked() {
                        self.selected_shape_indices = (0..self.doc.shapes.len()).collect();
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Clear Canvas")).clicked() {
                        self.push_undo();
                        self.doc.shapes.clear();
                        self.selected_shape_indices.clear();
                        ui.close_menu();
                    }
                });

                ui.menu_button(self.tr("View"), |ui| {
                    let tools_lbl = self.tr("Tool Palette (V2)");
                    ui.checkbox(&mut self.show_tools_palette, tools_lbl);
                    ui.separator();
                    let prop_lbl = self.tr("Properties & Layers");
                    ui.checkbox(&mut self.show_properties_window, prop_lbl);
                    let obj_lbl = self.tr("Objects List");
                    ui.checkbox(&mut self.show_objects_window, obj_lbl);
                    let job_lbl = self.tr("Job Control");
                    ui.checkbox(&mut self.show_job_window, job_lbl);
                    let move_lbl = self.tr("Laser Control & Jogging");
                    ui.checkbox(&mut self.show_move_window, move_lbl);
                    let traffic_lbl = self.tr("Data Traffic (TX / RX)");
                    ui.checkbox(&mut self.show_traffic_panel, traffic_lbl);
                    ui.separator();
                    if ui.button(self.tr("Fit to View")).clicked() {
                        self.zoom = 1.8;
                        self.pan = egui::Vec2::new(100.0, 80.0);
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Zoom In")).clicked() {
                        self.zoom = (self.zoom * 1.25).clamp(0.2, 20.0);
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Zoom Out")).clicked() {
                        self.zoom = (self.zoom * 0.8).clamp(0.2, 20.0);
                        ui.close_menu();
                    }
                    ui.separator();
                    let cam_bg_lbl = self.tr("Camera Background on Canvas");
                    if ui.checkbox(&mut self.camera_calibration.show_on_canvas, cam_bg_lbl).changed() {
                        let _ = self.camera_calibration.save();
                    }
                    let cam_grid_lbl = self.tr("Camera Alignment Grid");
                    if ui.checkbox(&mut self.camera_calibration.show_guide_grid, cam_grid_lbl).changed() {
                        let _ = self.camera_calibration.save();
                    }
                });

                ui.menu_button(self.tr("Laser"), |ui| {
                    if ui.button(self.tr("Material Library (Presets)...")).clicked() {
                        self.show_material_library = true;
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Material Test Generator (Power vs. Speed)...")).clicked() {
                        self.show_material_test_window = true;
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Engraving Depth Test Generator (Caliper)...")).clicked() {
                        self.show_depth_test_window = true;
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Preview Job Simulation (Alt+P)...")).clicked() {
                        self.show_preview_window = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(format!("{} ({}+M)", self.tr("Laser Control & Jogging"), cmd)).clicked() {
                        self.show_move_window = true;
                        ui.close_menu();
                    }
                    let is_on = self.device.is_weak_light_on;
                    let weak_label = if is_on { self.tr("Turn Off Weak Beam") } else { self.tr("Turn On Weak Beam") };
                    if ui.button(weak_label).clicked() {
                        self.device.toggle_weak_light();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(self.tr("Test Box Frame")).clicked() {
                        self.start_box_frame();
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Test Cut Contour (Shapes)")).clicked() {
                        self.start_contour_frame();
                        ui.close_menu();
                    }
                    ui.separator();
                    if self.device.settings.enable_limit_switches {
                        if ui.button(self.tr("Reset Origin to Limit Switches")).clicked() {
                            self.device.start_mechanical_home_reset();
                            ui.close_menu();
                        }
                        if ui.button(self.tr("Home Machine ($H)")).clicked() {
                            self.device.home();
                            ui.close_menu();
                        }
                    }
                    if ui.button(self.tr("Set Origin Here (G92)")).clicked() {
                        self.device.set_origin_here();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(self.tr("Web Remote & iPad Bridge...")).clicked() {
                        self.show_web_remote_window = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    let traffic_lbl = if self.show_traffic_panel { self.tr("Hide Data Traffic Log Panel") } else { self.tr("Show Data Traffic Log Panel") };
                    if ui.button(traffic_lbl).clicked() {
                        self.show_traffic_panel = !self.show_traffic_panel;
                        ui.close_menu();
                    }
                });

                ui.menu_button(self.tr("Generators"), |ui| {
                    if ui.button(self.tr("Finger Joint Box Maker...")).clicked() {
                        self.show_box_maker_window = true;
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Living Hinge...")).clicked() {
                        self.show_living_hinge_window = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(self.tr("Vector QR Code...")).clicked() {
                        self.show_qr_code_window = true;
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Batch Serial Numbers & Badges...")).clicked() {
                        self.show_variable_text_window = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(self.tr("Auto-Nest Shapes...")).clicked() {
                        self.show_nesting_window = true;
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Material Test Generator (Power vs. Speed)...")).clicked() {
                        self.show_material_test_window = true;
                        ui.close_menu();
                    }
                    if ui.button(self.tr("Engraving Depth Test Generator (Caliper)...")).clicked() {
                        self.show_depth_test_window = true;
                        ui.close_menu();
                    }
                });

                if self.device.settings.enable_camera_support {
                    ui.menu_button(self.tr("Camera"), |ui| {
                        if ui.button(format!("{} ({}+K)", self.tr("Camera Feed..."), cmd)).clicked() {
                            self.show_camera_window = true;
                            ui.close_menu();
                        }
                        if ui.button(self.tr("Scan Devices")).clicked() {
                            self.camera_manager.refresh_devices();
                            ui.close_menu();
                        }
                        ui.separator();
                        if !self.camera_manager.is_streaming {
                            if ui.button(self.tr("Start Stream")).clicked() {
                                self.camera_manager.start_stream();
                                ui.close_menu();
                            }
                        } else {
                            if ui.button(self.tr("Stop Stream")).clicked() {
                                self.camera_manager.stop_stream();
                                ui.close_menu();
                            }
                        }
                        if ui.button(self.tr("Update Bed Overlay (Cmd+U)")).clicked() {
                            self.trigger_update_overlay();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(self.tr("Auto-Center Selection on Camera Tile")).clicked() {
                            self.auto_fit_to_camera_tile(false, 2.0);
                            ui.close_menu();
                        }
                        if ui.button(self.tr("Auto-Fit & Scale Selection to Camera Tile (2mm Margin)")).clicked() {
                            self.auto_fit_to_camera_tile(true, 2.0);
                            ui.close_menu();
                        }
                    });
                }

                ui.separator();
                let theme_btn_text = if self.device.settings.dark_mode { self.tr("Theme: Dark") } else { self.tr("Theme: Light") };
                if ui.button(egui::RichText::new(theme_btn_text).strong())
                    .on_hover_text("Toggle between Dark Mode and Light Mode")
                    .clicked()
                {
                    self.device.settings.dark_mode = !self.device.settings.dark_mode;
                    let _ = self.device.settings.save();
                }

                ui.separator();
                ui.menu_button(egui::RichText::new(self.device.settings.language.short_label()).strong(), |ui| {
                    for &lang in crate::i18n::Language::ALL {
                        if ui.selectable_label(self.device.settings.language == lang, lang.label()).clicked() {
                            self.device.settings.language = lang;
                            let _ = self.device.settings.save();
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                let unit_label = match self.length_unit {
                    LengthUnit::Millimeters => "mm",
                    LengthUnit::Inches => "in",
                };
                ui.menu_button(egui::RichText::new(unit_label).strong(), |ui| {
                    for &u in LengthUnit::ALL {
                        if ui.selectable_label(self.length_unit == u, u.label()).clicked() {
                            self.length_unit = u;
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                let web_status_color = if self.web_remote_active { egui::Color32::from_rgb(0, 230, 118) } else { egui::Color32::from_rgb(0, 229, 255) };
                if ui.button(egui::RichText::new("Web Remote").strong().color(web_status_color)).on_hover_text("Open Web Remote & iPad Bridge control").clicked() {
                    self.show_web_remote_window = true;
                }
                ui.separator();
                ui.label(egui::RichText::new(concat!("PhotonCAM v", env!("CARGO_PKG_VERSION"))).strong().color(egui::Color32::from_rgb(255, 140, 0)));
                ui.label(egui::RichText::new(format!("({} | {}+{}: {})", tr("Arrows: Jog Laser"), cmd, tr("Arrows"), tr("Nudge Selection"))).italics().color(egui::Color32::GRAY));
            });

        });

        // -------------------------------------------------------------
        // BOTTOM STATUS BAR & QUICK LAYER PALETTE
        // -------------------------------------------------------------
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);

            // Quick Color & Tool Palette (LightBurn style: L00..L07, T01, T02)
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("Layer:")).size(11.0).weak());
                for (&idx, layer) in &self.doc.layers {
                    let color = egui::Color32::from_rgb(
                        u8::from_str_radix(&layer.color_hex[1..3], 16).unwrap_or(0),
                        u8::from_str_radix(&layer.color_hex[3..5], 16).unwrap_or(0),
                        u8::from_str_radix(&layer.color_hex[5..7], 16).unwrap_or(0),
                    );
                    let tag = layer.display_tag();
                    let raw_tag = tag.trim_matches(|c| c == '[' || c == ']');
                    let label = if layer.name.is_empty() || layer.name == tag || layer.name == raw_tag {
                        raw_tag
                    } else {
                        layer.name.as_str()
                    };
                    let is_active = self.selected_layer == idx;
                    let text_color = if color.r() as u16 + color.g() as u16 + color.b() as u16 > 400 {
                        egui::Color32::BLACK
                    } else {
                        egui::Color32::WHITE
                    };
                    let btn = egui::Button::new(
                        egui::RichText::new(label).size(11.0).color(text_color).strong()
                    )
                    .fill(color)
                    .min_size(egui::vec2(28.0, 18.0));

                    let resp = ui.add(btn).on_hover_text(format!(
                        "{} (Click to select or assign selected shapes)",
                        layer.display_label()
                    ));
                    if resp.clicked() {
                        self.selected_layer = idx;
                        for &s_idx in &self.selected_shape_indices {
                            if s_idx < self.doc.shapes.len() {
                                self.doc.shapes[s_idx].set_layer_index(idx);
                            }
                        }
                    }
                    if is_active {
                        let r = resp.rect.expand(2.0);
                        ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(2.0_f32, egui::Color32::WHITE), egui::StrokeKind::Outside);
                    }
                }
            });
            ui.add_space(2.0);
            ui.separator();

            let c_green = self.color_green();
            let c_orange = self.color_orange();
            let c_blue = self.color_blue();
            let c_red = self.color_red();

            ui.horizontal(|ui| {
                let is_conn = self.device.status == crate::device::ConnectionStatus::Connected;
                let (badge_color, badge_text) = if is_conn {
                    if self.device.is_physically_connected() {
                        (c_green, tr("Connected (USB)"))
                    } else {
                        (c_orange, tr("Simulator (Demo)"))
                    }
                } else {
                    (c_red, tr("Disconnected"))
                };

                if ui.button(egui::RichText::new(badge_text).strong().color(badge_color)).on_hover_text(tr("Open device connection & console window")).clicked() {
                    self.show_connection_window = true;
                }
                ui.label(format!("{} {}", tr("Controller:"), self.device.controller_type.name()));
                ui.separator();

                let pos_str = match self.length_unit {
                    LengthUnit::Millimeters => format!("X: {:.1} mm, Y: {:.1} mm", self.device.machine_x, self.device.machine_y),
                    LengthUnit::Inches => format!(
                        "X: {:.3} in, Y: {:.3} in ({:.1} mm, {:.1} mm)",
                        self.device.machine_x / 25.4,
                        self.device.machine_y / 25.4,
                        self.device.machine_x,
                        self.device.machine_y
                    ),
                };
                ui.label(format!("{} {}", tr("Machine Pos:"), pos_str));
                ui.separator();

                if self.device.is_job_running {
                    let pct = self.device.job_progress * 100.0;
                    ui.label(
                        egui::RichText::new(format!(
                            "{}: {:.1}% ({}/{})",
                            tr("CUTTING:"), pct, self.device.job_idx, self.device.job_total_lines
                        ))
                        .strong()
                        .color(c_orange),
                    );
                    ui.separator();
                } else if self.device.is_weak_light_on {
                    ui.label(
                        egui::RichText::new(tr("WEAK BEAM (1.0%)"))
                            .strong()
                            .color(c_blue),
                    );
                    ui.separator();
                }

                if self.device.is_framing_active {
                    ui.label(
                        egui::RichText::new(tr("FRAMING IN PROGRESS..."))
                            .strong()
                            .color(c_orange),
                    );
                    ui.separator();
                }

                let disp_status = tr(&self.status_msg);
                ui.label(egui::RichText::new(disp_status).strong());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let bed_str = match self.length_unit {
                        LengthUnit::Millimeters => format!("{:.0} x {:.0} mm", self.doc.width, self.doc.height),
                        LengthUnit::Inches => format!(
                            "{:.2} x {:.2} in ({:.0} x {:.0} mm)",
                            self.doc.width / 25.4,
                            self.doc.height / 25.4,
                            self.doc.width,
                            self.doc.height
                        ),
                    };
                    ui.label(format!("{} {} | {} {} | {} {}", tr("Bed:"), bed_str, tr("Shapes:"), self.doc.shapes.len(), tr("Selected:"), self.selected_shape_indices.len()));
                });
            });
        });

        // -------------------------------------------------------------
        // FLOATING INSPECTOR PANELS & DATA TRAFFIC LOG
        // -------------------------------------------------------------
        self.render_properties_window(ctx);
        self.render_objects_window(ctx);
        self.render_job_control_window(ctx);
        self.render_traffic_window(ctx);

        // -------------------------------------------------------------
        // CENTER 2D CAD CANVAS (Full interactive moving, sizing, drawing)
        // -------------------------------------------------------------
        egui::CentralPanel::default().show(ctx, |ui| {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            // Pan canvas with right mouse or middle mouse
            if response.dragged_by(egui::PointerButton::Secondary) || response.dragged_by(egui::PointerButton::Middle) {
                self.pan += response.drag_delta();
            }

            // Zoom with wheel ONLY when mouse is hovering over the canvas (not over sidebars/toolbars/dialogs)
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    let old_zoom = self.zoom;
                    let factor = 1.0 + scroll * 0.002;
                    let new_zoom = (self.zoom * factor).clamp(0.2, 20.0);

                    let canvas_origin_current = response.rect.min + self.pan;
                    if let Some(mouse_pos) = response.hover_pos() {
                        let pointer_offset = mouse_pos - canvas_origin_current;
                        self.pan -= pointer_offset * (new_zoom / old_zoom - 1.0);
                    }
                    self.zoom = new_zoom;
                }
            }

            let canvas_origin = response.rect.min + self.pan;

            // Laser bed outline
            let bed_w = self.doc.width as f32 * self.zoom;
            let bed_h = self.doc.height as f32 * self.zoom;
            let bed_rect = egui::Rect::from_min_size(canvas_origin, egui::Vec2::new(bed_w, bed_h));

            painter.rect_filled(bed_rect, 0.0, egui::Color32::from_rgb(250, 250, 250));

            // Draw live camera background overlay strictly clipped to the laser bed grid
            if self.camera_calibration.show_on_canvas {
                if let Some(ref texture) = self.camera_texture {
                    let bed_painter = painter.with_clip_rect(bed_rect);
                    self.paint_camera_on_bed(&bed_painter, canvas_origin, texture);
                }
            }

            painter.rect_stroke(bed_rect, 0.0, egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(180, 180, 180)), egui::StrokeKind::Outside);

            // Grid lines radiating from the machine origin corner (so 0,0 is always a clean grid corner)
            let is_origin_right = self.device.settings.origin_corner == crate::device::OriginCorner::TopRight
                || self.device.settings.origin_corner == crate::device::OriginCorner::BottomRight;
            let is_origin_bottom = self.device.settings.origin_corner == crate::device::OriginCorner::BottomLeft
                || self.device.settings.origin_corner == crate::device::OriginCorner::BottomRight;

            // Adaptive grid subdivisions and ruler labels based on zoom level and active units (mm vs inches):
            // (minor_step_mm, major_step_mm, label_step_mm)
            let (minor_step, major_step, label_step) = Self::grid_steps_for_zoom_unit(self.zoom, self.length_unit);

            let minor_tenths = ((minor_step * 10.0).round() as i64).max(1);
            let major_tenths = ((major_step * 10.0).round() as i64).max(1);
            let label_tenths = ((label_step * 10.0).round() as i64).max(1);
            let doc_w_tenths = (self.doc.width * 10.0).round() as i64;
            let doc_h_tenths = (self.doc.height * 10.0).round() as i64;
            let canvas_rect = response.rect;

            // X grid lines radiating from origin corner outwards
            let mut dist_tenths_x = 0;
            while dist_tenths_x <= doc_w_tenths {
                let dist_x = dist_tenths_x as f64 / 10.0;
                let doc_x = if is_origin_right {
                    self.doc.width - dist_x
                } else {
                    dist_x
                };
                let p1 = self.doc_to_screen(Point2D::new(doc_x, 0.0), canvas_origin);
                let p2 = self.doc_to_screen(Point2D::new(doc_x, self.doc.height), canvas_origin);

                // Only render if line is within or near the visible canvas viewport horizontally
                if p1.x >= canvas_rect.left() - 20.0 && p1.x <= canvas_rect.right() + 20.0 {
                    let is_major = dist_tenths_x % major_tenths == 0;
                    let is_label = dist_tenths_x % label_tenths == 0;

                    let stroke_color = if is_major {
                        egui::Color32::from_rgb(205, 210, 220)
                    } else {
                        egui::Color32::from_rgb(238, 240, 243)
                    };
                    let stroke_w = if is_major { 0.8_f32 } else { 0.5_f32 };
                    painter.line_segment([p1, p2], egui::Stroke::new(stroke_w, stroke_color));

                    // Distance label from origin (in mm or inches)
                    if is_label && dist_tenths_x > 0 {
                        let base_y = if is_origin_bottom {
                            p2.y + 10.0
                        } else {
                            p1.y - 10.0
                        };

                        // Keep label visible on screen even when zoomed into bed
                        let text_y = if is_origin_bottom {
                            if p2.y > canvas_rect.bottom() && p1.y < canvas_rect.bottom() {
                                canvas_rect.bottom() - 10.0
                            } else {
                                base_y
                            }
                        } else {
                            if p1.y < canvas_rect.top() && p2.y > canvas_rect.top() {
                                canvas_rect.top() + 10.0
                            } else {
                                base_y
                            }
                        };

                        let label_str = match self.length_unit {
                            LengthUnit::Millimeters => {
                                if dist_tenths_x % 10 == 0 {
                                    format!("{}", dist_tenths_x / 10)
                                } else {
                                    format!("{:.1}", dist_x)
                                }
                            }
                            LengthUnit::Inches => {
                                let dist_in = dist_x / 25.4;
                                if (dist_in.round() - dist_in).abs() < 0.05 {
                                    format!("{:.0}\"", dist_in.round())
                                } else {
                                    format!("{:.2}\"", dist_in)
                                }
                            }
                        };

                        let text_pos = egui::pos2(p1.x, text_y);
                        let pill_w = (label_str.len() as f32) * 6.5 + 6.0;
                        let pill_rect = egui::Rect::from_center_size(text_pos, egui::vec2(pill_w, 13.0));
                        painter.rect_filled(pill_rect, 2.0, egui::Color32::from_rgba_unmultiplied(250, 250, 250, 210));
                        painter.text(
                            text_pos,
                            egui::Align2::CENTER_CENTER,
                            &label_str,
                            egui::FontId::proportional(9.5),
                            egui::Color32::from_rgb(120, 125, 135),
                        );
                    }
                }

                dist_tenths_x += minor_tenths;
            }

            // Y grid lines radiating from origin corner outwards
            let mut dist_tenths_y = 0;
            while dist_tenths_y <= doc_h_tenths {
                let dist_y = dist_tenths_y as f64 / 10.0;
                let doc_y = if is_origin_bottom {
                    self.doc.height - dist_y
                } else {
                    dist_y
                };
                let p1 = self.doc_to_screen(Point2D::new(0.0, doc_y), canvas_origin);
                let p2 = self.doc_to_screen(Point2D::new(self.doc.width, doc_y), canvas_origin);

                // Only render if line is within or near the visible canvas viewport vertically
                if p1.y >= canvas_rect.top() - 20.0 && p1.y <= canvas_rect.bottom() + 20.0 {
                    let is_major = dist_tenths_y % major_tenths == 0;
                    let is_label = dist_tenths_y % label_tenths == 0;

                    let stroke_color = if is_major {
                        egui::Color32::from_rgb(205, 210, 220)
                    } else {
                        egui::Color32::from_rgb(238, 240, 243)
                    };
                    let stroke_w = if is_major { 0.8_f32 } else { 0.5_f32 };
                    painter.line_segment([p1, p2], egui::Stroke::new(stroke_w, stroke_color));

                    // Millimeter distance label from origin
                    if is_label && dist_tenths_y > 0 {
                        let base_x = if is_origin_right {
                            p2.x + 14.0
                        } else {
                            p1.x - 14.0
                        };

                        // Keep label visible on screen even when zoomed into bed
                        let text_x = if is_origin_right {
                            if p2.x > canvas_rect.right() && p1.x < canvas_rect.right() {
                                canvas_rect.right() - 14.0
                            } else {
                                base_x
                            }
                        } else {
                            if p1.x < canvas_rect.left() && p2.x > canvas_rect.left() {
                                canvas_rect.left() + 14.0
                            } else {
                                base_x
                            }
                        };

                        let label_str = match self.length_unit {
                            LengthUnit::Millimeters => {
                                if dist_tenths_y % 10 == 0 {
                                    format!("{}", dist_tenths_y / 10)
                                } else {
                                    format!("{:.1}", dist_y)
                                }
                            }
                            LengthUnit::Inches => {
                                let dist_in = dist_y / 25.4;
                                if (dist_in.round() - dist_in).abs() < 0.05 {
                                    format!("{:.0}\"", dist_in.round())
                                } else {
                                    format!("{:.2}\"", dist_in)
                                }
                            }
                        };

                        let text_pos = egui::pos2(text_x, p1.y);
                        let pill_w = (label_str.len() as f32) * 6.5 + 6.0;
                        let pill_rect = egui::Rect::from_center_size(text_pos, egui::vec2(pill_w, 13.0));
                        painter.rect_filled(pill_rect, 2.0, egui::Color32::from_rgba_unmultiplied(250, 250, 250, 210));
                        painter.text(
                            text_pos,
                            egui::Align2::CENTER_CENTER,
                            &label_str,
                            egui::FontId::proportional(9.5),
                            egui::Color32::from_rgb(120, 125, 135),
                        );
                    }
                }

                dist_tenths_y += minor_tenths;
            }

            // Origin marker (0, 0) - Respects origin_corner (TopRight, TopLeft, etc.)
            let origin_pos = self.origin_screen_pos(canvas_origin);
            painter.circle_filled(origin_pos, 5.0, egui::Color32::RED);
            painter.circle_stroke(origin_pos, 8.0, egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 120, 120)));

            let is_right = self.device.settings.origin_corner == crate::device::OriginCorner::TopRight
                || self.device.settings.origin_corner == crate::device::OriginCorner::BottomRight;
            let is_bot = self.device.settings.origin_corner == crate::device::OriginCorner::BottomLeft
                || self.device.settings.origin_corner == crate::device::OriginCorner::BottomRight;

            painter.text(
                origin_pos + egui::vec2(if is_right { -12.0 } else { 12.0 }, if is_bot { -8.0 } else { 8.0 }),
                if is_right { egui::Align2::RIGHT_CENTER } else { egui::Align2::LEFT_CENTER },
                tr("Origin (0, 0)"),
                egui::FontId::proportional(11.0),
                egui::Color32::RED,
            );

            // Visual learned safety boundary envelope (anchored to configured machine origin 0,0)
            if self.device.settings.custom_boundaries.enabled {
                if let (Some(min_x), Some(max_x), Some(min_y), Some(max_y)) = (
                    self.device.settings.custom_boundaries.min_x,
                    self.device.settings.custom_boundaries.max_x,
                    self.device.settings.custom_boundaries.min_y,
                    self.device.settings.custom_boundaries.max_y,
                ) {
                    let p_min = self.machine_to_screen(Point2D::new(min_x, min_y), canvas_origin);
                    let p_max = self.machine_to_screen(Point2D::new(max_x, max_y), canvas_origin);
                    let bound_rect = egui::Rect::from_two_pos(p_min, p_max);

                    // Clear boundary outline (interior left completely transparent so camera feed & materials remain crystal clear)
                    painter.rect_stroke(
                        bound_rect,
                        0.0,
                        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(46, 125, 50)),
                        egui::StrokeKind::Outside,
                    );

                    // Draw target markers for individual taught corners
                    let taught_corners = [
                        ("Top-Left", self.device.settings.custom_boundaries.corner_top_left),
                        ("Top-Right", self.device.settings.custom_boundaries.corner_top_right),
                        ("Bottom-Left", self.device.settings.custom_boundaries.corner_bottom_left),
                        ("Bottom-Right", self.device.settings.custom_boundaries.corner_bottom_right),
                    ];
                    for (_label, corner_opt) in &taught_corners {
                        if let Some((cx, cy)) = *corner_opt {
                            let c_screen = self.machine_to_screen(Point2D::new(cx, cy), canvas_origin);
                            painter.circle_filled(c_screen, 3.5, egui::Color32::from_rgb(46, 125, 50));
                            painter.circle_stroke(
                                c_screen,
                                5.5,
                                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(120, 200, 130)),
                            );
                        }
                    }

                    // Position label away from the origin corner to prevent overlap
                    let (label_pos, align) = match self.device.settings.origin_corner {
                        crate::device::OriginCorner::TopLeft => (bound_rect.right_top() + egui::vec2(-8.0, 8.0), egui::Align2::RIGHT_TOP),
                        crate::device::OriginCorner::TopRight => (bound_rect.left_top() + egui::vec2(8.0, 8.0), egui::Align2::LEFT_TOP),
                        crate::device::OriginCorner::BottomLeft => (bound_rect.right_bottom() + egui::vec2(-8.0, -8.0), egui::Align2::RIGHT_BOTTOM),
                        crate::device::OriginCorner::BottomRight => (bound_rect.left_bottom() + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM),
                    };
                    painter.text(
                        label_pos,
                        align,
                        tr("Safe Work Envelope (Soft Limits)"),
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_rgb(46, 125, 50),
                    );
                }
            }

            // ---------------------------------------------------------
            // MOUSE INTERACTION: CLICK INSIDE, DRAG-MOVE, DRAG-RESIZE
            // ---------------------------------------------------------
            let pointer_pos = response.interact_pointer_pos().or_else(|| response.hover_pos());
            if let Some(mouse_pos) = pointer_pos {
                let doc_pt = self.screen_to_doc(mouse_pos, canvas_origin);

                let handle_radius_doc = (16.0 / self.zoom as f64).max(6.0);
                let hit_tolerance_doc = (10.0 / self.zoom as f64).max(3.0);

                let shift_held = ui.input(|i| i.modifiers.shift);

                // Cursor management
                if self.active_tool == ActiveTool::Select {
                    if let Some(bounds) = self.selection_bounds() {
                        let handle_hit = self.test_handles(&doc_pt, &bounds, handle_radius_doc);
                        match handle_hit {
                            DragHandle::TopLeft | DragHandle::BottomRight => ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe),
                            DragHandle::TopRight | DragHandle::BottomLeft => ctx.set_cursor_icon(egui::CursorIcon::ResizeNeSw),
                            DragHandle::Left | DragHandle::Right => ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
                            DragHandle::Top | DragHandle::Bottom => ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical),
                            DragHandle::None | DragHandle::Move => {
                                if self.drag_handle == DragHandle::Move {
                                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                                } else if self.find_best_shape_at_point(&doc_pt, hit_tolerance_doc).is_some() {
                                    ctx.set_cursor_icon(egui::CursorIcon::Grab);
                                }
                            }
                        }
                    } else if self.find_best_shape_at_point(&doc_pt, hit_tolerance_doc).is_some() {
                        ctx.set_cursor_icon(egui::CursorIcon::Grab);
                    }
                }

                // 1. Mouse Click (Clean single click without dragging)
                if response.clicked_by(egui::PointerButton::Primary) {
                    if self.active_tool == ActiveTool::DrawText {
                        self.text_pos_x = doc_pt.x.clamp(0.0, self.doc.width);
                        self.text_pos_y = doc_pt.y.clamp(0.0, self.doc.height);
                        self.show_text_dialog = true;
                        self.active_tool = ActiveTool::Select;
                    } else if self.active_tool == ActiveTool::Select {
                        // Keep selection if clicking on or near an existing selection handle
                        let on_handle = if let Some(bounds) = self.selection_bounds() {
                            self.test_handles(&doc_pt, &bounds, handle_radius_doc) != DragHandle::None
                        } else {
                            false
                        };
                        if !on_handle {
                            if let Some(best_idx) = self.find_best_shape_at_point(&doc_pt, hit_tolerance_doc) {
                                if shift_held {
                                    if self.selected_shape_indices.contains(&best_idx) {
                                        self.selected_shape_indices.retain(|&i| i != best_idx);
                                    } else {
                                        self.selected_shape_indices.push(best_idx);
                                    }
                                } else {
                                    self.selected_shape_indices = vec![best_idx];
                                }
                                self.status_msg = format!("Selected shape #{} - Drag or use Arrow Keys to move", best_idx + 1);
                            } else if !shift_held {
                                self.selected_shape_indices.clear();
                            }
                        }
                    } else if self.active_tool == ActiveTool::NodeEdit {
                        let node_radius_doc = (8.0 / self.zoom as f64).max(2.0);
                        let mut hit_node = None;
                        if let Some(&shape_idx) = self.selected_shape_indices.first() {
                            if shape_idx < self.doc.shapes.len() {
                                if let Shape::Path { ref polyline, .. } = self.doc.shapes[shape_idx] {
                                    for (v_idx, pt) in polyline.points.iter().enumerate() {
                                        if pt.distance_to(&doc_pt) <= node_radius_doc {
                                            hit_node = Some(v_idx);
                                            break;
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(v_idx) = hit_node {
                            self.selected_node_index = Some(v_idx);
                            self.status_msg = format!("Selected Node #{} of path (Drag to move, Del to delete)", v_idx + 1);
                        } else if response.double_clicked() {
                            self.insert_node_at_point(doc_pt);
                        } else if let Some(best_idx) = self.find_best_shape_at_point(&doc_pt, hit_tolerance_doc) {
                            self.selected_shape_indices = vec![best_idx];
                            self.convert_selected_to_path();
                            self.selected_node_index = None;
                        }
                    }
                }
                // 2. Mouse Drag Start
                else if response.drag_started_by(egui::PointerButton::Primary) {
                    if self.active_tool == ActiveTool::DrawText {
                        self.text_pos_x = doc_pt.x.clamp(0.0, self.doc.width);
                        self.text_pos_y = doc_pt.y.clamp(0.0, self.doc.height);
                        self.show_text_dialog = true;
                        self.active_tool = ActiveTool::Select;
                    } else if self.active_tool == ActiveTool::Select {
                        // First check if clicking on resize handle of existing selection
                        let mut handle_hit = DragHandle::None;
                        if let Some(bounds) = self.selection_bounds() {
                            // First test origin where mouse was clicked down before dragging started
                            let press_doc_pt = ui.input(|i| i.pointer.press_origin()).map(|p| self.screen_to_doc(p, canvas_origin));
                            if let Some(ref ppt) = press_doc_pt {
                                handle_hit = self.test_handles(ppt, &bounds, handle_radius_doc);
                            }
                            if handle_hit == DragHandle::None {
                                handle_hit = self.test_handles(&doc_pt, &bounds, handle_radius_doc);
                            }
                        }

                        if handle_hit != DragHandle::None {
                            self.push_undo();
                            self.drag_handle = handle_hit;
                            self.drag_start_doc = Some(doc_pt);
                            self.last_drag_doc = Some(doc_pt);
                        } else {
                            // Check if drag started on an already selected shape (that is not locked)
                            let hit_already_selected = self.selected_shape_indices.iter().any(|&idx| {
                                idx < self.doc.shapes.len() && !self.is_shape_locked(idx) && self.doc.shapes[idx].hit_test(&doc_pt, hit_tolerance_doc)
                            });

                            if hit_already_selected {
                                self.push_undo();
                                self.drag_handle = DragHandle::Move;
                                self.drag_start_doc = Some(doc_pt);
                                self.last_drag_doc = Some(doc_pt);
                            } else if let Some(best_idx) = self.find_best_shape_at_point(&doc_pt, hit_tolerance_doc) {
                                if shift_held {
                                    if !self.selected_shape_indices.contains(&best_idx) {
                                        self.selected_shape_indices.push(best_idx);
                                    }
                                } else {
                                    self.selected_shape_indices = vec![best_idx];
                                }
                                if best_idx < self.doc.shapes.len() {
                                    self.selected_layer = self.doc.shapes[best_idx].layer_index();
                                }
                                self.push_undo();
                                self.drag_handle = DragHandle::Move;
                                self.drag_start_doc = Some(doc_pt);
                                self.last_drag_doc = Some(doc_pt);
                                self.status_msg = format!("Selected shape #{} - Drag or use Arrow Keys to move", best_idx + 1);
                            } else {
                                // Clicked on empty space: start rubberband marquee box select or deselect
                                if !shift_held {
                                    self.selected_shape_indices.clear();
                                }
                                self.is_box_selecting = true;
                                self.box_select_start = Some(doc_pt);
                                self.box_select_current = Some(doc_pt);
                                self.drag_handle = DragHandle::None;
                            }
                        }
                    } else if self.active_tool == ActiveTool::NodeEdit {
                        let node_radius_doc = (10.0 / self.zoom as f64).max(2.5);
                        if let Some(&shape_idx) = self.selected_shape_indices.first() {
                            if shape_idx < self.doc.shapes.len() {
                                if let Shape::Path { ref polyline, .. } = self.doc.shapes[shape_idx] {
                                    for (v_idx, pt) in polyline.points.iter().enumerate() {
                                        if pt.distance_to(&doc_pt) <= node_radius_doc {
                                            self.selected_node_index = Some(v_idx);
                                            self.dragging_node_index = Some(v_idx);
                                            self.push_undo();
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // Drawing new shape mode
                        self.push_undo();
                        self.is_drawing = true;
                        self.draw_start = Some(doc_pt);
                        self.draw_current = Some(doc_pt);
                    }
                }
                // 2. Mouse Dragging
                else if response.dragged_by(egui::PointerButton::Primary) {
                    if self.active_tool == ActiveTool::Select {
                        if self.is_box_selecting {
                            self.box_select_current = Some(doc_pt);
                        } else if !self.selected_shape_indices.is_empty() {
                            if let Some(last_pt) = self.last_drag_doc {
                                let delta_x = doc_pt.x - last_pt.x;
                                let delta_y = doc_pt.y - last_pt.y;

                                match self.drag_handle {
                                    DragHandle::Move => {
                                        for &idx in &self.selected_shape_indices {
                                            if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                self.doc.shapes[idx].translate(delta_x, delta_y);
                                            }
                                        }
                                        self.doc_version = self.doc_version.wrapping_add(1);
                                        if let Some(b) = self.selection_bounds() {
                                            self.status_msg = format!("Moving selection to X: {:.1}, Y: {:.1} mm", b.min_x, b.min_y);
                                        }
                                    }
                                    DragHandle::Right => {
                                        if let Some(b) = self.selection_bounds() {
                                            let orig_w = b.width().max(1e-4);
                                            let new_w = (doc_pt.x - b.min_x).max(1.0);
                                            let sx = new_w / orig_w;
                                            let (sy, anchor) = if self.lock_aspect_ratio {
                                                (sx, Point2D::new(b.min_x, (b.min_y + b.max_y) * 0.5))
                                            } else {
                                                (1.0, Point2D::new(b.min_x, b.min_y))
                                            };
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(anchor, sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                            self.status_msg = format!("Resized width to {:.1} mm", new_w);
                                        }
                                    }
                                    DragHandle::Left => {
                                        if let Some(b) = self.selection_bounds() {
                                            let orig_w = b.width().max(1e-4);
                                            let new_w = (b.max_x - doc_pt.x).max(1.0);
                                            let sx = new_w / orig_w;
                                            let (sy, anchor) = if self.lock_aspect_ratio {
                                                (sx, Point2D::new(b.max_x, (b.min_y + b.max_y) * 0.5))
                                            } else {
                                                (1.0, Point2D::new(b.max_x, b.min_y))
                                            };
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(anchor, sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                            self.status_msg = format!("Resized width to {:.1} mm", new_w);
                                        }
                                    }
                                    DragHandle::Bottom => {
                                        if let Some(b) = self.selection_bounds() {
                                            let orig_h = b.height().max(1e-4);
                                            let new_h = (doc_pt.y - b.min_y).max(1.0);
                                            let sy = new_h / orig_h;
                                            let (sx, anchor) = if self.lock_aspect_ratio {
                                                (sy, Point2D::new((b.min_x + b.max_x) * 0.5, b.min_y))
                                            } else {
                                                (1.0, Point2D::new(b.min_x, b.min_y))
                                            };
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(anchor, sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                            self.status_msg = format!("Resized height to {:.1} mm", new_h);
                                        }
                                    }
                                    DragHandle::Top => {
                                        if let Some(b) = self.selection_bounds() {
                                            let orig_h = b.height().max(1e-4);
                                            let new_h = (b.max_y - doc_pt.y).max(1.0);
                                            let sy = new_h / orig_h;
                                            let (sx, anchor) = if self.lock_aspect_ratio {
                                                (sy, Point2D::new((b.min_x + b.max_x) * 0.5, b.max_y))
                                            } else {
                                                (1.0, Point2D::new(b.min_x, b.max_y))
                                            };
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(anchor, sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                            self.status_msg = format!("Resized height to {:.1} mm", new_h);
                                        }
                                    }
                                    DragHandle::BottomRight => {
                                        if let Some(b) = self.selection_bounds() {
                                            let orig_w = b.width().max(1e-4);
                                            let orig_h = b.height().max(1e-4);
                                            let new_w = (doc_pt.x - b.min_x).max(1.0);
                                            let (sx, sy) = if self.lock_aspect_ratio {
                                                let s = new_w / orig_w;
                                                (s, s)
                                            } else {
                                                let new_h = (doc_pt.y - b.min_y).max(1.0);
                                                (new_w / orig_w, new_h / orig_h)
                                            };
                                            let anchor = Point2D::new(b.min_x, b.min_y);
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(anchor, sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                            self.status_msg = format!("Resized to {:.1} x {:.1} mm", b.width(), b.height());
                                        }
                                    }
                                    DragHandle::TopLeft => {
                                        if let Some(b) = self.selection_bounds() {
                                            let orig_w = b.width().max(1e-4);
                                            let orig_h = b.height().max(1e-4);
                                            let new_w = (b.max_x - doc_pt.x).max(1.0);
                                            let (sx, sy) = if self.lock_aspect_ratio {
                                                let s = new_w / orig_w;
                                                (s, s)
                                            } else {
                                                let new_h = (b.max_y - doc_pt.y).max(1.0);
                                                (new_w / orig_w, new_h / orig_h)
                                            };
                                            let anchor = Point2D::new(b.max_x, b.max_y);
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(anchor, sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                            self.status_msg = format!("Resized to {:.1} x {:.1} mm", b.width(), b.height());
                                        }
                                    }
                                    DragHandle::TopRight => {
                                        if let Some(b) = self.selection_bounds() {
                                            let orig_w = b.width().max(1e-4);
                                            let orig_h = b.height().max(1e-4);
                                            let new_w = (doc_pt.x - b.min_x).max(1.0);
                                            let (sx, sy) = if self.lock_aspect_ratio {
                                                let s = new_w / orig_w;
                                                (s, s)
                                            } else {
                                                let new_h = (b.max_y - doc_pt.y).max(1.0);
                                                (new_w / orig_w, new_h / orig_h)
                                            };
                                            let anchor = Point2D::new(b.min_x, b.max_y);
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(anchor, sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                            self.status_msg = format!("Resized to {:.1} x {:.1} mm", b.width(), b.height());
                                        }
                                    }
                                    DragHandle::BottomLeft => {
                                        if let Some(b) = self.selection_bounds() {
                                            let orig_w = b.width().max(1e-4);
                                            let orig_h = b.height().max(1e-4);
                                            let new_w = (b.max_x - doc_pt.x).max(1.0);
                                            let (sx, sy) = if self.lock_aspect_ratio {
                                                let s = new_w / orig_w;
                                                (s, s)
                                            } else {
                                                let new_h = (doc_pt.y - b.min_y).max(1.0);
                                                (new_w / orig_w, new_h / orig_h)
                                            };
                                            let anchor = Point2D::new(b.max_x, b.min_y);
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(anchor, sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                            self.status_msg = format!("Resized to {:.1} x {:.1} mm", b.width(), b.height());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            self.last_drag_doc = Some(doc_pt);
                        }
                    } else if self.active_tool == ActiveTool::NodeEdit {
                        if let Some(node_idx) = self.dragging_node_index {
                            if let Some(&shape_idx) = self.selected_shape_indices.first() {
                                if shape_idx < self.doc.shapes.len() {
                                    if let Shape::Path { ref mut polyline, .. } = self.doc.shapes[shape_idx] {
                                        if node_idx < polyline.points.len() {
                                            polyline.points[node_idx] = doc_pt;
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                        }
                                    }
                                }
                            }
                        }
                    } else if self.is_drawing {
                        self.draw_current = Some(doc_pt);
                    }
                }
                // 3. Mouse Drag Stopped
                else if response.drag_stopped() {
                    self.drag_handle = DragHandle::None;
                    self.drag_start_doc = None;
                    self.last_drag_doc = None;
                    self.dragging_node_index = None;

                    // Finalize rubberband box selection
                    if self.is_box_selecting {
                        if let (Some(start), Some(curr)) = (self.box_select_start, self.box_select_current) {
                            let select_box = BoundingBox::new(
                                start.x.min(curr.x),
                                start.y.min(curr.y),
                                start.x.max(curr.x),
                                start.y.max(curr.y),
                            );

                            if select_box.width() > 2.0 && select_box.height() > 2.0 {
                                for (idx, shape) in self.doc.shapes.iter().enumerate() {
                                    if self.is_shape_locked(idx) {
                                        continue;
                                    }
                                    let b = shape.bounds();
                                    if select_box.contains_point(&b.center()) || (b.min_x >= select_box.min_x && b.max_x <= select_box.max_x && b.min_y >= select_box.min_y && b.max_y <= select_box.max_y) {
                                        if !self.selected_shape_indices.contains(&idx) {
                                            self.selected_shape_indices.push(idx);
                                        }
                                    }
                                }
                            }
                        }
                        self.is_box_selecting = false;
                        self.box_select_start = None;
                        self.box_select_current = None;
                    }

                    // Finalize new shape drawing
                    if self.is_drawing {
                        if let (Some(start), Some(curr)) = (self.draw_start, self.draw_current) {
                            let w = (curr.x - start.x).abs();
                            let h = (curr.y - start.y).abs();
                            let min_x = start.x.min(curr.x);
                            let min_y = start.y.min(curr.y);

                            match self.active_tool {
                                ActiveTool::DrawRect => {
                                    if w > 2.0 && h > 2.0 {
                                        self.doc.add_shape(Shape::Rect {
                                            layer_index: self.selected_layer,
                                            x: min_x,
                                            y: min_y,
                                            width: w,
                                            height: h,
                                            corner_radius: 0.0,
                                        });
                                        self.selected_shape_indices = vec![self.doc.shapes.len() - 1];
                                        self.status_msg = format!("Created Rect {:.1}x{:.1}mm", w, h);
                                    }
                                }
                                ActiveTool::DrawCircle => {
                                    let rx = w * 0.5;
                                    let ry = h * 0.5;
                                    let cx = (start.x + curr.x) * 0.5;
                                    let cy = (start.y + curr.y) * 0.5;

                                    if rx > 1.0 && ry > 1.0 {
                                        if shift_held || (rx - ry).abs() < 1.0 {
                                            let r = (rx + ry) * 0.5;
                                            self.doc.add_shape(Shape::Circle {
                                                layer_index: self.selected_layer,
                                                center: Point2D::new(cx, cy),
                                                radius: r,
                                            });
                                            self.selected_shape_indices = vec![self.doc.shapes.len() - 1];
                                            self.status_msg = format!("Created Circle D={:.1}mm", r * 2.0);
                                        } else {
                                            // Create Oval / Ellipse as smooth closed polyline
                                            let segments = 48;
                                            let mut pts = Vec::with_capacity(segments);
                                            for i in 0..segments {
                                                let a = (i as f64 / segments as f64) * std::f64::consts::TAU;
                                                pts.push(Point2D::new(cx + rx * a.cos(), cy + ry * a.sin()));
                                            }
                                            self.doc.add_shape(Shape::Path {
                                                layer_index: self.selected_layer,
                                                polyline: Polyline::new(pts, true),
                                            });
                                            self.selected_shape_indices = vec![self.doc.shapes.len() - 1];
                                            self.status_msg = format!("Created Oval {:.1}x{:.1}mm", rx * 2.0, ry * 2.0);
                                        }
                                        self.doc_version = self.doc_version.wrapping_add(1);
                                    }
                                }
                                ActiveTool::DrawLine => {
                                    if start.distance_to(&curr) > 2.0 {
                                        self.doc.add_shape(Shape::Path {
                                            layer_index: self.selected_layer,
                                            polyline: Polyline::new(vec![start, curr], false),
                                        });
                                        self.selected_shape_indices = vec![self.doc.shapes.len() - 1];
                                        self.status_msg = "Created Line".to_string();
                                    }
                                }
                                _ => {}
                            }
                        }
                        self.is_drawing = false;
                        self.draw_start = None;
                        self.draw_current = None;
                    }
                }
            }

            // Render rubberband box selection preview
            if self.is_box_selecting {
                if let (Some(start), Some(curr)) = (self.box_select_start, self.box_select_current) {
                    let p1 = self.doc_to_screen(start, canvas_origin);
                    let p2 = self.doc_to_screen(curr, canvas_origin);
                    let select_rect = egui::Rect::from_two_pos(p1, p2);

                    painter.rect_filled(select_rect, 0.0, egui::Color32::from_rgba_premultiplied(0, 120, 255, 30));
                    painter.rect_stroke(select_rect, 0.0, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0, 150, 255)), egui::StrokeKind::Outside);
                }
            }

            // Render drawing preview
            if self.is_drawing {
                let shift_held = ui.input(|i| i.modifiers.shift);
                if let (Some(start), Some(curr)) = (self.draw_start, self.draw_current) {
                    let p1 = self.doc_to_screen(start, canvas_origin);
                    let p2 = self.doc_to_screen(curr, canvas_origin);
                    let preview_stroke = egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(0, 120, 255));

                    match self.active_tool {
                        ActiveTool::DrawRect => {
                            painter.rect_stroke(egui::Rect::from_two_pos(p1, p2), 0.0, preview_stroke, egui::StrokeKind::Outside);
                        }
                        ActiveTool::DrawCircle => {
                            let rx = (p2.x - p1.x).abs() * 0.5;
                            let ry = (p2.y - p1.y).abs() * 0.5;
                            let center = egui::pos2((p1.x + p2.x) * 0.5, (p1.y + p2.y) * 0.5);

                            if shift_held || (rx - ry).abs() < 1.5 {
                                let r = (rx + ry) * 0.5;
                                painter.circle_stroke(center, r, preview_stroke);
                            } else {
                                let segments = 36;
                                let mut screen_pts = Vec::with_capacity(segments);
                                for i in 0..segments {
                                    let a = (i as f32 / segments as f32) * std::f32::consts::TAU;
                                    screen_pts.push(egui::pos2(center.x + rx * a.cos(), center.y + ry * a.sin()));
                                }
                                painter.add(egui::Shape::closed_line(screen_pts, preview_stroke));
                            }
                        }
                        ActiveTool::DrawLine => {
                            painter.line_segment([p1, p2], preview_stroke);
                        }
                        _ => {}
                    }
                }
            }

            // Render all Document Shapes
            for (shape_idx, shape) in self.doc.shapes.iter().enumerate() {
                let layer = self.doc.layers.get(&shape.layer_index());
                let color_hex = layer.map(|l| l.color_hex.as_str()).unwrap_or("#000000");

                let is_selected = self.selected_shape_indices.contains(&shape_idx);

                let color = if is_selected {
                    egui::Color32::from_rgb(255, 140, 0) // Vibrant orange highlight
                } else {
                    egui::Color32::from_rgb(
                        u8::from_str_radix(&color_hex[1..3], 16).unwrap_or(0),
                        u8::from_str_radix(&color_hex[3..5], 16).unwrap_or(0),
                        u8::from_str_radix(&color_hex[5..7], 16).unwrap_or(0),
                    )
                };

                let stroke_width = if is_selected { 2.5_f32 } else { 1.5_f32 };

                if let Shape::Image { x, y, width, height, raw_pixels, raw_width, raw_height, image_path, brightness, contrast, gamma, sharpen, .. } = shape {
                    let min_p = self.doc_to_screen(Point2D::new(*x, *y), canvas_origin);
                    let max_p = self.doc_to_screen(Point2D::new(*x + *width, *y + *height), canvas_origin);
                    let img_rect = egui::Rect::from_two_pos(min_p, max_p);

                    let invert = layer.map_or(false, |l| l.cut_setting.dither_mode == DitherMode::GrayscalePower && l.cut_setting.invert_image);
                    let ceramic_preview = layer.map_or(false, |l| l.cut_setting.dither_mode == DitherMode::GrayscalePower && l.cut_setting.ceramic_preview);
                    let power_min = layer.map_or(0.0, |l| l.cut_setting.power_min);
                    let power_max = layer.map_or(100.0, |l| l.cut_setting.power_max);
                    let default_bands = Vec::new();
                    let ceramic_bands = layer.map_or(default_bands.as_slice(), |l| l.cut_setting.ceramic_bands.as_slice());

                    // Ensure GPU texture is loaded (with live Brightness/Contrast/Gamma/Sharpen & Ceramic Color Preview)
                    let tex_id_opt = if let Some((_, tex)) = self.image_textures.get(&shape_idx) {
                        Some(tex.id())
                    } else if let Some(pixels) = raw_pixels {
                        if *raw_width > 0 && *raw_height > 0 {
                            let processed_rgba = crate::dither::render_processed_preview_rgba(
                                pixels,
                                *raw_width as usize,
                                *raw_height as usize,
                                *brightness,
                                *contrast,
                                *gamma,
                                *sharpen,
                                invert,
                                ceramic_preview,
                                power_min,
                                power_max,
                                ceramic_bands,
                            );
                            let color_img = egui::ColorImage::from_rgba_unmultiplied(
                                [*raw_width as usize, *raw_height as usize],
                                &processed_rgba,
                            );
                            let tex = ctx.load_texture(
                                format!("img_shape_{}", shape_idx),
                                color_img,
                                egui::TextureOptions::LINEAR,
                            );
                            let id = tex.id();
                            self.image_textures.insert(shape_idx, (1, tex));
                            Some(id)
                        } else {
                            None
                        }
                    } else if let Some(path) = image_path {
                        if let Ok(dyn_img) = image::open(path) {
                            let rgba = dyn_img.to_rgba8();
                            let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                            let processed_rgba = crate::dither::render_processed_preview_rgba(
                                rgba.as_raw(),
                                w,
                                h,
                                *brightness,
                                *contrast,
                                *gamma,
                                *sharpen,
                                invert,
                                ceramic_preview,
                                power_min,
                                power_max,
                                ceramic_bands,
                            );
                            let color_img = egui::ColorImage::from_rgba_unmultiplied([w, h], &processed_rgba);
                            let tex = ctx.load_texture(
                                format!("img_shape_{}", shape_idx),
                                color_img,
                                egui::TextureOptions::LINEAR,
                            );
                            let id = tex.id();
                            self.image_textures.insert(shape_idx, (1, tex));
                            Some(id)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(tex_id) = tex_id_opt {
                        painter.image(
                            tex_id,
                            img_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }

                    // Stroke bounding outline around image
                    painter.rect_stroke(
                        img_rect,
                        0.0,
                        egui::Stroke::new(stroke_width, color),
                        egui::StrokeKind::Outside,
                    );
                } else {
                    let stroke = egui::Stroke::new(stroke_width, color);
                    let is_fill_mode = self.doc.layers.get(&shape.layer_index()).map_or(false, |l| {
                        l.cut_setting.mode == CutMode::Fill || l.cut_setting.mode == CutMode::OffsetFill
                    });

                    // Render Filled Shape Preview for Fill / Engrave mode
                    if is_fill_mode {
                        let fill_color = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 65);
                        match shape {
                            Shape::Rect { x, y, width, height, corner_radius, .. } => {
                                let p0 = self.doc_to_screen(Point2D::new(*x, *y), canvas_origin);
                                let p1 = self.doc_to_screen(Point2D::new(*x + *width, *y + *height), canvas_origin);
                                let r = egui::Rect::from_two_pos(p0, p1);
                                painter.rect_filled(r, (*corner_radius as f32 * self.zoom) as f32, fill_color);
                            }
                            Shape::Circle { center, radius, .. } => {
                                let sc = self.doc_to_screen(*center, canvas_origin);
                                painter.circle_filled(sc, (*radius as f32 * self.zoom) as f32, fill_color);
                            }
                            _ => {
                                for poly in shape.to_polylines() {
                                    if poly.is_closed && poly.points.len() >= 3 {
                                        let pts: Vec<egui::Pos2> = poly.points.iter().map(|p| self.doc_to_screen(*p, canvas_origin)).collect();
                                        painter.add(egui::Shape::convex_polygon(pts, fill_color, egui::Stroke::NONE));
                                    }
                                }
                            }
                        }
                    }

                    for poly in shape.to_polylines() {
                        if poly.points.len() >= 2 {
                            let n = poly.points.len();
                            let seg_count = if poly.is_closed { n } else { n - 1 };
                            for i in 0..seg_count {
                                let p1 = self.doc_to_screen(poly.points[i], canvas_origin);
                                let p2 = self.doc_to_screen(poly.points[(i + 1) % n], canvas_origin);
                                if p1.distance_sq(p2) > 0.01 {
                                    painter.line_segment([p1, p2], stroke);
                                }
                            }
                        }
                    }
                }
            }

            // Draw Real-time Live Preview for Image Trace (Bitmap to Vector)
            if self.show_trace_window && !self.trace_preview_polylines.is_empty() {
                let preview_stroke = egui::Stroke::new(2.2_f32, egui::Color32::from_rgb(255, 0, 180));
                let preview_glow = egui::Stroke::new(4.5_f32, egui::Color32::from_rgba_unmultiplied(255, 0, 180, 80));
                for poly in &self.trace_preview_polylines {
                    let n = poly.points.len();
                    if n >= 2 {
                        let seg_count = if poly.is_closed { n } else { n - 1 };
                        for i in 0..seg_count {
                            let p1 = self.doc_to_screen(poly.points[i], canvas_origin);
                            let p2 = self.doc_to_screen(poly.points[(i + 1) % n], canvas_origin);
                            if p1.distance_sq(p2) > 0.01 {
                                painter.line_segment([p1, p2], preview_glow);
                                painter.line_segment([p1, p2], preview_stroke);
                            }
                        }
                    }
                }
            }

            // Draw Combined Selection Bounding Box & 8 Resize Handles OR Node Handles
            if self.active_tool == ActiveTool::NodeEdit {
                if let Some(&shape_idx) = self.selected_shape_indices.first() {
                    if shape_idx < self.doc.shapes.len() {
                        if let Shape::Path { ref polyline, .. } = self.doc.shapes[shape_idx] {
                            for (v_idx, pt) in polyline.points.iter().enumerate() {
                                let scr_p = self.doc_to_screen(*pt, canvas_origin);
                                let is_node_sel = self.selected_node_index == Some(v_idx);
                                let node_sz = if is_node_sel { 12.0 } else { 8.0 };
                                let node_rect = egui::Rect::from_center_size(scr_p, egui::Vec2::splat(node_sz));
                                let fill_color = if is_node_sel {
                                    egui::Color32::from_rgb(255, 140, 0)
                                } else {
                                    egui::Color32::from_rgb(0, 220, 255)
                                };
                                painter.rect_filled(node_rect, 2.0, fill_color);
                                painter.rect_stroke(
                                    node_rect,
                                    2.0,
                                    egui::Stroke::new(1.5_f32, egui::Color32::BLACK),
                                    egui::StrokeKind::Outside,
                                );
                            }
                        }
                    }
                }
            } else if let Some(b) = self.selection_bounds() {
                let min_p = self.doc_to_screen(Point2D::new(b.min_x, b.min_y), canvas_origin);
                let max_p = self.doc_to_screen(Point2D::new(b.max_x, b.max_y), canvas_origin);
                let sel_rect = egui::Rect::from_two_pos(min_p, max_p);

                // Dashed selection outline
                painter.rect_stroke(
                    sel_rect,
                    0.0,
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0, 150, 255)),
                    egui::StrokeKind::Outside,
                );

                // Subtle blue tint to highlight selected region
                painter.rect_filled(sel_rect, 0.0, egui::Color32::from_rgba_unmultiplied(0, 150, 255, 18));

                let any_locked = self.selected_shape_indices.iter().any(|&i| i < self.doc.shapes.len() && self.doc.shapes[i].is_locked());
                if any_locked {
                    painter.text(
                        sel_rect.left_top() + egui::vec2(2.0, -4.0),
                        egui::Align2::LEFT_BOTTOM,
                        "LOCKED",
                        egui::FontId::proportional(12.0),
                        egui::Color32::from_rgb(255, 60, 60),
                    );
                }

                // 8 interactive resize handles
                let handle_pts = [
                    sel_rect.left_top(),
                    egui::pos2(sel_rect.center().x, sel_rect.top()),
                    sel_rect.right_top(),
                    egui::pos2(sel_rect.right(), sel_rect.center().y),
                    sel_rect.right_bottom(),
                    egui::pos2(sel_rect.center().x, sel_rect.bottom()),
                    sel_rect.left_bottom(),
                    egui::pos2(sel_rect.left(), sel_rect.center().y),
                ];

                for hp in handle_pts {
                    painter.rect_filled(egui::Rect::from_center_size(hp, egui::Vec2::splat(10.0)), 2.0, egui::Color32::WHITE);
                    painter.rect_stroke(
                        egui::Rect::from_center_size(hp, egui::Vec2::splat(10.0)),
                        2.0,
                        egui::Stroke::new(1.8_f32, egui::Color32::from_rgb(0, 120, 255)),
                        egui::StrokeKind::Outside,
                    );
                }
            }

            // Laser Head Position Crosshair & Firing / Weak Light Indicator (Relative to Origin Corner)
            let head_pos = self.machine_head_screen_pos(canvas_origin);
            let head_px = head_pos.x;
            let head_py = head_pos.y;

            if self.device.is_job_running && self.device.is_laser_firing {
                // Intense cutting beam with spark
                painter.circle_filled(head_pos, 22.0, egui::Color32::from_rgba_unmultiplied(0, 160, 255, 60));
                painter.circle_filled(head_pos, 11.0, egui::Color32::from_rgba_unmultiplied(255, 120, 0, 190));
                painter.circle_filled(head_pos, 4.0, egui::Color32::WHITE);
                painter.text(
                    head_pos + egui::vec2(16.0, -8.0),
                    egui::Align2::LEFT_BOTTOM,
                    "CUTTING...",
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(255, 160, 0),
                );
            } else if self.device.is_weak_light_on {
                // Glowing cyan/blue laser spot!
                painter.circle_filled(head_pos, 15.0, egui::Color32::from_rgba_unmultiplied(0, 220, 255, 45));
                painter.circle_filled(head_pos, 7.5, egui::Color32::from_rgba_unmultiplied(0, 255, 255, 170));
                painter.circle_filled(head_pos, 3.0, egui::Color32::WHITE);
                painter.text(
                    head_pos + egui::vec2(16.0, -8.0),
                    egui::Align2::LEFT_BOTTOM,
                    "WEAK BEAM (AIM)",
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(0, 220, 255),
                );
            } else {
                painter.line_segment([egui::pos2(head_px - 10.0, head_py), egui::pos2(head_px + 10.0, head_py)], egui::Stroke::new(1.5_f32, egui::Color32::RED));
                painter.line_segment([egui::pos2(head_px, head_py - 10.0), egui::pos2(head_px, head_py + 10.0)], egui::Stroke::new(1.5_f32, egui::Color32::RED));
                painter.circle_stroke(head_pos, 6.0, egui::Stroke::new(1.5_f32, egui::Color32::RED));
            }
        });

        // -------------------------------------------------------------
        // TEXT TOOL DIALOG WINDOW
        // -------------------------------------------------------------
        if self.show_text_dialog {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let mut close_dialog = false;
            let mut do_add_text = false;
            let mut is_open = true;
            let (def_pos, def_size) = self.window_pos_size("text_tool_dialog", [180.0, 90.0], [380.0, 420.0]);

            let resp = egui::Window::new(tr("Text Tool"))
                .id(egui::Id::new("text_tool_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.heading(tr("Text Creation for Laser"));
                    ui.add_space(4.0);

                    ui.label(egui::RichText::new("Enter Text:").strong());
                    ui.text_edit_multiline(&mut self.text_input);

                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Font Type:").strong());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.text_is_singleline, false, "Outline (TrueType / Cut & Engrave)");
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.text_is_singleline, true, "Single-Line (Hershey Simplex / Fast Marking)");
                    });

                    if self.text_is_singleline {
                        ui.label(
                            egui::RichText::new("Single-Line (stroke font) draws each character with a single laser beam path without double outlines. Ideal for fast serial numbers and marking.")
                                .italics()
                                .size(11.0)
                                .color(egui::Color32::from_rgb(100, 160, 255)),
                        );
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("Font:");
                            let available = crate::text::available_system_fonts();
                            egui::ComboBox::from_id_salt("text_font_picker")
                                .selected_text(&self.text_font_name)
                                .show_ui(ui, |ui| {
                                    for (name, path) in available {
                                        if ui.selectable_label(self.text_font_path == path, name).clicked() {
                                            self.text_font_name = name.to_string();
                                            self.text_font_path = path.to_string();
                                        }
                                    }
                                });

                            if ui.button("Browse...").on_hover_text("Select a custom .ttf or .otf file").clicked() {
                                self.prompt_browse_font(ctx);
                            }
                        });
                        ui.label(
                            egui::RichText::new("Outline font generates closed vector paths for cutting (Line) or engraving (Fill).")
                                .italics()
                                .size(11.0)
                                .color(egui::Color32::GRAY),
                        );
                    }

                    ui.add_space(6.0);
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("Height (mm):");
                        ui.add(egui::Slider::new(&mut self.text_font_size, 3.0..=120.0).suffix(" mm"));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Position:");
                        ui.label("X:");
                        ui.add(egui::DragValue::new(&mut self.text_pos_x).speed(1.0).suffix(" mm"));
                        ui.label("Y:");
                        ui.add(egui::DragValue::new(&mut self.text_pos_y).speed(1.0).suffix(" mm"));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Alignment:");
                        ui.selectable_value(&mut self.text_align, crate::text::TextAlign::Left, "Left");
                        ui.selectable_value(&mut self.text_align, crate::text::TextAlign::Center, "Center");
                        ui.selectable_value(&mut self.text_align, crate::text::TextAlign::Right, "Right");
                    });

                    ui.horizontal(|ui| {
                        ui.label("Layer:");
                        let cur_label = self.doc.layers.get(&self.selected_layer).map(|l| l.display_label()).unwrap_or_else(|| format!("[L{:02}]", self.selected_layer));
                        egui::ComboBox::from_id_salt("text_dialog_layer_picker")
                            .selected_text(cur_label)
                            .show_ui(ui, |ui| {
                                for (&idx, layer) in &self.doc.layers {
                                    if ui.selectable_label(self.selected_layer == idx, layer.display_label()).clicked() {
                                        self.selected_layer = idx;
                                    }
                                }
                            });
                    });

                    ui.add_space(4.0);
                    ui.separator();
                    ui.label(egui::RichText::new("Text on Path (Teksti polulla):").strong());
                    let has_path_sel = !self.selected_shape_indices.is_empty();
                    ui.checkbox(&mut self.text_on_path_enabled, "Flow text along selected path / curve");
                    if self.text_on_path_enabled {
                        if !has_path_sel {
                            ui.label(egui::RichText::new("⚠ Select a path or circle on the canvas first!").color(egui::Color32::from_rgb(255, 193, 7)).small());
                        } else {
                            ui.horizontal(|ui| {
                                ui.label("Path Offset:");
                                ui.add(egui::DragValue::new(&mut self.text_on_path_offset).speed(0.5).suffix(" mm"));
                                ui.label("Perpendicular / Dist:");
                                ui.add(egui::DragValue::new(&mut self.text_on_path_perpendicular).speed(0.5).suffix(" mm"));
                            });
                            ui.checkbox(&mut self.text_on_path_reverse, "Reverse path direction");
                        }
                    }

                    ui.add_space(8.0);
                    ui.separator();

                    ui.horizontal(|ui| {
                        let add_btn = egui::Button::new(
                            egui::RichText::new(" Add Text to Bed ")
                                .strong()
                                .color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(0, 120, 215));

                        if ui.add(add_btn).clicked() {
                            do_add_text = true;
                            close_dialog = true;
                        }

                        if ui.button("Close").clicked() {
                            close_dialog = true;
                        }
                    });
                });

            if let Some(r) = resp {
                self.record_window_rect("text_tool_dialog", r.response.rect);
            }
            if do_add_text {
                self.add_text_to_document();
            }
            if close_dialog || !is_open {
                self.show_text_dialog = false;
            }
        }

        // -------------------------------------------------------------
        // OFFSET SHAPES (CONTOUR) DIALOG WINDOW
        // -------------------------------------------------------------
        if self.show_offset_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let mut close_offset = false;
            let mut do_apply_offset = false;
            let mut is_open = true;
            let (def_pos, def_size) = self.window_pos_size("offset_shapes_dialog", [200.0, 100.0], [340.0, 280.0]);

            let resp = egui::Window::new(tr("Offset Shapes (Contour)"))
                .id(egui::Id::new("offset_shapes_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.heading(tr("Create Contour Offset"));
                    ui.label(egui::RichText::new("Generate outward or inward cutting borders around selected shapes, text, or images.").small().weak());
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label("Offset Distance:");
                        ui.add(
                            egui::DragValue::new(&mut self.offset_distance)
                                .speed(0.1)
                                .range(0.05..=200.0)
                                .suffix(" mm"),
                        );
                    });

                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Direction:").strong());
                    ui.horizontal(|ui| {
                        if ui.radio(!self.offset_inward && !self.offset_both, "Outward").clicked() {
                            self.offset_inward = false;
                            self.offset_both = false;
                        }
                        if ui.radio(self.offset_inward && !self.offset_both, "Inward").clicked() {
                            self.offset_inward = true;
                            self.offset_both = false;
                        }
                        if ui.radio(self.offset_both, "Both").clicked() {
                            self.offset_both = true;
                        }
                    });

                    ui.add_space(4.0);
                    ui.checkbox(&mut self.offset_delete_orig, "Delete original shape(s)");

                    ui.add_space(8.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        let apply_btn = egui::Button::new(
                            egui::RichText::new(" Apply Offset ").strong().color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(0, 120, 215));
                        if ui.add(apply_btn).clicked() {
                            do_apply_offset = true;
                            close_offset = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_offset = true;
                        }
                    });
                });

            if let Some(r) = resp {
                self.record_window_rect("offset_shapes_dialog", r.response.rect);
            }
            if do_apply_offset {
                self.apply_offset_selected();
            }
            if close_offset || !is_open {
                self.show_offset_window = false;
            }
        }

        // -------------------------------------------------------------
        // GRID ARRAY WINDOW
        // -------------------------------------------------------------
        if self.show_grid_array_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let mut close_grid = false;
            let mut do_apply = false;
            let mut is_open = true;
            let (def_pos, def_size) = self.window_pos_size("grid_array_dialog", [220.0, 110.0], [340.0, 320.0]);

            let resp = egui::Window::new(tr("Grid Array"))
                .id(egui::Id::new("grid_array_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.heading("Duplicate Grid Array");
                    ui.label(egui::RichText::new("Duplicate selected shape(s) into a matrix of rows and columns with custom spacing or pitch.").small().weak());
                    ui.add_space(6.0);

                    egui::Grid::new("grid_array_params")
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Columns (X):");
                            ui.add(egui::DragValue::new(&mut self.grid_columns).range(1..=100));
                            ui.end_row();

                            ui.label("Rows (Y):");
                            ui.add(egui::DragValue::new(&mut self.grid_rows).range(1..=100));
                            ui.end_row();

                            ui.label(if self.grid_use_spacing { "Gap / Spacing X:" } else { "Pitch Delta X:" });
                            ui.add(egui::DragValue::new(&mut self.grid_spacing_x).speed(0.5).range(0.0..=1000.0).suffix(" mm"));
                            ui.end_row();

                            ui.label(if self.grid_use_spacing { "Gap / Spacing Y:" } else { "Pitch Delta Y:" });
                            ui.add(egui::DragValue::new(&mut self.grid_spacing_y).speed(0.5).range(0.0..=1000.0).suffix(" mm"));
                            ui.end_row();
                        });

                    ui.add_space(4.0);
                    ui.checkbox(&mut self.grid_use_spacing, "Measure spacing as gap between object edges")
                        .on_hover_text("When checked, spacing is added to the shape's width/height. When unchecked, value is center-to-center pitch.");

                    ui.add_space(4.0);
                    let total_copies = self.grid_columns * self.grid_rows;
                    ui.label(egui::RichText::new(format!("Total items: {} ({} new copies)", total_copies, total_copies.saturating_sub(1))).strong());

                    ui.add_space(8.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        let apply_btn = egui::Button::new(
                            egui::RichText::new(" Create Grid ").strong().color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(0, 120, 215));
                        if ui.add(apply_btn).clicked() {
                            do_apply = true;
                            close_grid = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_grid = true;
                        }
                    });
                });

            if let Some(r) = resp {
                self.record_window_rect("grid_array_dialog", r.response.rect);
            }
            if do_apply {
                self.apply_grid_array();
            }
            if close_grid || !is_open {
                self.show_grid_array_window = false;
            }
        }

        // -------------------------------------------------------------
        // RADIAL ARRAY WINDOW
        // -------------------------------------------------------------
        if self.show_radial_array_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let mut close_radial = false;
            let mut do_apply = false;
            let mut is_open = true;
            let (def_pos, def_size) = self.window_pos_size("radial_array_dialog", [240.0, 120.0], [340.0, 340.0]);

            let resp = egui::Window::new(tr("Radial Array (Circular)"))
                .id(egui::Id::new("radial_array_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.heading(tr("Duplicate Radial Array"));
                    ui.label(egui::RichText::new("Duplicate selected shape(s) in a circular pattern around a center point.").small().weak());
                    ui.add_space(6.0);

                    egui::Grid::new("radial_array_params")
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Total Copies:");
                            ui.add(egui::DragValue::new(&mut self.radial_count).range(2..=360));
                            ui.end_row();

                            ui.label("Total Angle:");
                            ui.add(egui::DragValue::new(&mut self.radial_total_angle).speed(1.0).range(-360.0..=360.0).suffix(" °"));
                            ui.end_row();
                        });

                    ui.add_space(4.0);
                    ui.checkbox(&mut self.radial_rotate_copies, "Rotate copies along arc")
                        .on_hover_text("When checked, each copy rotates to face outward. When unchecked, copies stay upright.");

                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Center Point:").strong());
                    ui.checkbox(&mut self.radial_center_auto, "Use Selection Center");
                    if !self.radial_center_auto {
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            ui.add(egui::DragValue::new(&mut self.radial_center_x).speed(1.0).suffix(" mm"));
                            ui.label("Y:");
                            ui.add(egui::DragValue::new(&mut self.radial_center_y).speed(1.0).suffix(" mm"));
                        });
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        let apply_btn = egui::Button::new(
                            egui::RichText::new(" Create Radial Array ").strong().color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(0, 120, 215));
                        if ui.add(apply_btn).clicked() {
                            do_apply = true;
                            close_radial = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_radial = true;
                        }
                    });
                });

            if let Some(r) = resp {
                self.record_window_rect("radial_array_dialog", r.response.rect);
            }
            if do_apply {
                self.apply_radial_array();
            }
            if close_radial || !is_open {
                self.show_radial_array_window = false;
            }
        }

        // -------------------------------------------------------------
        // IMAGE TRACE (BITMAP TO VECTOR) WINDOW
        // -------------------------------------------------------------
        if self.show_trace_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let mut close_trace = false;
            let mut do_trace = false;
            let mut is_open = true;

            // Compute initial preview if not yet computed
            if self.trace_preview_polylines.is_empty() && self.trace_source_shape_idx.is_none() {
                self.update_trace_preview();
            }

            let count = self.trace_preview_polylines.len();
            let total_pts: usize = self.trace_preview_polylines.iter().map(|p| p.points.len()).sum();
            let (def_pos, def_size) = self.window_pos_size("trace_image_dialog", [150.0, 80.0], [400.0, 500.0]);

            let resp = egui::Window::new(tr("Trace Image (Bitmap to Vector)"))
                .id(egui::Id::new("trace_image_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.heading(tr("Trace Raster Image to Vectors"));
                    ui.label(egui::RichText::new("Convert logos, silhouettes, and bitmap graphics into sharp cutting and engraving vector paths.").small().weak());
                    ui.add_space(6.0);

                    let mut changed = false;

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Tracing Mode:").strong());
                        for &m in crate::trace::TraceMode::ALL {
                            if ui.selectable_value(&mut self.trace_mode, m, m.label()).clicked() {
                                changed = true;
                            }
                        }
                    });
                    ui.add_space(4.0);

                    egui::Grid::new("trace_params_grid")
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Threshold (0..255):");
                            if ui.add(egui::Slider::new(&mut self.trace_threshold, 1..=254)).changed() {
                                changed = true;
                            }
                            ui.end_row();

                            ui.label("Blur / Smoothness:");
                            if ui.add(egui::Slider::new(&mut self.trace_blur_radius, 0..=5).text("px radius")).changed() {
                                changed = true;
                            }
                            ui.end_row();

                            ui.label("Minimum Area:");
                            if ui.add(egui::DragValue::new(&mut self.trace_min_area).speed(0.1).range(0.01..=50.0).suffix(" mm²")).changed() {
                                changed = true;
                            }
                            ui.end_row();

                            ui.label("Curve Tolerance:");
                            if ui.add(egui::DragValue::new(&mut self.trace_tolerance).speed(0.01).range(0.01..=1.0).suffix(" mm")).changed() {
                                changed = true;
                            }
                            ui.end_row();
                        });

                    ui.add_space(4.0);
                    if ui.checkbox(&mut self.trace_invert, "Invert (trace bright pixels instead of dark)").changed() {
                        changed = true;
                    }
                    ui.checkbox(&mut self.trace_delete_image, "Replace original bitmap with vector result");

                    if changed {
                        self.update_trace_preview();
                    }

                    ui.add_space(6.0);
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("● Live Preview:").color(egui::Color32::from_rgb(255, 0, 180)).strong());
                            if count > 0 {
                                ui.label(egui::RichText::new(format!("{} vector paths ({} points)", count, total_pts)).strong());
                            } else {
                                ui.label(egui::RichText::new("No contours found (adjust threshold slider)").italics().color(egui::Color32::from_rgb(255, 170, 50)));
                            }
                        });
                        ui.label(egui::RichText::new("Contours are also rendered live in glowing magenta over the image on the canvas.").small().weak());

                        // Interactive mini-preview box inside the window
                        let (preview_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 150.0), egui::Sense::hover());
                        let painter = ui.painter_at(preview_rect);
                        painter.rect_filled(preview_rect, 4.0, egui::Color32::from_rgb(20, 22, 26));
                        painter.rect_stroke(preview_rect, 4.0, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(50, 55, 65)), egui::StrokeKind::Inside);

                        if let Some(src_idx) = self.trace_source_shape_idx {
                            if src_idx < self.doc.shapes.len() {
                                if let Shape::Image { x: img_x, y: img_y, width: img_w, height: img_h, .. } = self.doc.shapes[src_idx] {
                                    if img_w > 0.0 && img_h > 0.0 {
                                        let pad = 12.0_f32;
                                        let avail_w = (preview_rect.width() - pad * 2.0).max(10.0);
                                        let avail_h = (preview_rect.height() - pad * 2.0).max(10.0);
                                        let scale = (avail_w / img_w as f32).min(avail_h / img_h as f32);
                                        let off_x = preview_rect.min.x + pad + (avail_w - img_w as f32 * scale) * 0.5;
                                        let off_y = preview_rect.min.y + pad + (avail_h - img_h as f32 * scale) * 0.5;

                                        let img_box = egui::Rect::from_min_size(
                                            egui::pos2(off_x, off_y),
                                            egui::vec2(img_w as f32 * scale, img_h as f32 * scale),
                                        );
                                        painter.rect_filled(img_box, 0.0, egui::Color32::from_rgb(30, 32, 38));
                                        painter.rect_stroke(img_box, 0.0, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(70, 75, 85)), egui::StrokeKind::Outside);

                                        let stroke = egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 0, 180));
                                        for poly in &self.trace_preview_polylines {
                                            let n = poly.points.len();
                                            if n >= 2 {
                                                let seg_count = if poly.is_closed { n } else { n - 1 };
                                                for i in 0..seg_count {
                                                    let p1 = egui::pos2(
                                                        off_x + (poly.points[i].x - img_x) as f32 * scale,
                                                        off_y + (poly.points[i].y - img_y) as f32 * scale,
                                                    );
                                                    let p2 = egui::pos2(
                                                        off_x + (poly.points[(i + 1) % n].x - img_x) as f32 * scale,
                                                        off_y + (poly.points[(i + 1) % n].y - img_y) as f32 * scale,
                                                    );
                                                    if p1.distance_sq(p2) > 0.01 {
                                                        painter.line_segment([p1, p2], stroke);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        let trace_btn = egui::Button::new(
                            egui::RichText::new(" Trace & Create Vectors ").strong().color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(0, 140, 255));
                        if ui.add(trace_btn).clicked() {
                            do_trace = true;
                            close_trace = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_trace = true;
                        }
                    });
                });

            if let Some(r) = resp {
                self.record_window_rect("trace_image_dialog", r.response.rect);
            }
            if do_trace {
                self.apply_trace_image();
            }
            if close_trace || !is_open {
                self.show_trace_window = false;
                self.trace_preview_polylines.clear();
                self.trace_source_shape_idx = None;
            }
        }

        // -------------------------------------------------------------
        // MANAGE MACHINE PROFILES WINDOW
        // -------------------------------------------------------------
        if self.show_device_profiles_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let mut close_profiles = false;
            let mut is_open = true;
            let (def_pos, def_size) = self.window_pos_size("machine_profiles_dialog", [180.0, 90.0], [460.0, 420.0]);

            let resp = egui::Window::new(tr("Manage Machine Profiles"))
                .id(egui::Id::new("machine_profiles_dialog"))
                .collapsible(true)
                .resizable(true)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.heading(tr("Laser Machine Profiles"));
                    ui.label(egui::RichText::new(tr("Save and switch between different laser cutters and engravers with distinct bed sizes, origins, and laser types.")).small().weak());
                    ui.add_space(8.0);

                    let cur_name = self.device.settings.current_profile_name.clone();
                    let mut switch_to: Option<String> = None;
                    let mut delete_name: Option<String> = None;

                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        for prof in &self.device.settings.profiles {
                            let is_active = prof.name == cur_name;
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    if is_active {
                                        ui.label(egui::RichText::new(tr("● ACTIVE")).color(egui::Color32::from_rgb(50, 205, 50)).strong());
                                    }
                                    ui.label(egui::RichText::new(&prof.name).strong());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if !is_active {
                                            if self.device.settings.profiles.len() > 1 {
                                                if ui.button(tr("Delete")).clicked() {
                                                    delete_name = Some(prof.name.clone());
                                                }
                                            }
                                            if ui.button(tr("Activate")).clicked() {
                                                switch_to = Some(prof.name.clone());
                                            }
                                        }
                                    });
                                });
                                ui.label(egui::RichText::new(format!(
                                    "{} {:.0} x {:.0} mm | Laser: {:?} | Origin: {:?}",
                                    tr("Bed:"), prof.bed_width, prof.bed_height, prof.laser_type, prof.origin_corner
                                )).small().weak());
                            });
                            ui.add_space(2.0);
                        }
                    });

                    if let Some(to_activate) = switch_to {
                        self.device.settings.apply_profile(&to_activate);
                        self.doc.width = self.device.settings.bed_width;
                        self.doc.height = self.device.settings.bed_height;
                        let _ = self.device.settings.save();
                        self.status_msg = format!("Active Laser Profile: {}", to_activate);
                    }

                    if let Some(to_del) = delete_name {
                        self.device.settings.delete_profile(&to_del);
                        let _ = self.device.settings.save();
                        self.status_msg = format!("Deleted profile: {}", to_del);
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(egui::RichText::new(tr("Save Current Settings as New Profile:")).strong());
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.profile_new_name);
                        let can_save = !self.profile_new_name.trim().is_empty();
                        if ui.add_enabled(can_save, egui::Button::new(tr("Save as New Profile"))).clicked() {
                            let trimmed = self.profile_new_name.trim().to_string();
                            self.device.settings.save_current_as_profile(&trimmed);
                            let _ = self.device.settings.save();
                            self.profile_new_name.clear();
                            self.status_msg = format!("Saved new profile: {}", trimmed);
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    if ui.button(tr("Close")).clicked() {
                        close_profiles = true;
                    }
                });

            if let Some(r) = resp {
                self.record_window_rect("machine_profiles_dialog", r.response.rect);
            }
            if close_profiles || !is_open {
                self.show_device_profiles_window = false;
            }
        }

        // -------------------------------------------------------------
        // MATERIAL TEST GENERATOR WINDOW
        // -------------------------------------------------------------
        if self.show_material_test_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let mut close_mat = false;
            let mut do_gen_mat = false;
            let mut is_open = true;
            let (def_pos, def_size) = self.window_pos_size("material_test_dialog", [170.0, 75.0], [390.0, 480.0]);

            let resp = egui::Window::new(tr("Material Test Generator"))
                .id(egui::Id::new("material_test_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.heading("Power vs. Speed Test Matrix");
                    ui.label(
                        egui::RichText::new("Automatically generates a labeled calibration matrix to find the exact speed and power for ceramic paint layers, engraving, or cutting.")
                            .small()
                            .weak(),
                    );
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Test Mode:").strong());
                        ui.radio_value(&mut self.mat_test_is_engrave, true, "Engrave / Ceramic Paint (Grayscale PWM)");
                        ui.radio_value(&mut self.mat_test_is_engrave, false, "Vector Cut (Line)");
                    });

                    ui.add_space(4.0);
                    egui::Grid::new("mat_test_params_grid")
                        .num_columns(3)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Rows (Speeds):");
                            ui.add(egui::DragValue::new(&mut self.mat_test_rows).range(2..=12));
                            ui.label(egui::RichText::new("Y-axis").small().weak());
                            ui.end_row();

                            ui.label("Columns (Powers):");
                            ui.add(egui::DragValue::new(&mut self.mat_test_cols).range(2..=12));
                            ui.label(egui::RichText::new("X-axis").small().weak());
                            ui.end_row();

                            ui.label("Min Speed:");
                            ui.add(egui::DragValue::new(&mut self.mat_test_min_speed).speed(1.0).range(1.0..=1000.0).suffix(" mm/s"));
                            ui.label("Max Speed:");
                            ui.add(egui::DragValue::new(&mut self.mat_test_max_speed).speed(1.0).range(1.0..=1000.0).suffix(" mm/s"));
                            ui.end_row();

                            ui.label("Min Power:");
                            ui.add(egui::DragValue::new(&mut self.mat_test_min_power).speed(1.0).range(1.0..=100.0).suffix(" %"));
                            ui.label("Max Power:");
                            ui.add(egui::DragValue::new(&mut self.mat_test_max_power).speed(1.0).range(1.0..=100.0).suffix(" %"));
                            ui.end_row();

                            ui.label("Square Size:");
                            ui.add(egui::DragValue::new(&mut self.mat_test_box_size).speed(0.5).range(4.0..=30.0).suffix(" mm"));
                            ui.label("");
                            ui.end_row();
                        });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        let gen_btn = egui::Button::new(
                            egui::RichText::new(" Generate Test Grid on Bed ").strong().color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(34, 139, 34));
                        if ui.add(gen_btn).clicked() {
                            do_gen_mat = true;
                            close_mat = true;
                        }
                        if ui.button("Close").clicked() {
                            close_mat = true;
                        }
                    });
                });

            if let Some(r) = resp {
                self.record_window_rect("material_test_dialog", r.response.rect);
            }
            if do_gen_mat {
                self.generate_material_test_grid();
            }
            if close_mat || !is_open {
                self.show_material_test_window = false;
            }
        }

        // -------------------------------------------------------------
        // MATERIAL LIBRARY & PRESET DATABASE WINDOWS
        // -------------------------------------------------------------
        self.show_material_library_window(ctx);
        self.show_save_material_dialog_window(ctx);
        self.show_preview_simulator_window(ctx);
        self.show_nesting_dialog_window(ctx);
        self.show_generators_dialog_windows(ctx);
        self.show_depth_test_dialog_window(ctx);

        // -------------------------------------------------------------
        // 1. DEVICE CONNECTION & CONSOLE WINDOW
        // -------------------------------------------------------------
        if self.show_connection_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let c_green = self.color_green();
            let c_orange = self.color_orange();
            let (def_pos, def_size) = self.window_pos_size("connection_console_dialog", [100.0, 100.0], [450.0, 480.0]);

            let resp = egui::Window::new(tr("Device Connection & Console"))
                .id(egui::Id::new("connection_console_dialog"))
                .collapsible(true)
                .resizable(true)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut self.show_connection_window)
                .show(ctx, |ui| {
                    ui.heading(tr("Serial Port & Controller Connection"));
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(tr("Controller Type:"));
                        egui::ComboBox::from_id_salt("conn_controller_picker")
                            .selected_text(tr(self.device.controller_type.name()))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.device.controller_type, crate::device::ControllerType::Grbl, tr(crate::device::ControllerType::Grbl.name()));
                                ui.selectable_value(&mut self.device.controller_type, crate::device::ControllerType::Smoothieware, tr(crate::device::ControllerType::Smoothieware.name()));
                                ui.selectable_value(&mut self.device.controller_type, crate::device::ControllerType::RuidaUdp, tr(crate::device::ControllerType::RuidaUdp.name()));
                                ui.selectable_value(&mut self.device.controller_type, crate::device::ControllerType::VirtualDemo, tr(crate::device::ControllerType::VirtualDemo.name()));
                            });
                    });

                    // Connection type: local USB/serial or network link to a Raspberry Pi running PhotonCAM Bridge
                    let is_net = crate::device::parse_network_port(&self.device.selected_port).is_some();
                    ui.horizontal(|ui| {
                        ui.label(tr("Connection:"));
                        if ui.selectable_label(!is_net, tr("USB / Serial")).clicked() && is_net {
                            self.device.selected_port = self.available_ports.first().cloned().unwrap_or_default();
                        }
                        if ui.selectable_label(is_net, tr("Network (Raspberry Pi)")).clicked() && !is_net {
                            self.device.selected_port = format!("tcp://photoncam.local:{}", crate::device::DEFAULT_BRIDGE_TCP_PORT);
                        }
                    });

                    if is_net {
                        ui.horizontal(|ui| {
                            ui.label(tr("Bridge address:"));
                            let mut addr = self.device.selected_port.trim_start_matches("tcp://").to_string();
                            if ui.add(egui::TextEdit::singleline(&mut addr).desired_width(220.0).hint_text("photoncam.local:2323")).changed() {
                                self.device.selected_port = format!("tcp://{}", addr.trim());
                            }
                        });
                        ui.label(egui::RichText::new(tr("Enter the Raspberry Pi hostname or IP (e.g. photoncam.local or 192.168.68.90). Default port 2323.")).small().weak());
                    } else {
                    ui.horizontal(|ui| {
                        ui.label(tr("Port:"));
                        if self.available_ports.is_empty() {
                            self.available_ports = crate::device::DeviceManager::scan_available_ports();
                        }

                        if self.device.selected_port.is_empty() && !self.available_ports.is_empty() {
                            self.device.selected_port = self.available_ports[0].clone();
                        }

                        egui::ComboBox::from_id_salt("conn_port_picker")
                            .selected_text(if self.device.selected_port.is_empty() { tr("Select port...") } else { &self.device.selected_port })
                            .show_ui(ui, |ui| {
                                for p in &self.available_ports {
                                    ui.selectable_value(&mut self.device.selected_port, p.clone(), p);
                                }
                            });

                        if ui.button(tr("Scan Ports")).on_hover_text(tr("Refresh connected USB serial ports")).clicked() {
                            self.available_ports = crate::device::DeviceManager::scan_available_ports();
                            if !self.available_ports.is_empty() {
                                self.device.selected_port = self.available_ports[0].clone();
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label(tr("Baud Rate:"));
                        egui::ComboBox::from_id_salt("conn_baud_picker")
                            .selected_text(format!("{}", self.device.selected_baud))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.device.selected_baud, 115200, tr("115200 (GRBL standard)"));
                                ui.selectable_value(&mut self.device.selected_baud, 250000, "250000");
                                ui.selectable_value(&mut self.device.selected_baud, 9600, "9600");
                                ui.selectable_value(&mut self.device.selected_baud, 57600, "57600");
                            });
                    });
                    }

                    ui.add_space(4.0);
                    ui.checkbox(&mut self.device.settings.auto_connect, tr("Auto-connect to this port on startup"));

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let is_conn = self.device.status == crate::device::ConnectionStatus::Connected;
                        if is_conn {
                            if self.device.is_physically_connected() {
                                let lbl = if is_net { "CONNECTED (NETWORK)" } else { "CONNECTED (USB)" };
                                ui.label(egui::RichText::new(tr(lbl)).strong().color(c_green));
                            } else {
                                ui.label(egui::RichText::new(tr("SIMULATOR (DEMO)")).strong().color(c_orange));
                            }
                            if ui.button(tr("Disconnect")).clicked() {
                                self.device.disconnect();
                            }
                        } else {
                            ui.colored_label(egui::Color32::RED, tr("NOT CONNECTED"));
                            let conn_btn = egui::Button::new(
                                egui::RichText::new(tr(" Connect to Device ")).strong().color(egui::Color32::WHITE),
                            ).fill(egui::Color32::from_rgb(0, 140, 255));
                            if ui.add(conn_btn).clicked() {
                                let port = self.device.selected_port.clone();
                                let baud = self.device.selected_baud;
                                let _ = self.device.connect(&port, baud);
                            }
                        }

                        if ui.button(tr("Save Connection")).on_hover_text(tr("Save this port as default for startup")).clicked() {
                            self.device.settings.default_port = self.device.selected_port.clone();
                            self.device.settings.default_baud = self.device.selected_baud;
                            let _ = self.device.settings.save();
                            self.status_msg = tr("Connection settings saved (~/.config/laser_burn/settings.json)").to_string();
                        }
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(egui::RichText::new(tr("G-Code Console & Log Terminal:")).strong());

                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.add_space(2.0);
                            for line in &self.device.console_history {
                                let color = if line.starts_with('>') {
                                    egui::Color32::from_rgb(100, 200, 255)
                                } else if line.contains("error") || line.contains("WARNING") || line.contains("VAROITUS") {
                                    egui::Color32::from_rgb(255, 120, 100)
                                } else if line.contains("ok") || line.contains("CONNECTION") || line.contains("BEAM") || line.contains("YHTEYS") || line.contains("VALO") {
                                    egui::Color32::from_rgb(140, 255, 140)
                                } else {
                                    egui::Color32::LIGHT_GRAY
                                };
                                ui.colored_label(color, line);
                            }
                        });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let text_edit = ui.text_edit_singleline(&mut self.command_input);
                        let send_pressed = ui.button(tr("Send")).clicked() || (text_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if send_pressed && !self.command_input.trim().is_empty() {
                            let cmd = self.command_input.trim().to_string();
                            self.device.send_raw(&cmd);
                            self.command_input.clear();
                            text_edit.request_focus();
                        }

                        if ui.button(tr("Clear Log")).clicked() {
                            self.device.console_history.clear();
                        }
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(tr("Quick Commands:"));
                        if ui.button("$$").on_hover_text(tr("Show GRBL settings")).clicked() {
                            self.device.send_raw("$$");
                        }
                        if ui.button("?").on_hover_text(tr("Query device status and coordinates")).clicked() {
                            self.device.send_raw("?");
                        }
                        if ui.button("$10=19").on_hover_text(tr("Enable pin reporting ($10=19)")).clicked() {
                            self.device.send_raw("$10=19");
                        }
                        if ui.button("$21=1").on_hover_text(tr("Enable hard limit protection ($21=1)")).clicked() {
                            self.device.send_raw("$21=1");
                        }
                        if self.device.settings.enable_limit_switches {
                            if ui.button("$H").on_hover_text(tr("Home machine ($H)")).clicked() {
                                self.device.home();
                            }
                        }
                        if ui.button("$X").on_hover_text(tr("Unlock alarm ($X)")).clicked() {
                            self.device.send_raw("$X");
                            self.device.send_raw("$21=1");
                        }
                    });
                });

            if let Some(r) = resp {
                self.record_window_rect("connection_console_dialog", r.response.rect);
            }
        }

        // -------------------------------------------------------------
        // 2. LASER CONTROL & POSITIONING WINDOW (MOVE & WEAK LIGHT)
        // -------------------------------------------------------------
        if self.show_move_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let sel_center = self.selection_bounds().map(|b| b.center());
            let mut do_start_box_frame = false;
            let mut do_start_contour_frame = false;
            let c_green = self.color_green();
            let c_orange = self.color_orange();
            let (def_pos, def_size) = self.window_pos_size("laser_control_dialog", [140.0, 100.0], [380.0, 520.0]);

            let resp = egui::Window::new(tr("Laser Control & Positioning"))
                .id(egui::Id::new("laser_control_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut self.show_move_window)
                .show(ctx, |ui| {
                    ui.heading(tr("Laser Head Positioning & Jog"));
                    ui.add_space(4.0);

                    // Coordinates box
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Machine Coordinates:").strong());
                            let m_str = match self.length_unit {
                                LengthUnit::Millimeters => format!("X: {:.2} mm   Y: {:.2} mm", self.device.machine_x, self.device.machine_y),
                                LengthUnit::Inches => format!("X: {:.3} in   Y: {:.3} in ({:.1} mm, {:.1} mm)", self.device.machine_x / 25.4, self.device.machine_y / 25.4, self.device.machine_x, self.device.machine_y),
                            };
                            ui.label(m_str);
                        });
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Work Coordinates:").strong());
                            let w_str = match self.length_unit {
                                LengthUnit::Millimeters => format!("X: {:.2} mm   Y: {:.2} mm", self.device.work_x, self.device.work_y),
                                LengthUnit::Inches => format!("X: {:.3} in   Y: {:.3} in ({:.1} mm, {:.1} mm)", self.device.work_x / 25.4, self.device.work_y / 25.4, self.device.work_x, self.device.work_y),
                            };
                            ui.label(w_str);
                        });
                    });

                    ui.add_space(6.0);

                    // Step size buttons
                    ui.horizontal(|ui| {
                        ui.label("Step Size:");
                        match self.length_unit {
                            LengthUnit::Millimeters => {
                                ui.selectable_value(&mut self.jog_step, 0.1, "0.1mm");
                                ui.selectable_value(&mut self.jog_step, 1.0, "1mm");
                                ui.selectable_value(&mut self.jog_step, 5.0, "5mm");
                                ui.selectable_value(&mut self.jog_step, 10.0, "10mm");
                                ui.selectable_value(&mut self.jog_step, 50.0, "50mm");
                                ui.selectable_value(&mut self.jog_step, 100.0, "100mm");
                            }
                            LengthUnit::Inches => {
                                ui.selectable_value(&mut self.jog_step, 2.54 * 0.05, "0.05\"");
                                ui.selectable_value(&mut self.jog_step, 2.54 * 0.1, "0.1\"");
                                ui.selectable_value(&mut self.jog_step, 25.4 * 0.25, "0.25\"");
                                ui.selectable_value(&mut self.jog_step, 25.4 * 0.5, "0.5\"");
                                ui.selectable_value(&mut self.jog_step, 25.4 * 1.0, "1.0\"");
                                ui.selectable_value(&mut self.jog_step, 25.4 * 2.0, "2.0\"");
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Speed (mm/min):");
                        ui.add(egui::Slider::new(&mut self.jog_speed, 500.0..=8000.0).suffix(" mm/min"));
                    });

                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.keyboard_jog_laser, tr("Keyboard Arrow Jogging"));
                    });

                    ui.add_space(4.0);
                    if self.device.is_physically_connected() || self.device.settings.enable_limit_switches {
                        ui.horizontal(|ui| {
                            ui.label("Limit Switches:");
                            let x_active = self.device.pin_limit_x;
                            let y_active = self.device.pin_limit_y;

                            let x_color = if x_active { egui::Color32::from_rgb(255, 60, 60) } else { egui::Color32::from_rgb(130, 150, 170) };
                            let y_color = if y_active { egui::Color32::from_rgb(255, 60, 60) } else { egui::Color32::from_rgb(130, 150, 170) };

                            ui.label(egui::RichText::new(format!("X: {}", if x_active { "TRIGGERED" } else { "Open" })).color(x_color).strong());
                            ui.label(egui::RichText::new(format!("Y: {}", if y_active { "TRIGGERED" } else { "Open" })).color(y_color).strong());

                            if self.device.pin_limit_z {
                                ui.label(egui::RichText::new("Z: Triggered").color(egui::Color32::from_rgb(255, 120, 0)).strong());
                            }

                            if ui.small_button("Sync ($10=19, $21=1)").on_hover_text("Send $10=19 (real-time pin reporting) and $21=1 (hard limits protection) to GRBL").clicked() {
                                self.device.send_raw("$10=19");
                                self.device.send_raw("$21=1");
                                self.device.send_realtime(b'?');
                                self.status_msg = "Sent $10=19 and $21=1 to GRBL".to_string();
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label(tr("Home Speed:"));
                            if ui.add(egui::Slider::new(&mut self.device.settings.homing_speed, 100.0..=1200.0).suffix(" mm/min")).on_hover_text("Gentle seek speed for mechanical switch calibration (default 400 mm/min = 6.6 mm/s)").changed() {
                                let _ = self.device.settings.save();
                            }
                        });
                    }

                    if self.device.grbl_state.starts_with("Alarm") {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("Status: {}", self.device.grbl_state)).color(egui::Color32::from_rgb(255, 60, 60)).strong());
                            if ui.button(tr("Unlock ($X)")).clicked() {
                                self.device.send_raw("$X");
                                self.device.send_raw("$21=1");
                            }
                        });
                    }
                    ui.add_space(4.0);

                    // Interactive D-Pad Grid
                    let step = self.jog_step;
                    let speed = self.jog_speed;

                    let btn_size = egui::vec2(64.0, 32.0);
                    let grid_w = 3.0 * btn_size.x + 2.0 * 4.0;
                    let pad = ((ui.available_width() - grid_w) / 2.0).max(0.0);

                    let is_origin_right = matches!(
                        self.device.settings.origin_corner,
                        crate::device::OriginCorner::TopRight | crate::device::OriginCorner::BottomRight
                    );
                    let is_origin_top = matches!(
                        self.device.settings.origin_corner,
                        crate::device::OriginCorner::TopLeft | crate::device::OriginCorner::TopRight
                    );
                    let (left_lbl, right_lbl) = if is_origin_right { ("X+", "X-") } else { ("X-", "X+") };
                    let (up_lbl, down_lbl) = if is_origin_top { ("Y-", "Y+") } else { ("Y+", "Y-") };
                    let nw_lbl = format!("{}{}", left_lbl, up_lbl);
                    let ne_lbl = format!("{}{}", right_lbl, up_lbl);
                    let sw_lbl = format!("{}{}", left_lbl, down_lbl);
                    let se_lbl = format!("{}{}", right_lbl, down_lbl);

                    ui.horizontal(|ui| {
                        ui.add_space(pad);
                        egui::Grid::new("move_window_grid")
                            .spacing([4.0, 4.0])
                            .min_col_width(btn_size.x)
                            .show(ui, |ui| {
                                // Row 1: NW, Up, NE
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(&nw_lbl).size(11.0))).on_hover_text(format!("Move Up-Left ({})", nw_lbl)).clicked() {
                                    self.device.jog_arrow(-1.0, 1.0, step, speed);
                                    ctx.request_repaint();
                                }
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(up_lbl).strong().size(15.0))).on_hover_text(format!("Move Up ({})", up_lbl)).clicked() {
                                    self.device.jog_arrow(0.0, 1.0, step, speed);
                                    ctx.request_repaint();
                                }
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(&ne_lbl).size(11.0))).on_hover_text(format!("Move Up-Right ({})", ne_lbl)).clicked() {
                                    self.device.jog_arrow(1.0, 1.0, step, speed);
                                    ctx.request_repaint();
                                }
                                ui.end_row();

                                // Row 2: Left, Origin, Right
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(left_lbl).strong().size(15.0))).on_hover_text(format!("Move Left ({})", left_lbl)).clicked() {
                                    self.device.jog_arrow(-1.0, 0.0, step, speed);
                                    ctx.request_repaint();
                                }
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new("0,0").strong().size(14.0))).on_hover_text("Move to origin (G0 X0 Y0)").clicked() {
                                    self.device.go_to_origin();
                                    ctx.request_repaint();
                                }
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(right_lbl).strong().size(15.0))).on_hover_text(format!("Move Right ({})", right_lbl)).clicked() {
                                    self.device.jog_arrow(1.0, 0.0, step, speed);
                                    ctx.request_repaint();
                                }
                                ui.end_row();

                                // Row 3: SW, Down, SE
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(&sw_lbl).size(11.0))).on_hover_text(format!("Move Down-Left ({})", sw_lbl)).clicked() {
                                    self.device.jog_arrow(-1.0, -1.0, step, speed);
                                    ctx.request_repaint();
                                }
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(down_lbl).strong().size(15.0))).on_hover_text(format!("Move Down ({})", down_lbl)).clicked() {
                                    self.device.jog_arrow(0.0, -1.0, step, speed);
                                    ctx.request_repaint();
                                }
                                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(&se_lbl).size(11.0))).on_hover_text(format!("Move Down-Right ({})", se_lbl)).clicked() {
                                    self.device.jog_arrow(1.0, -1.0, step, speed);
                                    ctx.request_repaint();
                                }
                                ui.end_row();
                            });
                    });

                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Go to Corner:").strong());
                    ui.horizontal(|ui| {
                        let (tl_x, tl_y) = self.device.get_corner_coordinates("top_left");
                        let tl_text = format!("Top-Left ({:.0},{:.0})", tl_x, tl_y);
                        if ui.button(tl_text)
                            .on_hover_text(format!("Move laser head directly to top-left corner (X={:.1}, Y={:.1} mm)", tl_x, tl_y))
                            .clicked()
                        {
                            self.device.go_to_corner("top_left");
                            ctx.request_repaint();
                        }

                        let (tr_x, tr_y) = self.device.get_corner_coordinates("top_right");
                        let tr_text = format!("Top-Right ({:.0},{:.0})", tr_x, tr_y);
                        if ui.button(tr_text)
                            .on_hover_text(format!("Move laser head directly to top-right corner (X={:.1}, Y={:.1} mm)", tr_x, tr_y))
                            .clicked()
                        {
                            self.device.go_to_corner("top_right");
                            ctx.request_repaint();
                        }

                        let (bl_x, bl_y) = self.device.get_corner_coordinates("bottom_left");
                        let bl_text = format!("Bottom-Left ({:.0},{:.0})", bl_x, bl_y);
                        if ui.button(bl_text)
                            .on_hover_text(format!("Move laser head directly to bottom-left corner (X={:.1}, Y={:.1} mm)", bl_x, bl_y))
                            .clicked()
                        {
                            self.device.go_to_corner("bottom_left");
                            ctx.request_repaint();
                        }

                        let (br_x, br_y) = self.device.get_corner_coordinates("bottom_right");
                        let br_text = format!("Bottom-Right ({:.0},{:.0})", br_x, br_y);
                        if ui.button(br_text)
                            .on_hover_text(format!("Move laser head directly to bottom-right corner (X={:.1}, Y={:.1} mm)", br_x, br_y))
                            .clicked()
                        {
                            self.device.go_to_corner("bottom_right");
                            ctx.request_repaint();
                        }
                    });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if self.device.settings.enable_limit_switches {
                            let is_homing = self.device.homing_phase.is_some();
                            let reset_btn = egui::Button::new(
                                egui::RichText::new(if is_homing { "Abort Reset" } else { "Reset Origin (Switches)" })
                                    .strong()
                                    .color(egui::Color32::WHITE)
                            ).fill(if is_homing { egui::Color32::from_rgb(220, 50, 40) } else { egui::Color32::from_rgb(33, 115, 185) });

                            if ui.add(reset_btn).on_hover_text(if is_homing { "Click to immediately abort homing move" } else { "Move head to physical limit switches (ignoring soft limits) and calibrate origin (0,0)" }).clicked() {
                                if is_homing {
                                    self.device.stop_job();
                                } else {
                                    self.device.start_mechanical_home_reset();
                                }
                            }
                        }

                        if ui.button("Set Origin (G92)").on_hover_text("Set this position as work origin (0,0)").clicked() {
                            self.device.set_origin_here();
                        }
                        if ui.button("Center on Selection").clicked() {
                            if let Some(c) = sel_center {
                                self.device.go_to_point(c);
                            }
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();

                    // -------------------------------------------------
                    // WEAK LIGHT / AIM BEAM SECTION
                    // -------------------------------------------------
                    ui.heading("Weak Beam / Alignment Pointer");
                    ui.add_space(2.0);

                    let is_on = self.device.is_weak_light_on;
                    let weak_btn_text = if is_on {
                        "WEAK BEAM ON (Click to turn off)"
                    } else {
                        "Turn on weak beam for alignment"
                    };

                    let weak_btn = egui::Button::new(
                        egui::RichText::new(weak_btn_text)
                            .strong()
                            .color(if is_on { egui::Color32::WHITE } else { egui::Color32::from_rgb(220, 240, 255) }),
                    ).fill(if is_on { egui::Color32::from_rgb(255, 69, 0) } else { egui::Color32::from_rgb(40, 70, 110) });

                    if ui.add(weak_btn).clicked() {
                        self.device.toggle_weak_light();
                    }

                    ui.horizontal(|ui| {
                        ui.label("Weak Beam Power:");
                        ui.add(egui::Slider::new(&mut self.device.settings.weak_light_percent, 0.2..=5.0).suffix(" %"));
                    });

                    ui.label(
                        egui::RichText::new("Weak beam (1.0%) turns on laser at low power so you can see the focal point without burning the workpiece.")
                            .italics()
                            .size(11.0)
                            .color(egui::Color32::GRAY),
                    );

                    ui.add_space(8.0);
                    ui.separator();

                    // -------------------------------------------------
                    // FRAMING TEST SECTION
                    // -------------------------------------------------
                    ui.heading("Framing Test with Weak Beam");
                    ui.add_space(2.0);

                    ui.horizontal(|ui| {
                        let box_btn = egui::Button::new(
                            egui::RichText::new(" Box Framing Test ").strong().color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(0, 120, 215));

                        if ui.add(box_btn).on_hover_text("Traces bounding box with weak beam").clicked() {
                            do_start_box_frame = true;
                        }

                        let contour_btn = egui::Button::new(
                            egui::RichText::new(" Contour Framing Test ").strong().color(egui::Color32::WHITE),
                        ).fill(egui::Color32::from_rgb(140, 40, 180));

                        if ui.add(contour_btn).on_hover_text("Traces actual cut paths with weak beam").clicked() {
                            do_start_contour_frame = true;
                        }
                    });

                    if self.device.is_framing_active {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Framing test in progress...").strong().color(c_orange));
                            if ui.button("Stop Test").clicked() {
                                self.device.stop_frame_test();
                            }
                        });
                    }

                    ui.add_space(8.0);
                    ui.separator();

                    // Custom Soft Limits (Corners)
                    ui.heading("Custom Soft Limits (Corners)");
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.device.settings.custom_boundaries.enabled, "Enforce learned soft limits").changed() {
                            let _ = self.device.settings.save();
                        }
                    });

                    ui.label("Jog laser to target corner and save:");
                    ui.horizontal(|ui| {
                        let bl_label = match self.device.settings.custom_boundaries.corner_bottom_left {
                            Some((x, y)) => format!("[Set] Bottom-Left ({:.0},{:.0})", x, y),
                            None => "Bottom-Left".to_string(),
                        };
                        if ui.button(bl_label).on_hover_text("Save current position as bottom-left corner").clicked() {
                            self.device.save_boundary_corner("bottom_left");
                        }

                        let br_label = match self.device.settings.custom_boundaries.corner_bottom_right {
                            Some((x, y)) => format!("[Set] Bottom-Right ({:.0},{:.0})", x, y),
                            None => "Bottom-Right".to_string(),
                        };
                        if ui.button(br_label).on_hover_text("Save current position as bottom-right corner").clicked() {
                            self.device.save_boundary_corner("bottom_right");
                        }
                    });

                    ui.horizontal(|ui| {
                        let tl_label = match self.device.settings.custom_boundaries.corner_top_left {
                            Some((x, y)) => format!("[Set] Top-Left ({:.0},{:.0})", x, y),
                            None => "Top-Left".to_string(),
                        };
                        if ui.button(tl_label).on_hover_text("Save current position as top-left corner").clicked() {
                            self.device.save_boundary_corner("top_left");
                        }

                        let tr_label = match self.device.settings.custom_boundaries.corner_top_right {
                            Some((x, y)) => format!("[Set] Top-Right ({:.0},{:.0})", x, y),
                            None => "Top-Right".to_string(),
                        };
                        if ui.button(tr_label).on_hover_text("Save current position as top-right corner").clicked() {
                            self.device.save_boundary_corner("top_right");
                        }
                    });

                    if let (Some(min_x), Some(max_x), Some(min_y), Some(max_y)) = (
                        self.device.settings.custom_boundaries.min_x,
                        self.device.settings.custom_boundaries.max_x,
                        self.device.settings.custom_boundaries.min_y,
                        self.device.settings.custom_boundaries.max_y,
                    ) {
                        let b_color = if self.device.settings.custom_boundaries.enabled {
                            c_green
                        } else {
                            egui::Color32::GRAY
                        };
                        ui.label(egui::RichText::new(format!(
                            "Limits: X: {:.1}..{:.1}, Y: {:.1}..{:.1} mm (Size: {:.1} x {:.1} mm)",
                            min_x, max_x, min_y, max_y, (max_x - min_x).abs(), (max_y - min_y).abs()
                        )).color(b_color).strong());
                    }

                    if ui.button("Clear Custom Limits").on_hover_text("Clear all taught corners and disable soft limits").clicked() {
                        self.device.clear_boundary_corners();
                    }
                });

            if let Some(r) = resp {
                self.record_window_rect("laser_control_dialog", r.response.rect);
            }
            if do_start_box_frame {
                self.start_box_frame();
            }
            if do_start_contour_frame {
                self.start_contour_frame();
            }
        }

        // -------------------------------------------------------------
        // 3. DEVICE & WORKBED SETTINGS WINDOW
        // -------------------------------------------------------------
        if self.show_settings_window {
            let mut close_settings = false;
            let mut do_export_settings = false;
            let mut do_import_settings = false;
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let (def_pos, def_size) = self.window_pos_size("device_settings_dialog", [160.0, 100.0], [420.0, 560.0]);

            let resp = egui::Window::new(tr("Device & Bed Settings"))
                .id(egui::Id::new("device_settings_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut self.show_settings_window)
                .show(ctx, |ui| {
                    ui.heading(tr("PhotonCAM Settings"));
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr("Language:")).strong());
                        egui::ComboBox::from_id_salt("settings_language_combo")
                            .selected_text(self.device.settings.language.label())
                            .show_ui(ui, |ui| {
                                for &lang in crate::i18n::Language::ALL {
                                    if ui.selectable_label(self.device.settings.language == lang, lang.label()).clicked() {
                                        self.device.settings.language = lang;
                                        let _ = self.device.settings.save();
                                    }
                                }
                            });
                    });
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr("Units:")).strong());
                        egui::ComboBox::from_id_salt("settings_units_combo")
                            .selected_text(self.length_unit.label())
                            .show_ui(ui, |ui| {
                                for &u in LengthUnit::ALL {
                                    if ui.selectable_label(self.length_unit == u, u.label()).clicked() {
                                        self.length_unit = u;
                                    }
                                }
                            });
                    });
                    ui.add_space(4.0);
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr("Machine Profile:")).strong());
                        let cur_prof = self.device.settings.current_profile_name.clone();
                        egui::ComboBox::from_id_salt("settings_machine_profile_combo")
                            .selected_text(&cur_prof)
                            .show_ui(ui, |ui| {
                                let names: Vec<String> = self.device.settings.profiles.iter().map(|p| p.name.clone()).collect();
                                for name in names {
                                    let is_sel = name == cur_prof;
                                    if ui.selectable_label(is_sel, &name).clicked() {
                                        self.device.settings.apply_profile(&name);
                                        self.doc.width = self.device.settings.bed_width;
                                        self.doc.height = self.device.settings.bed_height;
                                    }
                                }
                            });
                        if ui.button(tr("Manage Profiles...")).clicked() {
                            self.show_device_profiles_window = true;
                        }
                    });
                    ui.add_space(4.0);
                    ui.separator();

                    let bed_dim_title = match self.length_unit {
                        LengthUnit::Millimeters => tr("Bed Dimensions (mm):"),
                        LengthUnit::Inches => tr("Bed Dimensions (Inches):"),
                    };
                    ui.label(egui::RichText::new(bed_dim_title).strong());
                    ui.horizontal(|ui| {
                        let is_in = self.length_unit == LengthUnit::Inches;
                        let suf = self.length_unit.suffix();
                        let spd = if is_in { 0.1 } else { 1.0 };
                        let min_r = if is_in { 2.0 } else { 50.0 };
                        let max_r = if is_in { 150.0 } else { 3000.0 };

                        let mut disp_w = self.length_unit.to_display(self.device.settings.bed_width);
                        let mut disp_h = self.length_unit.to_display(self.device.settings.bed_height);

                        ui.label(tr("Width:"));
                        if ui.add(egui::DragValue::new(&mut disp_w).speed(spd).range(min_r..=max_r).suffix(suf)).changed() {
                            self.device.settings.bed_width = self.length_unit.from_display(disp_w);
                            self.doc.width = self.device.settings.bed_width;
                        }
                        ui.label(tr("Height:"));
                        if ui.add(egui::DragValue::new(&mut disp_h).speed(spd).range(min_r..=max_r).suffix(suf)).changed() {
                            self.device.settings.bed_height = self.length_unit.from_display(disp_h);
                            self.doc.height = self.device.settings.bed_height;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label(tr("Origin Corner:"));
                        egui::ComboBox::from_id_salt("settings_origin_picker")
                            .selected_text(match self.device.settings.origin_corner {
                                crate::device::OriginCorner::TopLeft => "Top-Left (Rear-Left)",
                                crate::device::OriginCorner::BottomLeft => "Bottom-Left (Front-Left)",
                                crate::device::OriginCorner::TopRight => "Top-Right (Rear-Right)",
                                crate::device::OriginCorner::BottomRight => "Bottom-Right (Front-Right)",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.device.settings.origin_corner, crate::device::OriginCorner::TopLeft, "Top-Left (Rear-Left)");
                                ui.selectable_value(&mut self.device.settings.origin_corner, crate::device::OriginCorner::BottomLeft, "Bottom-Left (Front-Left)");
                                ui.selectable_value(&mut self.device.settings.origin_corner, crate::device::OriginCorner::TopRight, "Top-Right (Rear-Right)");
                                ui.selectable_value(&mut self.device.settings.origin_corner, crate::device::OriginCorner::BottomRight, "Bottom-Right (Front-Right)");
                            });
                    });

                    ui.add_space(6.0);
                    ui.separator();

                    ui.label(egui::RichText::new(tr("Laser & Beam Parameters:")).strong());
                    ui.horizontal(|ui| {
                        ui.label(tr("Laser Type:"));
                        egui::ComboBox::from_id_salt("settings_laser_type")
                            .selected_text(match self.device.settings.laser_type {
                                crate::device::LaserType::Diode => "Diode Laser (Blue 450nm)",
                                crate::device::LaserType::Co2 => "CO2 Laser (10600nm)",
                                crate::device::LaserType::FiberGalvo => "Fiber / Galvo (1064nm)",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.device.settings.laser_type, crate::device::LaserType::Diode, "Diode Laser (Blue 450nm)");
                                ui.selectable_value(&mut self.device.settings.laser_type, crate::device::LaserType::Co2, "CO2 Laser (10600nm)");
                                ui.selectable_value(&mut self.device.settings.laser_type, crate::device::LaserType::FiberGalvo, "Fiber / Galvo (1064nm)");
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.label(tr("Max Power ($30 / S-max):"));
                        ui.add(egui::DragValue::new(&mut self.device.settings.max_s_value).speed(10.0).range(1.0..=10000.0));
                    });

                    ui.horizontal(|ui| {
                        ui.label(tr("Weak Beam Power for Alignment:"));
                        ui.add(egui::Slider::new(&mut self.device.settings.weak_light_percent, 0.2..=5.0).suffix(" %"));
                    });

                    ui.horizontal(|ui| {
                        ui.label(tr("Beam Kerf (Full Width):"));
                        ui.add(
                            egui::DragValue::new(&mut self.device.settings.default_kerf)
                                .speed(0.005)
                                .max_decimals(3)
                                .range(0.0..=2.0)
                                .suffix(" mm"),
                        ).on_hover_text("Full laser beam width (kerf). PhotonCAM automatically offsets cutlines by half this width (kerf / 2).");
                        ui.label(egui::RichText::new(format!("-> Offset: ±{:.3} mm", self.device.settings.default_kerf * 0.5)).color(egui::Color32::from_rgb(100, 200, 255)).strong());
                    });
                    ui.label(egui::RichText::new("Enter the FULL beam width (e.g. 0.100 mm). PhotonCAM expands outer cuts by +0.05 mm and shrinks holes by -0.05 mm. (In LightBurn this was called 'Kerf Offset' which was width/2).").small().weak());

                    ui.add_space(6.0);
                    ui.separator();

                    ui.label(egui::RichText::new(tr("Speed Limits:")).strong());
                    ui.horizontal(|ui| {
                        ui.label(tr("Rapid Speed G0:"));
                        ui.add(egui::DragValue::new(&mut self.device.settings.rapid_speed).speed(50.0).suffix(" mm/min"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Jog Speed:"));
                        ui.add(egui::DragValue::new(&mut self.device.settings.jog_speed).speed(50.0).suffix(" mm/min"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Framing Test Speed:"));
                        ui.add(egui::DragValue::new(&mut self.device.settings.frame_speed).speed(50.0).suffix(" mm/min"));
                    });

                    ui.add_space(8.0);
                    ui.separator();

                    ui.label(egui::RichText::new(tr("Limit Switches:")).strong());
                    ui.checkbox(&mut self.device.settings.enable_limit_switches, tr("Enable physical limit switches (Switches installed)"));
                    if !self.device.settings.enable_limit_switches {
                        ui.label(egui::RichText::new("Switch-related buttons and homing are disabled by default for machine protection.").small().weak());
                    } else {
                        ui.label(egui::RichText::new("Ensure your machine has working switches and wiring before homing.").small().color(egui::Color32::from_rgb(220, 160, 40)));
                        ui.horizontal(|ui| {
                            ui.label(tr("Homing / Seek Speed:"));
                            if ui.add(egui::Slider::new(&mut self.device.settings.homing_speed, 100.0..=1200.0).suffix(" mm/min")).on_hover_text("Gentle seek speed for mechanical switch calibration (default 400 mm/min = 6.6 mm/s)").changed() {
                                let _ = self.device.settings.save();
                            }
                        });
                    }

                    ui.add_space(8.0);
                    ui.separator();

                    ui.checkbox(&mut self.device.settings.auto_connect, tr("Auto-connect to laser port on startup"));
                    ui.checkbox(&mut self.device.settings.enable_camera_support, tr("Enable Camera & Alignment Features"))
                        .on_hover_text("Uncheck this if your machine does not have an overhead USB camera to hide all camera buttons and menus for a clean, distraction-free interface.");

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new(tr("Save & Close")).strong()).clicked() {
                            if let Err(e) = self.device.settings.save() {
                                self.status_msg = format!("Error saving settings: {}", e);
                            } else {
                                let _ = Document::save_default_layers(&self.doc.layers);
                                self.status_msg = "Settings & material palette saved (~/.config/laser_burn/)".to_string();
                            }
                            close_settings = true;
                        }

                        ui.separator();

                        if ui.button(tr("Export Profile...")).on_hover_text("Save all machine, camera, and layer settings into an exportable profile file (.json)").clicked() {
                            do_export_settings = true;
                        }

                        if ui.button(tr("Import Profile...")).on_hover_text("Load and restore machine settings, camera calibration, and layer materials from an exported profile (.json)").clicked() {
                            do_import_settings = true;
                        }
                    });
                });

            if let Some(r) = resp {
                self.record_window_rect("device_settings_dialog", r.response.rect);
            }
            if close_settings {
                self.show_settings_window = false;
            }
            if do_export_settings {
                self.prompt_export_settings(ctx);
            }
            if do_import_settings {
                self.prompt_import_settings(ctx);
            }
        }

        // -------------------------------------------------------------
        // WEB REMOTE & IPAD BRIDGE IKKUNA
        // -------------------------------------------------------------
        if self.show_web_remote_window {
            let mut is_open = self.show_web_remote_window;
            let mut do_start = false;
            let mut do_stop = false;
            let web_active = self.web_remote_active;
            let web_port = self.web_remote_port;
            let (def_pos, def_size) = self.window_pos_size("web_remote_dialog", [180.0, 110.0], [460.0, 520.0]);

            let resp = egui::Window::new("🌐 PhotonCAM Web Remote & iPad Bridge")
                .id(egui::Id::new("web_remote_dialog"))
                .collapsible(true)
                .resizable(false)
                .default_pos(def_pos)
                .default_size(def_size)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.heading("🌐 Ohjaa laseria iPadilla tai puhelimella");
                    ui.label("Voit ohjata laseria verstaalla millä tahansa tabletilla tai puhelimella langattomasti selaimen kautta.");
                    ui.add_space(8.0);

                    let local_ip = crate::server::BridgeServer::detect_local_ip();
                    let web_url = format!("http://{}:{}", local_ip, web_port);

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Palvelimen tila:").strong());
                            if web_active {
                                ui.label(egui::RichText::new("🟢 PÄÄLLÄ").color(egui::Color32::from_rgb(0, 230, 118)).strong());
                            } else {
                                ui.label(egui::RichText::new("⚪ POIS PÄÄLTÄ").color(egui::Color32::GRAY));
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("iPad / Selainosoite:").strong());
                            ui.label(egui::RichText::new(&web_url).color(egui::Color32::from_rgb(0, 229, 255)).strong());
                        });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if !web_active {
                                if ui.button(egui::RichText::new("▶️ Käynnistä Web Remote").strong()).clicked() {
                                    do_start = true;
                                }
                            } else {
                                if ui.button(egui::RichText::new("⏹️ Pysäytä Palvelin").strong()).clicked() {
                                    do_stop = true;
                                }
                                if ui.button("🌐 Avaa selaimessa").clicked() {
                                    ctx.open_url(egui::OpenUrl::new_tab(&web_url));
                                }
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("📱 iPad / iPhone -käyttöohje:").strong());
                    ui.label("1. Varmista, että iPad on samassa Wi-Fi -verkossa kuin tämä Mac/PC.");
                    ui.label(format!("2. Avaa iPadin Safari ja kirjoita osoitteeksi: {}", web_url));
                    ui.label("3. Napauta Safarin 'Jaa' -painiketta ja valitse 'Lisää Koti-valikkoon' (Add to Home Screen).");
                    ui.label("👉 Tällöin PhotonCAM aukeaa koko ruudun itsenäisenä sovelluksena suoraan iPadin kotivalikosta!");

                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(egui::RichText::new("Raspberry Pi -asennus (Automaattikäynnistys bootissa):").strong());
                    ui.label("Aja Raspberry Pi:n päätteessä yhdellä komennolla:");
                    ui.code("curl -sSL https://raw.githubusercontent.com/pietini69/photoncam/master/scripts/install_pi_bridge.sh | sudo bash");
                    ui.label("Skripti luo systemd-palvelun, joka käynnistää PhotonCAM-sillan automaattisesti aina kun Pi saa virtaa.");
                });

            if let Some(r) = resp {
                self.record_window_rect("web_remote_dialog", r.response.rect);
            }
            self.show_web_remote_window = is_open;
            if do_start {
                self.start_web_remote();
            }
            if do_stop {
                self.stop_web_remote();
            }
        }

        // -------------------------------------------------------------
        // 4. KAMERA-IKKUNA (CAMERA FEED & ALIGNMENT)
        // -------------------------------------------------------------
        if self.show_camera_window {
            let lang = self.device.settings.language;
            let tr = |k| crate::i18n::t(k, lang);
            let mut is_open = self.show_camera_window;
            let mut need_stream_restart = false;

            let max_w = (ctx.screen_rect().width() - 40.0).max(450.0);
            let max_h = (ctx.screen_rect().height() - 40.0).max(520.0);
            let (def_pos, def_size) = self.window_pos_size("camera_feed_dialog", [100.0, 60.0], [500.0, 640.0]);

            let resp = egui::Window::new(tr("Camera Feed & Alignment"))
                .id(egui::Id::new("camera_feed_dialog"))
                .open(&mut is_open)
                .default_pos(def_pos)
                .default_size(def_size)
                .min_size([400.0, 480.0])
                .max_size([max_w, max_h])
                .constrain(true)
                .resizable(true)
                .show(ctx, |ui| {
                    // Row 1: Device Selection & Scan
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Device:").strong());

                        let current_cam_name = if self.camera_manager.devices.is_empty() {
                            "No cameras detected".to_string()
                        } else if let Some(d) = self.camera_manager.devices.get(self.camera_manager.selected_device_idx) {
                            d.name.clone()
                        } else {
                            "Select camera...".to_string()
                        };

                        let dropdown_w = (ui.available_width() - 120.0).clamp(120.0, 260.0);
                        egui::ComboBox::from_id_salt("camera_device_dropdown")
                            .selected_text(&current_cam_name)
                            .width(dropdown_w)
                            .show_ui(ui, |ui| {
                                for (idx, d) in self.camera_manager.devices.iter().enumerate() {
                                    if ui.selectable_value(&mut self.camera_manager.selected_device_idx, idx, &d.name).clicked() {
                                        if self.camera_manager.is_streaming {
                                            need_stream_restart = true;
                                        }
                                    }
                                }
                            });

                        if ui.button("Scan Cameras").on_hover_text("Scan for connected USB and network cameras").clicked() {
                            self.camera_manager.refresh_devices();
                        }
                    });

                    // Row 2: Stream Controls & Status
                    let c_green = self.color_green();
                    let c_orange = self.color_orange();
                    let c_blue = self.color_blue();
                    let c_red = self.color_red();
                    ui.horizontal(|ui| {
                        if !self.camera_manager.is_streaming {
                            let start_btn = egui::Button::new(egui::RichText::new("Start Stream").color(c_green).strong());
                            if ui.add(start_btn).on_hover_text("Start real-time video stream").clicked() {
                                self.camera_manager.start_stream();
                            }
                        } else {
                            let stop_btn = egui::Button::new(egui::RichText::new("Stop Stream").color(c_red).strong());
                            if ui.add(stop_btn).on_hover_text("Stop video stream").clicked() {
                                self.camera_manager.stop_stream();
                            }
                        }

                        let is_capturing = self.oneshot_snapshot.is_some();
                        let snap_btn = egui::Button::new(
                            egui::RichText::new(if is_capturing { "Capturing..." } else { "Update Overlay" })
                                .strong()
                                .color(if is_capturing { egui::Color32::from_rgb(255, 230, 80) } else { egui::Color32::WHITE })
                        ).fill(if is_capturing { egui::Color32::from_rgb(180, 100, 20) } else { egui::Color32::from_rgb(33, 115, 185) });

                        if ui.add(snap_btn).on_hover_text("Capture fresh snapshot of bed and workpiece (Cmd+U)").clicked() {
                            self.trigger_update_overlay();
                        }

                        ui.separator();

                        let (dot_color, state_txt) = if self.camera_manager.is_streaming {
                            (c_green, "LIVE")
                        } else {
                            (egui::Color32::GRAY, "IDLE")
                        };
                        ui.label(egui::RichText::new(format!("{}", state_txt)).color(dot_color).strong());
                    });

                    if need_stream_restart {
                        self.camera_manager.stop_stream();
                        self.camera_manager.start_stream();
                    }

                    ui.separator();

                    // Navigation Tabs
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.camera_window_tab, 0, "4-Point Wizard");
                        ui.selectable_value(&mut self.camera_window_tab, 1, "Manual Sliders");
                        ui.selectable_value(&mut self.camera_window_tab, 2, "Lens & Fisheye");
                        ui.selectable_value(&mut self.camera_window_tab, 3, "Canvas Overlay");
                    });

                    ui.separator();

                    // Fixed Bottom Toolbar docked using TopBottomPanel (No feedback loop)
                    egui::TopBottomPanel::bottom("camera_docked_bottom_panel")
                        .resizable(false)
                        .show_separator_line(true)
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 4)))
                        .show_inside(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.camera_calibration.show_on_canvas, "Show on Canvas");

                                if self.camera_calibration.use_homography {
                                    ui.label(egui::RichText::new("Homography: Active").color(c_green).small().strong());
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button(egui::RichText::new("Save Calibration").color(c_blue).strong()).clicked() {
                                        if let Err(e) = self.camera_calibration.save() {
                                            self.status_msg = format!("Save Error: {}", e);
                                        } else {
                                            self.status_msg = "Camera calibration saved (~/.config/laser_burn/camera_calibration.json)".to_string();
                                        }
                                    }
                                });
                            });
                        });

                    // Scrollable central area fills remaining space without window expansion
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show_inside(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    match self.camera_window_tab {
                                        0 => {
                                            // -------------------------------------------------
                                            // TAB 0: 4-POINT ALIGNMENT WIZARD (HOMOGRAPHY)
                                            // -------------------------------------------------
                                            ui.heading("4-Point Alignment Wizard");
                                            ui.label(egui::RichText::new("Mathematically align camera with 4 scrap papers or cardboard pieces!").small().weak());
                                            ui.add_space(4.0);

                                            // Wizard Step Tabs
                                            ui.horizontal(|ui| {
                                                let step_names = ["1. Place Papers", "2. Burn Marks", "3. Click Targets", "4. Complete"];
                                                for (i, name) in step_names.iter().enumerate() {
                                                    let is_cur = self.wizard_step == i;
                                                    let is_past = self.wizard_step > i;
                                                    let color = if is_cur {
                                                        egui::Color32::from_rgb(100, 210, 255)
                                                    } else if is_past {
                                                        egui::Color32::from_rgb(80, 230, 80)
                                                    } else {
                                                        egui::Color32::GRAY
                                                    };
                                                    if ui.selectable_label(is_cur, egui::RichText::new(*name).color(color).strong()).clicked() {
                                                        self.wizard_step = i;
                                                    }
                                                }
                                            });
                                            ui.separator();
                                            ui.add_space(4.0);

                                            let targets = self.alignment_target_doc_coords();

                                            match self.wizard_step {
                                                0 => {
                                                    // STEP 0: SETUP 4 PAPERS
                                                    ui.label(egui::RichText::new("Step 1: Place 4 small pieces of paper or cardboard").strong());
                                                    ui.label(egui::RichText::new("You do NOT need a large sheet covering the whole bed! Just place 4 small scrap pieces (A5/A6, Post-it notes, tape, or cardboard) under the 4 target locations.").small());
                                                    ui.add_space(6.0);

                                                    ui.horizontal(|ui| {
                                                        ui.label("Margin from Bed Edges (mm):");
                                                        ui.add(egui::DragValue::new(&mut self.camera_calibration.target_inset_mm).speed(1.0).range(15.0..=150.0));
                                                    });
                                                    ui.add(egui::Slider::new(&mut self.camera_calibration.target_inset_mm, 15.0..=150.0).text("Edge Inset"));
                                                    ui.add_space(6.0);

                                                    ui.label(egui::RichText::new("Target Locations on Laser Bed:").strong());
                                                    egui::Frame::group(ui.style())
                                                        .inner_margin(egui::Margin::symmetric(8, 6))
                                                        .show(ui, |ui| {
                                                            let corner_names = ["1. Top-Left", "2. Top-Right", "3. Bottom-Right", "4. Bottom-Left"];
                                                            for i in 0..4 {
                                                                let (x, y) = targets[i];
                                                                ui.horizontal(|ui| {
                                                                    ui.label(egui::RichText::new(corner_names[i]).strong());
                                                                    ui.label(egui::RichText::new(format!("({:.0}, {:.0}) mm", x, y)).monospace());
                                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                        if ui.button("Jog Laser Spot").on_hover_text("Move laser nozzle directly over this spot with weak beam on").clicked() {
                                                                            self.jog_to_alignment_target(i);
                                                                        }
                                                                    });
                                                                });
                                                                if i < 3 {
                                                                    ui.separator();
                                                                }
                                                            }
                                                        });
                                                    ui.add_space(8.0);
                                                    if ui.button(egui::RichText::new("Framing: Visit All 4 Spots").color(egui::Color32::from_rgb(100, 220, 255))).on_hover_text("Laser will visit each of the 4 spots so you can verify paper placement").clicked() {
                                                        for i in 0..4 {
                                                            self.jog_to_alignment_target(i);
                                                        }
                                                    }

                                                    ui.add_space(10.0);
                                                    ui.horizontal(|ui| {
                                                        if ui.button(egui::RichText::new("Next: Proceed to Burn Marks ->").color(c_green).strong()).clicked() {
                                                            self.wizard_step = 1;
                                                        }
                                                        if ui.button("Skip Burning (Marks already placed) ->").clicked() {
                                                            self.wizard_step = 2;
                                                        }
                                                    });
                                                }
                                                1 => {
                                                    // STEP 1: BURN BULLSEYE MARKS
                                                    ui.label(egui::RichText::new("Step 2: Burn 4 High-Contrast Bullseye Marks").strong());
                                                    ui.label(egui::RichText::new("The laser will mark a 10mm crosshair and concentric rings on each of your 4 scrap papers, then park out of the way.").small());
                                                    ui.add_space(6.0);

                                                    ui.horizontal(|ui| {
                                                        ui.label("Laser Power (%):");
                                                        ui.add(egui::Slider::new(&mut self.camera_calibration.burn_power_percent, 1.0..=100.0).custom_formatter(|v, _| format!("{:.0}%", v)));
                                                    });
                                                    ui.horizontal(|ui| {
                                                        ui.label("Marking Speed (mm/min):");
                                                        ui.add(egui::DragValue::new(&mut self.camera_calibration.burn_speed).speed(50.0).range(200.0..=6000.0));
                                                    });
                                                    ui.add_space(8.0);

                                                    if self.device.is_job_running {
                                                        ui.horizontal(|ui| {
                                                            ui.spinner();
                                                            ui.label(egui::RichText::new(format!("Marking in progress: {:.0}%", self.device.job_progress * 100.0)).strong().color(c_orange));
                                                            if ui.button(egui::RichText::new("Stop").color(c_red)).clicked() {
                                                                self.device.stop_job();
                                                            }
                                                        });
                                                    } else {
                                                        ui.horizontal(|ui| {
                                                            if ui.button(egui::RichText::new("Burn 4 Alignment Marks").color(c_orange).strong()).on_hover_text("Execute rapid G-code to burn 4 crosshair bullseyes").clicked() {
                                                                self.burn_alignment_targets();
                                                            }
                                                            if ui.button("Marks Already Burned / Skip ->").clicked() {
                                                                self.wizard_step = 2;
                                                            }
                                                        });
                                                    }

                                                    ui.add_space(10.0);
                                                    ui.horizontal(|ui| {
                                                        if ui.button("<- Back to Paper Setup").clicked() {
                                                            self.wizard_step = 0;
                                                        }
                                                        if ui.button(egui::RichText::new("Next: Click Targets on Camera ->").color(c_green).strong()).clicked() {
                                                            self.wizard_step = 2;
                                                        }
                                                    });
                                                }
                                                2 => {
                                                    // STEP 2: CLICK TARGETS WITH ZOOM LOUPE
                                                    ui.label(egui::RichText::new("Step 3: Click the 4 Bullseye Targets on the Camera Image").strong());
                                                    ui.label(egui::RichText::new("Hover mouse over a target to see the 4x Zoom Loupe for pixel-perfect centering!").small().weak());
                                                    ui.add_space(4.0);

                                                    let target_labels = [
                                                        ("Target 1 (Top-Left)", c_red),
                                                        ("Target 2 (Top-Right)", c_green),
                                                        ("Target 3 (Bottom-Right)", c_orange),
                                                        ("Target 4 (Bottom-Left)", c_blue),
                                                    ];

                                                    ui.horizontal(|ui| {
                                                        for i in 0..4 {
                                                            let is_active = self.wizard_target_idx == i;
                                                            let has_val = self.wizard_points[i].is_some();
                                                            let (name, col) = target_labels[i];
                                                            let text = if has_val { format!("[OK] {}", name) } else { name.to_string() };
                                                            if ui.selectable_label(is_active, egui::RichText::new(text).color(col).strong()).clicked() {
                                                                self.wizard_target_idx = i;
                                                            }
                                                        }
                                                    });

                                                    ui.add_space(6.0);

                                                    // Interactive Camera Canvas with Loupe
                                                    if let Some(ref texture) = self.camera_texture {
                                                        let avail_w = ui.available_width();
                                                        let tex_size = texture.size_vec2();
                                                        if tex_size.x > 0.0 && tex_size.y > 0.0 {
                                                            let aspect = tex_size.y / tex_size.x;
                                                            let target_w = (avail_w - 14.0).clamp(180.0, 480.0);
                                                            let target_h = target_w * aspect;
                                                            let final_size = egui::vec2(target_w, target_h);

                                                            ui.vertical_centered(|ui| {
                                                                let (rect, response) = ui.allocate_exact_size(final_size, egui::Sense::click_and_drag());

                                                                // Draw camera frame
                                                                ui.painter().image(
                                                                    texture.id(),
                                                                    rect,
                                                                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                                                    egui::Color32::WHITE,
                                                                );

                                                                let hover_pos = response.hover_pos();
                                                                if response.clicked() {
                                                                    if let Some(pos) = hover_pos {
                                                                        let u = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
                                                                        let v = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0) as f64;
                                                                        self.wizard_points[self.wizard_target_idx] = Some((u, v));
                                                                        self.wizard_target_idx = (self.wizard_target_idx + 1).min(3);
                                                                    }
                                                                }

                                                                // Draw placed target markers (1, 2, 3, 4)
                                                                for (i, opt_pt) in self.wizard_points.iter().enumerate() {
                                                                    if let Some((u, v)) = *opt_pt {
                                                                        let px = rect.left() + rect.width() * u as f32;
                                                                        let py = rect.top() + rect.height() * v as f32;
                                                                        let marker_pos = egui::pos2(px, py);
                                                                        let (_, col) = target_labels[i];

                                                                        ui.painter().circle_filled(marker_pos, 10.0, col);
                                                                        ui.painter().circle_stroke(marker_pos, 10.0, egui::Stroke::new(1.5_f32, egui::Color32::BLACK));
                                                                        ui.painter().text(
                                                                            marker_pos,
                                                                            egui::Align2::CENTER_CENTER,
                                                                            format!("{}", i + 1),
                                                                            egui::FontId::proportional(13.0),
                                                                            egui::Color32::BLACK,
                                                                        );
                                                                    }
                                                                }

                                                                // Draw Floating Zoom Loupe (Magnifier) near cursor
                                                                if let Some(pos) = hover_pos {
                                                                    if rect.contains(pos) {
                                                                        let u = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                                                                        let v = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);

                                                                        let loupe_size = 130.0_f32;
                                                                        let zoom = self.wizard_magnifier_zoom.clamp(2.0, 8.0);
                                                                        let half_uv = 0.5 / zoom;

                                                                        let uv_min = egui::pos2((u - half_uv).clamp(0.0, 1.0), (v - half_uv).clamp(0.0, 1.0));
                                                                        let uv_max = egui::pos2((u + half_uv).clamp(0.0, 1.0), (v + half_uv).clamp(0.0, 1.0));

                                                                        // Position loupe away from cursor
                                                                        let loupe_x = if pos.x < rect.center().x {
                                                                            rect.right() - loupe_size - 8.0
                                                                        } else {
                                                                            rect.left() + 8.0
                                                                        };
                                                                        let loupe_y = rect.top() + 8.0;
                                                                        let loupe_rect = egui::Rect::from_min_size(egui::pos2(loupe_x, loupe_y), egui::vec2(loupe_size, loupe_size));

                                                                        ui.painter().rect_filled(loupe_rect, 6.0, egui::Color32::BLACK);
                                                                        ui.painter().image(
                                                                            texture.id(),
                                                                            loupe_rect,
                                                                            egui::Rect::from_min_max(uv_min, uv_max),
                                                                            egui::Color32::WHITE,
                                                                        );
                                                                        ui.painter().rect_stroke(loupe_rect, 6.0, egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(0, 230, 255)), egui::StrokeKind::Outside);

                                                                        let loupe_center = loupe_rect.center();
                                                                        let cross_stroke = egui::Stroke::new(1.2_f32, egui::Color32::from_rgb(255, 30, 30));
                                                                        ui.painter().line_segment([egui::pos2(loupe_center.x - 16.0, loupe_center.y), egui::pos2(loupe_center.x + 16.0, loupe_center.y)], cross_stroke);
                                                                        ui.painter().line_segment([egui::pos2(loupe_center.x, loupe_center.y - 16.0), egui::pos2(loupe_center.x, loupe_center.y + 16.0)], cross_stroke);
                                                                        ui.painter().circle_stroke(loupe_center, 4.0, cross_stroke);
                                                                    }
                                                                }
                                                            });
                                                        }
                                                    } else {
                                                        ui.vertical_centered(|ui| {
                                                            ui.add_space(30.0);
                                                            ui.label("Camera stream is currently IDLE. Click 'Start Stream' above to view feed.");
                                                            ui.add_space(30.0);
                                                        });
                                                    }

                                                    ui.add_space(6.0);
                                                    ui.horizontal(|ui| {
                                                        ui.label("Loupe Zoom:");
                                                        ui.add(egui::Slider::new(&mut self.wizard_magnifier_zoom, 2.0..=8.0).custom_formatter(|v, _| format!("{:.1}x", v)));
                                                        if ui.button("Clear Points").clicked() {
                                                            self.wizard_points = [None; 4];
                                                            self.wizard_target_idx = 0;
                                                        }
                                                    });

                                                    ui.add_space(8.0);
                                                    let all_set = self.wizard_points.iter().all(|p| p.is_some());
                                                    if all_set {
                                                        let apply_btn = egui::Button::new(
                                                            egui::RichText::new("Compute & Apply Homography Alignment")
                                                                .color(c_green)
                                                                .strong(),
                                                        );
                                                        if ui.add(apply_btn).clicked() {
                                                            let mut pts = [crate::camera::AlignmentPoint { machine_x: 0.0, machine_y: 0.0, camera_u: 0.0, camera_v: 0.0 }; 4];
                                                            for i in 0..4 {
                                                                let (x, y) = targets[i];
                                                                let (u, v) = self.wizard_points[i].unwrap();
                                                                pts[i] = crate::camera::AlignmentPoint { machine_x: x, machine_y: y, camera_u: u, camera_v: v };
                                                            }
                                                            match self.camera_calibration.update_homography(&pts) {
                                                                Ok(()) => {
                                                                    let _ = self.camera_calibration.save();
                                                                    self.camera_calibration.show_on_canvas = true;
                                                                    self.wizard_step = 3;
                                                                    self.status_msg = "Homography alignment solved & saved! Bed overlay is now active.".to_string();
                                                                }
                                                                Err(e) => {
                                                                    self.status_msg = format!("Alignment calculation error: {}", e);
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        let missing_count = self.wizard_points.iter().filter(|p| p.is_none()).count();
                                                        ui.label(egui::RichText::new(format!("Click {} more target(s) on the camera image above to complete.", missing_count)).strong().color(c_orange));
                                                    }

                                                    ui.add_space(8.0);
                                                    ui.horizontal(|ui| {
                                                        if ui.button("<- Back to Burn").clicked() {
                                                            self.wizard_step = 1;
                                                        }
                                                    });
                                                }
                                                3 => {
                                                    // STEP 3: COMPLETED
                                                    ui.vertical_centered(|ui| {
                                                        ui.add_space(10.0);
                                                        ui.heading(egui::RichText::new("4-Point Alignment Complete!").color(c_green));
                                                        ui.label("The camera is now mathematically aligned to your laser bed coordinates using perspective homography.");
                                                        ui.label("The background image under the grid now exactly matches your physical workpiece.");
                                                        ui.add_space(10.0);

                                                        if ui.button(egui::RichText::new("Recalibrate / Start Over").strong()).clicked() {
                                                            self.wizard_step = 0;
                                                        }
                                                    });
                                                }
                                                _ => {}
                                            }
                                        }
                                        1 => {
                                            // -------------------------------------------------
                                            // TAB 1: BED ALIGNMENT (MANUAL SLIDERS)
                                            // -------------------------------------------------
                                            ui.heading("Bed Alignment (Manual Sliders)");
                                            ui.label(egui::RichText::new("Manual affine controls (offset, scale, rotation).").small().weak());
                                            ui.add_space(6.0);

                                            if self.camera_calibration.use_homography {
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new("[Active] 4-Point Homography").color(c_green).strong());
                                                    if ui.button("Switch to Manual Mode").clicked() {
                                                        self.camera_calibration.use_homography = false;
                                                    }
                                                });
                                                ui.add_space(6.0);
                                            }

                                            ui.label(egui::RichText::new("Separate Zooms X & Y (Image size on laser bed):").strong());

                                            ui.horizontal(|ui| {
                                                ui.label("Zoom X (Width):");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.scale_x).speed(0.005).range(0.1..=4.0));
                                            });
                                            ui.add(egui::Slider::new(&mut self.camera_calibration.scale_x, 0.2..=3.0)
                                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                                                .text("X Width"));

                                            ui.horizontal(|ui| {
                                                ui.label("Zoom Y (Height):");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.scale_y).speed(0.005).range(0.1..=4.0));
                                            });
                                            ui.add(egui::Slider::new(&mut self.camera_calibration.scale_y, 0.2..=3.0)
                                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                                                .text("Y Height"));

                                            ui.horizontal(|ui| {
                                                ui.label("Uniform Zoom (Both):");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.uniform_scale).speed(0.005).range(0.1..=4.0));
                                                if ui.button("Reset 1:1").clicked() {
                                                    self.camera_calibration.scale_x = 1.0;
                                                    self.camera_calibration.scale_y = 1.0;
                                                    self.camera_calibration.uniform_scale = 1.0;
                                                }
                                            });

                                            ui.add_space(8.0);
                                            egui::Grid::new("bed_align_compact_grid")
                                                .spacing([12.0, 10.0])
                                                .show(ui, |ui| {
                                                    ui.label("Offset X (mm):");
                                                    ui.add(egui::DragValue::new(&mut self.camera_calibration.offset_x_mm).speed(0.5).range(-500.0..=500.0));
                                                    ui.end_row();

                                                    ui.label("Offset Y (mm):");
                                                    ui.add(egui::DragValue::new(&mut self.camera_calibration.offset_y_mm).speed(0.5).range(-500.0..=500.0));
                                                    ui.end_row();

                                                    ui.label("Rotation (°):");
                                                    ui.add(egui::DragValue::new(&mut self.camera_calibration.rotation_deg).speed(0.5).range(-180.0..=180.0));
                                                    ui.end_row();
                                                });

                                            ui.add_space(6.0);
                                            ui.horizontal(|ui| {
                                                ui.checkbox(&mut self.camera_calibration.flip_h, "Flip Horizontal");
                                                ui.checkbox(&mut self.camera_calibration.flip_v, "Flip Vertical");
                                            });

                                            ui.add_space(8.0);
                                            ui.horizontal(|ui| {
                                                if ui.button("Fit to Bed").on_hover_text("Fit camera image directly to bed dimensions").clicked() {
                                                    self.camera_calibration.reset_alignment();
                                                }
                                                if ui.button("Reset Alignment").clicked() {
                                                    self.camera_calibration.reset_alignment();
                                                }
                                            });
                                        }
                                        2 => {
                                            // -------------------------------------------------
                                            // TAB 2: FISHEYE LENS & PREVIEW
                                            // -------------------------------------------------
                                            if let Some(ref texture) = self.camera_texture {
                                                let avail_w = ui.available_width();
                                                let tex_size = texture.size_vec2();
                                                if tex_size.x > 0.0 && tex_size.y > 0.0 {
                                                    let aspect = tex_size.y / tex_size.x;
                                                    let target_w = (avail_w - 14.0).clamp(160.0, 460.0);
                                                    let target_h = target_w * aspect;
                                                    let final_size = egui::vec2(target_w, target_h);

                                                    ui.vertical_centered(|ui| {
                                                        let (rect, _response) = ui.allocate_exact_size(final_size, egui::Sense::hover());

                                                        // Draw undistorted live image
                                                        ui.painter().image(
                                                            texture.id(),
                                                            rect,
                                                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                                            egui::Color32::WHITE,
                                                        );

                                                        // Straight alignment guide grid overlay
                                                        if self.camera_calibration.show_guide_grid {
                                                            let guide_color = egui::Color32::from_rgba_unmultiplied(0, 230, 255, 150);
                                                            let stroke = egui::Stroke::new(1.0_f32, guide_color);
                                                            for i in 1..6 {
                                                                let t = i as f32 / 6.0;
                                                                let x = rect.left() + rect.width() * t;
                                                                let y = rect.top() + rect.height() * t;
                                                                ui.painter().line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], stroke);
                                                                ui.painter().line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], stroke);
                                                            }
                                                        }
                                                    });
                                                }
                                            } else {
                                                ui.vertical_centered(|ui| {
                                                    ui.add_space(40.0);
                                                    ui.heading("No Video Stream");
                                                    ui.label("Select a camera above and click 'Start Stream'.");
                                                    ui.add_space(40.0);
                                                });
                                            }

                                            ui.add_space(6.0);
                                            ui.checkbox(&mut self.camera_calibration.show_guide_grid, "Show Alignment Guide Grid");

                                            ui.add_space(6.0);
                                            ui.separator();
                                            ui.label(egui::RichText::new("Fisheye Lens Correction (drag slider left into negative):").strong());

                                            ui.horizontal(|ui| {
                                                ui.label("Distortion (k1):");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.fisheye_k1).speed(0.005).range(-1.0..=1.0));
                                            });
                                            ui.add(egui::Slider::new(&mut self.camera_calibration.fisheye_k1, -1.0..=1.0).show_value(false));

                                            ui.horizontal(|ui| {
                                                ui.label("Fine-tune (k2):");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.fisheye_k2).speed(0.002).range(-0.5..=0.5));
                                            });
                                            ui.add(egui::Slider::new(&mut self.camera_calibration.fisheye_k2, -0.5..=0.5).show_value(false));

                                            ui.horizontal(|ui| {
                                                ui.label("Lens Zoom X:");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.lens_zoom_x).speed(0.005).range(0.2..=4.0));
                                                ui.label("Y:");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.lens_zoom_y).speed(0.005).range(0.2..=4.0));
                                            });

                                            ui.add_space(4.0);
                                            ui.label(egui::RichText::new("Optical Center (extended Y range):").strong());
                                            ui.horizontal(|ui| {
                                                ui.label("Center X:");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.center_x).speed(0.005).range(-2.0..=2.0));
                                                ui.label("Center Y:");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.center_y).speed(0.005).range(-2.0..=2.0));
                                            });
                                            ui.add(egui::Slider::new(&mut self.camera_calibration.center_y, -1.5..=1.5).text("Y Center Slider"));

                                            ui.add_space(6.0);
                                            ui.separator();
                                            ui.label(egui::RichText::new("Pure Zoom (Separate X & Y):").strong());
                                            ui.label(egui::RichText::new("Scale image closer / farther without changing lens distortion:").small().weak());

                                            ui.horizontal(|ui| {
                                                ui.label("Zoom X (Width):");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.scale_x).speed(0.005).range(0.1..=4.0));
                                            });
                                            ui.add(egui::Slider::new(&mut self.camera_calibration.scale_x, 0.2..=3.0)
                                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                                                .text("X Width"));

                                            ui.horizontal(|ui| {
                                                ui.label("Zoom Y (Height):");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.scale_y).speed(0.005).range(0.1..=4.0));
                                            });
                                            ui.add(egui::Slider::new(&mut self.camera_calibration.scale_y, 0.2..=3.0)
                                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                                                .text("Y Height"));

                                            ui.horizontal(|ui| {
                                                ui.label("Uniform Zoom (Both):");
                                                ui.add(egui::DragValue::new(&mut self.camera_calibration.uniform_scale).speed(0.005).range(0.1..=4.0));
                                                if ui.button("Reset 1:1").clicked() {
                                                    self.camera_calibration.scale_x = 1.0;
                                                    self.camera_calibration.scale_y = 1.0;
                                                    self.camera_calibration.uniform_scale = 1.0;
                                                }
                                            });

                                            ui.add_space(4.0);
                                            if ui.button("Reset Lens Distortion").clicked() {
                                                self.camera_calibration.reset_lens();
                                            }
                                        }
                                        _ => {
                                            // -------------------------------------------------
                                            // TAB 3: CANVAS OVERLAY SETTINGS
                                            // -------------------------------------------------
                                            ui.heading("Bed Canvas Background");
                                            ui.add_space(6.0);

                                            ui.checkbox(&mut self.camera_calibration.show_on_canvas, "Show camera image as laser bed background overlay");
                                            ui.label(egui::RichText::new("Shortcut: Cmd+Shift+C toggles canvas overlay on/off anytime.").small().weak());

                                            ui.add_space(8.0);
                                            ui.horizontal(|ui| {
                                                ui.label("Opacity:");
                                                ui.add(egui::Slider::new(&mut self.camera_calibration.opacity, 0.05..=1.0)
                                                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                                            });

                                            ui.add_space(10.0);
                                            ui.label(egui::RichText::new("Tip: With the background overlay enabled, you can place your physical material on the laser bed and visually position cut/engrave vectors directly over it with millimeter precision!").small().color(egui::Color32::from_rgb(220, 200, 140)));
                                        }
                                    }
                                });
                        });
                });

            if let Some(r) = resp {
                self.record_window_rect("camera_feed_dialog", r.response.rect);
            }
            self.show_camera_window = is_open;
        }

        // V2 Photoshop-Style Tool Palette & Flyouts
        self.render_tool_palette_v2(ctx);

        // Track main window dimensions
        let screen_rect = ctx.screen_rect();
        self.ui_layout.main_window_width = Some(screen_rect.width());
        self.ui_layout.main_window_height = Some(screen_rect.height());

        // Ensure layout is saved to disk immediately if closing window
        if ctx.input(|i| i.viewport().close_requested()) {
            self.save_ui_layout_to_disk();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let layout = self.capture_ui_layout();
        eframe::set_value(storage, "photoncam_ui_layout", &layout);
        let _ = layout.save_to_disk();
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(10)
    }
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    #[test]
    fn test_copy_paste_single_and_cascading() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();
        app.clipboard_shapes.clear();

        app.doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
            corner_radius: 0.0,
        });
        assert_eq!(app.doc.shapes.len(), 1);

        app.selected_shape_indices = vec![0];
        app.copy_selected();
        assert_eq!(app.clipboard_shapes.len(), 1);
        assert_eq!(app.doc.shapes.len(), 1);

        // First paste (+5mm)
        app.paste();
        assert_eq!(app.doc.shapes.len(), 2);
        assert_eq!(app.selected_shape_indices, vec![1]);
        if let Shape::Rect { x, y, width, height, .. } = &app.doc.shapes[1] {
            assert_eq!(*x, 15.0);
            assert_eq!(*y, 15.0);
            assert_eq!(*width, 20.0);
            assert_eq!(*height, 20.0);
        } else {
            panic!("Expected pasted shape to be Rect");
        }

        // Second paste (cascades +5mm more -> +10mm from original)
        app.paste();
        assert_eq!(app.doc.shapes.len(), 3);
        assert_eq!(app.selected_shape_indices, vec![2]);
        if let Shape::Rect { x, y, .. } = &app.doc.shapes[2] {
            assert_eq!(*x, 20.0);
            assert_eq!(*y, 20.0);
        } else {
            panic!("Expected second pasted shape to be Rect at (20, 20)");
        }
    }

    #[test]
    fn test_cut_and_paste() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();
        app.clipboard_shapes.clear();

        app.doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 50.0,
            y: 50.0,
            width: 30.0,
            height: 30.0,
            corner_radius: 0.0,
        });

        app.selected_shape_indices = vec![0];
        app.cut_selected();

        assert_eq!(app.doc.shapes.len(), 0);
        assert!(app.selected_shape_indices.is_empty());
        assert_eq!(app.clipboard_shapes.len(), 1);

        app.paste();
        assert_eq!(app.doc.shapes.len(), 1);
        assert_eq!(app.selected_shape_indices, vec![0]);
        if let Shape::Rect { x, y, .. } = &app.doc.shapes[0] {
            assert_eq!(*x, 55.0);
            assert_eq!(*y, 55.0);
        } else {
            panic!("Expected pasted shape to be Rect");
        }
    }

    #[test]
    fn test_duplicate() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        app.doc.add_shape(Shape::Circle {
            layer_index: 1,
            center: Point2D::new(100.0, 100.0),
            radius: 20.0,
        });

        app.selected_shape_indices = vec![0];
        app.duplicate_selected();

        assert_eq!(app.doc.shapes.len(), 2);
        assert_eq!(app.selected_shape_indices, vec![1]);
        if let Shape::Circle { center, radius, layer_index } = &app.doc.shapes[1] {
            assert_eq!(center.x, 105.0);
            assert_eq!(center.y, 105.0);
            assert_eq!(*radius, 20.0);
            assert_eq!(*layer_index, 1);
        } else {
            panic!("Expected duplicated shape to be Circle");
        }
    }

    #[test]
    fn test_multi_selection_copy_paste_delete() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        app.doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 10.0,
            y: 10.0,
            width: 10.0,
            height: 10.0,
            corner_radius: 0.0,
        });
        app.doc.add_shape(Shape::Circle {
            layer_index: 0,
            center: Point2D::new(40.0, 40.0),
            radius: 5.0,
        });

        app.selected_shape_indices = vec![0, 1];
        app.copy_selected();
        assert_eq!(app.clipboard_shapes.len(), 2);

        app.paste();
        assert_eq!(app.doc.shapes.len(), 4);
        assert_eq!(app.selected_shape_indices, vec![2, 3]);

        app.delete_selected();
        assert_eq!(app.doc.shapes.len(), 2);
        assert!(app.selected_shape_indices.is_empty());
    }

    #[test]
    fn test_locked_shape_protection() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        // Add a locked group shape
        app.doc.add_shape(Shape::Group {
            layer_index: 0,
            shapes: vec![Shape::Rect {
                layer_index: 0,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                corner_radius: 0.0,
            }],
            locked: true,
        });

        // Add an unlocked shape
        app.doc.add_shape(Shape::Circle {
            layer_index: 0,
            center: Point2D::new(50.0, 50.0),
            radius: 10.0,
        });

        // Select both and try to delete
        app.selected_shape_indices = vec![0, 1];
        app.delete_selected();

        // Locked shape (index 0) must remain!
        assert_eq!(app.doc.shapes.len(), 1);
        assert!(app.doc.shapes[0].is_locked());

        // Copy locked shape and paste -> new shape must be unlocked!
        app.selected_shape_indices = vec![0];
        app.copy_selected();
        app.paste();

        assert_eq!(app.doc.shapes.len(), 2);
        assert!(!app.doc.shapes[1].is_locked());
    }

    #[test]
    fn test_layer_locking_protection() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        // Add Layer 1 and mark it locked
        let mut l1 = Layer::new(1, "Jig / Alignment", "#FF8800");
        l1.locked = true;
        app.doc.layers.insert(1, l1);

        // Add a shape on Layer 1 (locked layer)
        app.doc.add_shape(Shape::Rect {
            layer_index: 1,
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 50.0,
            corner_radius: 0.0,
        });

        // Add an unlocked shape on Layer 0
        app.doc.add_shape(Shape::Circle {
            layer_index: 0,
            center: Point2D::new(100.0, 100.0),
            radius: 15.0,
        });

        // Shape 0 belongs to locked layer 1
        assert!(app.is_shape_locked(0), "Shape on locked layer must be reported as locked");
        assert!(!app.is_shape_locked(1), "Shape on unlocked layer 0 must not be locked");

        // Clicking on shape 0 must be ignored by find_best_shape_at_point
        let hit = app.find_best_shape_at_point(&Point2D::new(20.0, 20.0), 5.0);
        assert_eq!(hit, None, "Clicking locked layer shape must return None (clicks fall through)");

        // Clicking on shape 1 must hit shape 1
        let hit1 = app.find_best_shape_at_point(&Point2D::new(100.0, 100.0), 5.0);
        assert_eq!(hit1, Some(1), "Clicking unlocked shape must succeed");

        // If both shapes are selected and delete_selected is called:
        app.selected_shape_indices = vec![0, 1];
        app.delete_selected();

        // Shape 0 on locked layer must be safely preserved!
        assert_eq!(app.doc.shapes.len(), 1, "Only unlocked shape should be deleted");
        assert_eq!(app.doc.shapes[0].layer_index(), 1);
    }

    #[test]
    fn test_dual_units_inches_and_metric() {
        let mut app = LaserBurnApp::default();
        assert_eq!(app.length_unit, LengthUnit::Millimeters);

        // 25.4 mm is exactly 1.0 inch
        assert!((app.length_unit.to_display(25.4) - 25.4).abs() < 1e-6);

        app.length_unit = LengthUnit::Inches;
        assert_eq!(app.length_unit.suffix(), " in");
        assert!((app.length_unit.to_display(25.4) - 1.0).abs() < 1e-6);
        assert!((app.length_unit.from_display(2.5) - 63.5).abs() < 1e-6);

        let dual_str = app.length_unit.format_dual(50.8);
        assert!(dual_str.contains("50.8 mm"));
        assert!(dual_str.contains("2.000 in"));
    }

    #[test]
    fn test_undo_separation_preserves_laser_parameters() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();

        // Calibrate laser parameters on Layer 0
        if let Some(l0) = app.doc.layers.get_mut(&0) {
            l0.cut_setting.speed = 850.0;
            l0.cut_setting.power_max = 95.0;
            l0.cut_setting.num_passes = 3;
            l0.cut_setting.air_assist = true;
        }

        // Draw initial shape
        app.doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
            corner_radius: 0.0,
        });

        // Push undo before moving shape
        app.push_undo();
        app.doc.shapes[0].translate(50.0, 50.0);
        let b_moved = app.doc.shapes[0].bounds();
        assert_eq!(b_moved.min_x, 50.0);

        // Execute Undo
        app.undo();

        // Geometric shape position must be reverted to 0.0
        let b_reverted = app.doc.shapes[0].bounds();
        assert_eq!(b_reverted.min_x, 0.0);

        // CRITICAL: Laser cut parameters must NOT be reverted or lost!
        let l0 = app.doc.layers.get(&0).expect("Layer 0 must exist");
        assert_eq!(l0.cut_setting.speed, 850.0, "Speed must be preserved across vector undos");
        assert_eq!(l0.cut_setting.power_max, 95.0, "Power must be preserved across vector undos");
        assert_eq!(l0.cut_setting.num_passes, 3);
        assert!(l0.cut_setting.air_assist);
    }

    #[test]
    fn test_rotate_selected_and_flip() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        app.doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
            corner_radius: 0.0,
        });

        app.selected_shape_indices = vec![0];

        // Rotate +1.0 degree
        app.rotate_selected(1.0);
        let b1 = app.doc.shapes[0].bounds();
        let c1 = b1.center();
        assert!((c1.x - 20.0).abs() < 1e-4);
        assert!((c1.y - 20.0).abs() < 1e-4);

        // Rotate back -1.0 degree
        app.rotate_selected(-1.0);
        let b2 = app.doc.shapes[0].bounds();
        let c2 = b2.center();
        assert!((c2.x - 20.0).abs() < 1e-4);
        assert!((c2.y - 20.0).abs() < 1e-4);

        // Flip H and Flip V
        app.flip_selected_h();
        app.flip_selected_v();
        let b3 = app.doc.shapes[0].bounds();
        let c3 = b3.center();
        assert!((c3.x - 20.0).abs() < 1e-4);
        assert!((c3.y - 20.0).abs() < 1e-4);
    }

    #[test]
    fn test_adaptive_grid_steps_for_zoom() {
        // Zoomed out far: 50mm minor, 100mm major, 100mm label
        assert_eq!(LaserBurnApp::grid_steps_for_zoom(0.4), (50.0, 100.0, 100.0));

        // Normal fit / compact zoom (e.g. 0.8 - 1.2): 10mm minor, 50mm major, 50mm label
        assert_eq!(LaserBurnApp::grid_steps_for_zoom(1.0), (10.0, 50.0, 50.0));

        // Slight zoom-in (e.g. 1.5 - 2.5): 5mm minor, 20mm major, 20mm label
        assert_eq!(LaserBurnApp::grid_steps_for_zoom(1.8), (5.0, 20.0, 20.0));

        // Medium close-up (e.g. 3.0 - 5.0): 2mm minor, 10mm major, 10mm label
        assert_eq!(LaserBurnApp::grid_steps_for_zoom(3.5), (2.0, 10.0, 10.0));

        // High detail (e.g. 6.0 - 9.0): 1mm minor, 5mm major, 5mm label
        assert_eq!(LaserBurnApp::grid_steps_for_zoom(7.0), (1.0, 5.0, 5.0));

        // Micro detail (>= 10.0): 0.5mm minor, 2mm major, 2mm label
        assert_eq!(LaserBurnApp::grid_steps_for_zoom(12.0), (0.5, 2.0, 2.0));
    }

    #[test]
    fn test_async_file_dialog_actions() {
        let app = LaserBurnApp::default();
        let tmp_dir = std::env::temp_dir();
        let save_target = tmp_dir.join("test_save_project.lbrn2");

        // Send a SaveProject action through the channel
        app.file_dialog_tx
            .send(FileDialogAction::SaveProject(save_target.clone()))
            .unwrap();

        // Verify channel receives and handles SaveProject safely
        if let Ok(FileDialogAction::SaveProject(path)) = app.file_dialog_rx.try_recv() {
            let save_path = if path.extension().is_none() {
                path.with_extension("lbrn2")
            } else {
                path
            };
            assert!(Lbrn2File::save_to_file(&app.doc, &save_path).is_ok());
            assert!(save_path.exists());
            let _ = std::fs::remove_file(&save_path);
        } else {
            panic!("Expected SaveProject action from file_dialog_rx");
        }
    }

    #[test]
    fn test_alignment_wizard_targets_and_gcode() {
        let mut app = LaserBurnApp::default();
        app.doc.width = 465.0;
        app.doc.height = 440.0;
        app.camera_calibration.target_inset_mm = 50.0;

        let targets = app.alignment_target_doc_coords();
        assert_eq!(targets.len(), 4);
        assert_eq!(targets[0], (50.0, 50.0));
        assert_eq!(targets[1], (415.0, 50.0));
        assert_eq!(targets[2], (415.0, 390.0));
        assert_eq!(targets[3], (50.0, 390.0));

        let gcode = app.generate_alignment_targets_gcode();
        assert!(gcode.contains("G21"));
        assert!(gcode.contains("G90"));
        assert!(gcode.contains("M3"));
        assert!(gcode.contains("M5"));
        assert!(gcode.contains("G1")); // Crosshairs
        assert!(gcode.contains("G2")); // Bullseye circles

        // Test homography solving with these targets
        let pts = [
            crate::camera::AlignmentPoint { machine_x: targets[0].0, machine_y: targets[0].1, camera_u: 0.1, camera_v: 0.1 },
            crate::camera::AlignmentPoint { machine_x: targets[1].0, machine_y: targets[1].1, camera_u: 0.9, camera_v: 0.1 },
            crate::camera::AlignmentPoint { machine_x: targets[2].0, machine_y: targets[2].1, camera_u: 0.9, camera_v: 0.9 },
            crate::camera::AlignmentPoint { machine_x: targets[3].0, machine_y: targets[3].1, camera_u: 0.1, camera_v: 0.9 },
        ];
        assert!(app.camera_calibration.update_homography(&pts).is_ok());
        assert!(app.camera_calibration.use_homography);
        assert!(app.camera_calibration.inv_homography_matrix.is_some());
    }

    #[test]
    fn test_settings_profile_export_and_import() {
        let mut app = LaserBurnApp::default();
        app.device.settings.bed_width = 465.0;
        app.device.settings.bed_height = 440.0;
        app.device.settings.origin_corner = crate::device::OriginCorner::TopRight;
        app.device.settings.enable_limit_switches = true;
        app.device.settings.default_kerf = 0.125;

        // Custom layer name
        if let Some(l0) = app.doc.layers.get_mut(&0) {
            l0.name = "Custom Vaneri 4mm".to_string();
        }

        let profile = SettingsProfile {
            version: 1,
            device: app.device.settings.clone(),
            camera: Some(app.camera_calibration.clone()),
            layers: Some(app.doc.layers.clone()),
        };

        let json = serde_json::to_string_pretty(&profile).expect("Failed to serialize SettingsProfile");
        assert!(json.contains("Custom Vaneri 4mm"));
        assert!(json.contains("465.0"));
        assert!(json.contains("TopRight"));

        // Now create a fresh app with generic factory defaults
        let mut fresh_app = LaserBurnApp::default();
        assert_eq!(fresh_app.device.settings.bed_width, 400.0);
        assert!(!fresh_app.device.settings.enable_limit_switches);

        // Import the profile json
        let parsed_profile: SettingsProfile = serde_json::from_str(&json).expect("Failed to parse SettingsProfile");
        fresh_app.device.settings = parsed_profile.device;
        fresh_app.doc.width = fresh_app.device.settings.bed_width;
        fresh_app.doc.height = fresh_app.device.settings.bed_height;
        if let Some(layers) = parsed_profile.layers {
            fresh_app.doc.layers = layers;
        }

        // Verify restoration
        assert_eq!(fresh_app.device.settings.bed_width, 465.0);
        assert_eq!(fresh_app.device.settings.bed_height, 440.0);
        assert_eq!(fresh_app.device.settings.origin_corner, crate::device::OriginCorner::TopRight);
        assert!(fresh_app.device.settings.enable_limit_switches);
        assert_eq!(fresh_app.device.settings.default_kerf, 0.125);
        assert_eq!(fresh_app.doc.layers.get(&0).unwrap().name, "Custom Vaneri 4mm");
    }

    #[test]
    fn test_image_import_and_dither_raster_scanlines() {
        let mut app = LaserBurnApp::default();

        // Create a temporary 64x64 test image with a black circle on white background
        let mut img_buf = image::RgbaImage::new(64, 64);
        for (x, y, pixel) in img_buf.enumerate_pixels_mut() {
            let dx = x as f32 - 32.0;
            let dy = y as f32 - 32.0;
            if dx * dx + dy * dy <= 400.0 {
                *pixel = image::Rgba([0, 0, 0, 255]); // black dot (burn)
            } else {
                *pixel = image::Rgba([255, 255, 255, 255]); // white (no burn)
            }
        }
        let temp_path = std::env::temp_dir().join("laserburn_test_img.png");
        img_buf.save(&temp_path).expect("Failed to save test image");

        // Test import_image
        app.import_image(&temp_path).expect("import_image failed");
        assert_eq!(app.doc.shapes.len(), 1);

        // Verify shape is Shape::Image
        if let Shape::Image { layer_index, width, height, raw_pixels, raw_width, raw_height, .. } = &app.doc.shapes[0] {
            assert_eq!(*layer_index, 1);
            assert!(*width > 0.0);
            assert!(*height > 0.0);
            assert_eq!(*raw_width, 64);
            assert_eq!(*raw_height, 64);
            assert!(raw_pixels.is_some());
        } else {
            panic!("Expected Shape::Image");
        }

        // Verify layer 1 was configured as CutMode::Image with Atkinson dithering
        let layer1 = app.doc.layers.get(&1).expect("Layer 1 should exist");
        assert_eq!(layer1.cut_setting.mode, CutMode::Image);
        assert_eq!(layer1.cut_setting.dither_mode, DitherMode::Atkinson);

        // Test generating raster scanlines directly
        let scanlines = app.doc.shapes[0].to_raster_polylines(DitherMode::Atkinson, 0.5);
        assert!(!scanlines.is_empty(), "Should generate horizontal raster scanlines");
        println!("Generated {} raster scanlines for Atkinson dither", scanlines.len());

        // Test toolpath optimization for image
        let opt = CutOptimizer::new();
        let job = opt.optimize(&app.doc);
        assert!(!job.paths.is_empty(), "Optimized job should contain raster paths for the image");
        println!("OptimizedJob contains {} toolpath paths", job.paths.len());

        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_circle_non_uniform_scaling_becomes_oval() {
        let mut shape = Shape::Circle {
            layer_index: 0,
            center: Point2D::new(50.0, 50.0),
            radius: 20.0, // 40x40 mm
        };

        // Uniform scaling preserves Circle
        shape.scale(Point2D::new(50.0, 50.0), 2.0, 2.0);
        if let Shape::Circle { radius, .. } = &shape {
            assert_eq!(*radius, 40.0);
        } else {
            panic!("Expected uniform scale to preserve Circle");
        }

        // Non-uniform scaling converts Circle to an Ellipse/Oval Path
        shape.scale(Point2D::new(50.0, 50.0), 2.0, 0.5);
        if let Shape::Path { polyline, .. } = &shape {
            assert!(polyline.is_closed);
            assert_eq!(polyline.points.len(), 48);
            let b = polyline.bounds();
            assert!((b.width() - 160.0).abs() < 1.0, "Expected width ~160mm, got {}", b.width());
            assert!((b.height() - 40.0).abs() < 1.0, "Expected height ~40mm, got {}", b.height());
        } else {
            panic!("Expected non-uniform scale to convert Circle to Oval Path");
        }
    }

    #[test]
    fn test_image_scaling_and_job_estimate() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();

        let shape = Shape::Image {
            layer_index: 1,
            x: 20.0,
            y: 30.0,
            width: 100.0,
            height: 80.0,
            raw_pixels: None,
            raw_width: 800,
            raw_height: 640,
            image_path: None,
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
            sharpen: 0.0,
        };
        app.doc.add_shape(shape);
        app.selected_shape_indices = vec![0];

        // Scale image non-uniformly
        app.doc.shapes[0].scale(Point2D::new(20.0, 30.0), 1.5, 0.5);
        if let Shape::Image { width, height, .. } = &app.doc.shapes[0] {
            assert_eq!(*width, 150.0);
            assert_eq!(*height, 40.0);
        } else {
            panic!("Expected Shape::Image");
        }

        let est = app.get_job_estimate();
        assert!(est.total_cut_distance_mm > 0.0);
        assert!(est.total_estimated_time_sec > 0.0);
        assert!(!est.formatted_time.is_empty());
    }

    #[test]
    fn test_undo_redo_and_offset_and_material_test_and_camera_autofit() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();

        // 1. Test Undo / Redo
        app.push_undo();
        app.doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 30.0,
            corner_radius: 0.0,
        });
        assert_eq!(app.doc.shapes.len(), 1);
        app.undo();
        assert_eq!(app.doc.shapes.len(), 0);
        app.redo();
        assert_eq!(app.doc.shapes.len(), 1);

        // 2. Test Offset Shapes (Outward + Inward)
        app.selected_shape_indices = vec![0];
        app.offset_distance = 5.0;
        app.offset_inward = false; // Outward
        app.offset_both = false;
        app.offset_delete_orig = false;
        app.apply_offset_selected();
        assert_eq!(app.doc.shapes.len(), 2);
        let b = app.doc.shapes[1].bounds();
        assert!((b.width() - 50.0).abs() < 1e-4);
        assert!((b.height() - 40.0).abs() < 1e-4);

        // 3. Test Material Test Generator
        app.doc.shapes.clear();
        app.mat_test_rows = 3;
        app.mat_test_cols = 4;
        app.generate_material_test_grid();
        assert!(app.doc.shapes.len() > 12, "Material test grid should create cells and text labels");

        // 4. Test Image Preprocess & Ceramic Preview
        let gray = vec![0.0f32, 0.25, 0.5, 0.75, 1.0, 0.5, 0.25, 0.75, 0.5];
        let processed = crate::dither::preprocess_grayscale(&gray, 3, 3, 10.0, 20.0, 1.2, 0.5);
        assert_eq!(processed.len(), 9);
        let raw_rgba = vec![128u8; 3 * 3 * 4];
        let bands = crate::document::default_ceramic_bands();
        let rgba = crate::dither::render_processed_preview_rgba(
            &raw_rgba,
            3,
            3,
            0.0,
            0.0,
            1.0,
            0.0,
            false,
            true,
            10.0,
            90.0,
            &bands,
        );
        assert_eq!(rgba.len(), 3 * 3 * 4);

        // 5. Test Camera Workpiece Detection & Auto-Fit
        let mut frame = crate::camera::CameraFrame {
            width: 320,
            height: 240,
            rgba: vec![25u8; 320 * 240 * 4],
            frame_number: 1,
        };
        // Paint dark background and bright centered tile (x: 100..220, y: 60..180)
        for y in 0..240 {
            for x in 0..320 {
                let idx = ((y * 320 + x) * 4) as usize;
                let in_tile = x >= 100 && x <= 220 && y >= 60 && y <= 180;
                let c = if in_tile { 230 } else { 25 };
                frame.rgba[idx] = c;
                frame.rgba[idx + 1] = c;
                frame.rgba[idx + 2] = c;
                frame.rgba[idx + 3] = 255;
            }
        }
        let cal = crate::camera::CameraCalibration::default();
        let wp = crate::camera::detect_workpiece_on_bed(&frame, &cal, 400.0, 400.0)
            .expect("Should detect bright ceramic tile on dark bed");
        assert!(wp.width > 50.0 && wp.height > 50.0);
    }

    #[test]
    fn test_material_library_gui_apply_and_save() {
        let mut app = LaserBurnApp::default();
        assert!(!app.material_lib.materials.is_empty(), "Material library should load factory presets");

        // 1. Verify and apply factory preset to layer 0
        let birch = app.material_lib.find_material("Birch Plywood (Koivuvaneri)").expect("Must have Birch Plywood");
        let thick_3 = birch.find_thickness(3.0).expect("Must have 3.0mm thickness");
        let clean_cut = thick_3.find_operation("Clean Cut").expect("Must have Clean Cut operation");

        if let Some(layer) = app.doc.layers.get_mut(&0) {
            clean_cut.apply_to_layer(layer);
            assert_eq!(layer.cut_setting.speed, 3.0);
            assert_eq!(layer.cut_setting.power_max, 100.0);
            assert_eq!(layer.cut_setting.num_passes, 2);
            assert_eq!(layer.cut_setting.air_assist, true);
        } else {
            panic!("Layer 0 should exist");
        }

        // 2. Modify layer 0 and save as new preset via dialog state
        if let Some(layer) = app.doc.layers.get_mut(&0) {
            layer.cut_setting.speed = 18.5;
            layer.cut_setting.power_max = 75.0;
            layer.name = "Custom Foam".to_string();
        }

        app.open_save_layer_dialog();
        assert!(app.show_save_material_dialog);
        assert_eq!(app.save_mat_name, "Custom Foam");

        app.save_mat_category = "Foam & Sponge".to_string();
        app.save_mat_thickness_mm = 10.0;
        app.save_mat_thickness_label = "10.0 mm High Density".to_string();
        app.save_mat_op_name = "Fast Cut".to_string();
        app.save_mat_notes = "High air assist required to prevent flame".to_string();

        let layer = app.doc.layers.get(&0).unwrap();
        let op = crate::material::MaterialOperation::from_layer(&app.save_mat_op_name, layer, &app.save_mat_notes);
        app.material_lib.add_or_update_operation(
            &app.save_mat_category,
            &app.save_mat_name,
            app.save_mat_thickness_mm,
            &app.save_mat_thickness_label,
            op,
        );

        // Verify newly added preset in library
        let foam = app.material_lib.find_material("Custom Foam").expect("Custom Foam should exist in library");
        assert_eq!(foam.category, "Foam & Sponge");
        let thick_10 = foam.find_thickness(10.0).expect("10mm should exist");
        let fast_cut = thick_10.find_operation("Fast Cut").expect("Fast Cut operation should exist");
        assert_eq!(fast_cut.cut_setting.speed, 18.5);
        assert_eq!(fast_cut.cut_setting.power_max, 75.0);
    }

    #[test]
    fn test_material_library_file_dialog_actions() {
        let mut app = LaserBurnApp::default();
        let tmp_dir = std::env::temp_dir();
        let export_target = tmp_dir.join("test_export_materials.json");

        // Send export action
        app.file_dialog_tx
            .send(FileDialogAction::ExportMaterialLibrary(export_target.clone()))
            .unwrap();

        if let Ok(FileDialogAction::ExportMaterialLibrary(path)) = app.file_dialog_rx.try_recv() {
            assert!(app.material_lib.save_to_path(&path).is_ok());
            assert!(path.exists());

            // Modify library in memory
            app.material_lib.materials.clear();
            assert_eq!(app.material_lib.materials.len(), 0);

            // Send import action
            app.file_dialog_tx
                .send(FileDialogAction::ImportMaterialLibrary(path.clone()))
                .unwrap();

            if let Ok(FileDialogAction::ImportMaterialLibrary(import_path)) = app.file_dialog_rx.try_recv() {
                let imported = crate::material::MaterialLibrary::load_from_any_path(&import_path).unwrap();
                app.material_lib.merge(imported);
                assert!(!app.material_lib.materials.is_empty(), "Library should be restored from imported JSON");
            }

            let _ = std::fs::remove_file(&path);
        } else {
            panic!("Expected ExportMaterialLibrary action from rx");
        }
    }

    #[test]
    fn test_gui_grid_and_radial_array() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        app.doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            corner_radius: 0.0,
        });
        app.selected_shape_indices = vec![0];

        // 3x2 Grid Array with 5mm spacing
        app.grid_columns = 3;
        app.grid_rows = 2;
        app.grid_spacing_x = 5.0;
        app.grid_spacing_y = 5.0;
        app.grid_use_spacing = true;
        app.apply_grid_array();

        assert_eq!(app.doc.shapes.len(), 6);
        assert_eq!(app.selected_shape_indices.len(), 6);

        // Test undo
        app.undo();
        assert_eq!(app.doc.shapes.len(), 1);

        // Radial Array: 4 copies around (50, 50)
        app.selected_shape_indices = vec![0];
        app.radial_count = 4;
        app.radial_total_angle = 360.0;
        app.radial_center_auto = false;
        app.radial_center_x = 50.0;
        app.radial_center_y = 50.0;
        app.radial_rotate_copies = false;
        app.apply_radial_array();

        assert_eq!(app.doc.shapes.len(), 4);
        assert_eq!(app.selected_shape_indices.len(), 4);
    }

    #[test]
    fn test_gui_machine_profiles_switch_and_create() {
        let mut app = LaserBurnApp::default();

        // Switch to Compact Diode (200x200)
        assert!(app.device.settings.apply_profile("Compact Diode (200x200)"));
        app.doc.width = app.device.settings.bed_width;
        app.doc.height = app.device.settings.bed_height;
        assert_eq!(app.doc.width, 200.0);
        assert_eq!(app.doc.height, 200.0);

        // Save current as new profile "My Custom 50W"
        app.device.settings.bed_width = 500.0;
        app.device.settings.bed_height = 300.0;
        app.device.settings.save_current_as_profile("My Custom 50W");

        assert_eq!(app.device.settings.current_profile_name, "My Custom 50W");
        let prof = app.device.settings.profiles.iter().find(|p| p.name == "My Custom 50W").unwrap();
        assert_eq!(prof.bed_width, 500.0);
        assert_eq!(prof.bed_height, 300.0);

        // Switch back to Large Format / CO2 (600x400)
        assert!(app.device.settings.apply_profile("Large Format / CO2 (600x400)"));
        assert_eq!(app.device.settings.bed_width, 600.0);
    }

    #[test]
    fn test_gui_text_on_path() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        // Add a circle as the path
        app.doc.add_shape(Shape::Circle {
            layer_index: 0,
            center: Point2D::new(100.0, 100.0),
            radius: 40.0,
        });
        app.selected_shape_indices = vec![0];

        // Configure Text on Path
        app.text_input = "LASER".to_string();
        app.text_is_singleline = true;
        app.text_font_size = 12.0;
        app.text_on_path_enabled = true;
        app.text_on_path_offset = 0.0;
        app.text_on_path_perpendicular = 2.0;
        app.text_on_path_reverse = false;

        app.add_text_to_document();

        // Must now have the original circle plus the new text group
        assert_eq!(app.doc.shapes.len(), 2);
        if let Shape::Group { shapes, .. } = &app.doc.shapes[1] {
            assert!(!shapes.is_empty(), "Text on path should produce non-empty glyph shapes");
        } else {
            panic!("Expected newly added text to be a Group");
        }
    }

    #[test]
    fn test_gui_image_trace() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        // Create an image with a dark circle
        let w = 40;
        let h = 40;
        let mut pixels = vec![255u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let dx = x as f64 - 20.0;
                let dy = y as f64 - 20.0;
                let off = (y * w + x) * 4;
                if dx * dx + dy * dy <= 100.0 {
                    pixels[off] = 0; // Black circle
                    pixels[off + 1] = 0;
                    pixels[off + 2] = 0;
                    pixels[off + 3] = 255;
                }
            }
        }

        app.doc.add_shape(Shape::Image {
            layer_index: 1,
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 50.0,
            image_path: None,
            raw_pixels: Some(pixels),
            raw_width: w as u32,
            raw_height: h as u32,
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
            sharpen: 0.0,
        });
        app.selected_shape_indices = vec![0];

        // Test opening trace dialog & live preview
        app.open_trace_dialog();
        assert!(app.show_trace_window);
        assert!(!app.trace_preview_polylines.is_empty(), "Live preview should have contours immediately");

        // Adjust threshold and verify live update
        app.trace_threshold = 200;
        app.update_trace_preview();
        assert!(!app.trace_preview_polylines.is_empty());

        // Test Centerline tracing mode
        app.trace_mode = crate::trace::TraceMode::Centerline;
        app.update_trace_preview();
        assert!(!app.trace_preview_polylines.is_empty(), "Centerline trace should extract paths");

        app.trace_mode = crate::trace::TraceMode::Outline;
        app.trace_threshold = 128;
        app.trace_invert = false;
        app.trace_blur_radius = 0;
        app.trace_delete_image = true; // Replace image with vectors
        app.update_trace_preview();
        app.apply_trace_image();

        assert_eq!(app.doc.shapes.len(), 1);
        if let Shape::Group { shapes, .. } = &app.doc.shapes[0] {
            assert!(!shapes.is_empty(), "Trace should produce vector paths");
        } else {
            panic!("Expected traced vectors to replace image as a Group");
        }
    }

    #[test]
    fn test_gui_load_sample_files() {
        let mut app = LaserBurnApp::default();

        // Test loading sample DXF bracket
        app.load_sample_dxf();
        assert!(app.doc.shapes.len() >= 10, "Sample DXF bracket has multiple shapes");
        assert!(app.doc.layers.len() >= 3, "Sample DXF has CUT, HOLES, ENGRAVE layers");

        // Test loading sample PDF test card
        app.load_sample_pdf();
        assert!(app.doc.shapes.len() >= 6, "Sample PDF test card has multiple shapes");
        assert!(app.doc.layers.len() >= 2, "Sample PDF has multiple layers");
    }

    #[test]
    fn test_gui_open_dxf_and_pdf() {
        let mut app = LaserBurnApp::default();
        let tmp = std::env::temp_dir();

        // 1. Test opening DXF
        let dxf_path = tmp.join("test_gui_dxf.dxf");
        let dxf_content = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0.0\n20\n0.0\n11\n50.0\n21\n50.0\n0\nENDSEC\n0\nEOF";
        std::fs::write(&dxf_path, dxf_content).unwrap();
        assert!(app.open_file(&dxf_path).is_ok());
        assert_eq!(app.doc.shapes.len(), 1);
        let _ = std::fs::remove_file(&dxf_path);

        // 2. Test opening PDF
        let pdf_path = tmp.join("test_gui_pdf.pdf");
        let pdf_content = b"%PDF-1.4\n1 0 obj\n<< /Length 28 >>\nstream\n0 0 m 100 100 l S\nendstream\nendobj\n%%EOF";
        std::fs::write(&pdf_path, pdf_content).unwrap();
        assert!(app.open_file(&pdf_path).is_ok());
        assert_eq!(app.doc.shapes.len(), 1);
        let _ = std::fs::remove_file(&pdf_path);
    }

    #[test]
    fn test_gui_import_vector_file_merging() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();

        // Start with a circle in the document
        app.doc.add_shape(Shape::Circle { layer_index: 0, center: Point2D::new(50.0, 50.0), radius: 20.0 });
        assert_eq!(app.doc.shapes.len(), 1);

        let tmp = std::env::temp_dir();
        let dxf_path = tmp.join("test_merge_dxf.dxf");
        let dxf_content = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0.0\n20\n0.0\n11\n50.0\n21\n50.0\n0\nENDSEC\n0\nEOF";
        std::fs::write(&dxf_path, dxf_content).unwrap();

        assert!(app.import_vector_file(&dxf_path).is_ok());
        // Document should now contain both the original circle and the imported line!
        assert_eq!(app.doc.shapes.len(), 2);
        let _ = std::fs::remove_file(&dxf_path);
    }

    #[test]
    fn test_gui_node_editing() {
        let mut app = LaserBurnApp::default();
        app.doc.shapes.clear();
        app.selected_shape_indices.clear();

        // Add a triangle path with 3 points
        let tri = Polyline::new(
            vec![
                Point2D::new(0.0, 0.0),
                Point2D::new(40.0, 0.0),
                Point2D::new(20.0, 30.0),
            ],
            true,
        );
        app.doc.add_shape(Shape::Path { layer_index: 0, polyline: tri });
        app.selected_shape_indices = vec![0];
        app.active_tool = ActiveTool::NodeEdit;

        // 1. Move vertex #1 (at 40, 0) to (50, 5)
        app.selected_node_index = Some(1);
        if let Shape::Path { ref mut polyline, .. } = app.doc.shapes[0] {
            polyline.points[1] = Point2D::new(50.0, 5.0);
        }
        if let Shape::Path { ref polyline, .. } = app.doc.shapes[0] {
            assert_eq!(polyline.points[1], Point2D::new(50.0, 5.0));
        }

        // 2. Insert node at midpoint of edge between (0, 0) and (50, 5): approx (25, 2.5)
        assert!(app.insert_node_at_point(Point2D::new(25.0, 2.5)));
        if let Shape::Path { ref polyline, .. } = app.doc.shapes[0] {
            assert_eq!(polyline.points.len(), 4, "Must now have 4 vertices after node insertion");
        }

        // 3. Delete the newly inserted node
        app.selected_node_index = Some(1);
        app.delete_selected_node();
        if let Shape::Path { ref polyline, .. } = app.doc.shapes[0] {
            assert_eq!(polyline.points.len(), 3, "Must return to 3 vertices after deleting node");
        }

        // 4. Toggle closed/open
        app.toggle_selected_path_closed();
        if let Shape::Path { ref polyline, .. } = app.doc.shapes[0] {
            assert!(!polyline.is_closed, "Must be open after toggling");
        }
        app.toggle_selected_path_closed();
        if let Shape::Path { ref polyline, .. } = app.doc.shapes[0] {
            assert!(polyline.is_closed, "Must be closed after toggling again");
        }
    }

    #[test]
    fn test_gui_language_switching() {
        let mut app = LaserBurnApp::default();
        // Default is English
        assert_eq!(app.device.settings.language, crate::i18n::Language::English);
        assert_eq!(app.tr("File"), "File");
        assert_eq!(app.tr("Edit"), "Edit");
        assert_eq!(app.tr("Select / Move"), "Select / Move");
        assert_eq!(app.tr("Transform & Size"), "Transform & Size");
        assert_eq!(app.tr("Job Execution (G-Code)"), "Job Execution (G-Code)");

        // Switch to Finnish
        app.device.settings.language = crate::i18n::Language::Finnish;
        assert_eq!(app.tr("File"), "Tiedosto");
        assert_eq!(app.tr("Edit"), "Muokkaa");
        assert_eq!(app.tr("Select / Move"), "Valitse / Siirrä");
        assert_eq!(app.tr("Transform & Size"), "Muunnos & Koko");
        assert_eq!(app.tr("Job Execution (G-Code)"), "Työn ajo (G-koodi)");
    }

    #[test]
    fn test_align_and_distribute() {
        let mut app = LaserBurnApp::default();
        app.doc.width = 400.0;
        app.doc.height = 400.0;
        app.doc.shapes.clear();

        // Add 3 rectangles
        app.doc.add_shape(Shape::Rect { layer_index: 0, x: 10.0, y: 10.0, width: 20.0, height: 20.0, corner_radius: 0.0 });
        app.doc.add_shape(Shape::Rect { layer_index: 0, x: 50.0, y: 40.0, width: 20.0, height: 20.0, corner_radius: 0.0 });
        app.doc.add_shape(Shape::Rect { layer_index: 0, x: 120.0, y: 80.0, width: 20.0, height: 20.0, corner_radius: 0.0 });

        app.selected_shape_indices = vec![0, 1, 2];

        // 1. Center to bed
        app.align_selection(AlignMode::CenterToBed);
        let b = app.selection_bounds().expect("bounds");
        let cx = (b.min_x + b.max_x) / 2.0;
        let cy = (b.min_y + b.max_y) / 2.0;
        assert!((cx - 200.0).abs() < 1e-4, "Group center X should be 200 (bed center)");
        assert!((cy - 200.0).abs() < 1e-4, "Group center Y should be 200 (bed center)");

        // 2. Align Left
        app.align_selection(AlignMode::Left);
        let b_left = app.selection_bounds().expect("bounds");
        for &idx in &app.selected_shape_indices {
            let sb = app.doc.shapes[idx].bounds();
            assert!((sb.min_x - b_left.min_x).abs() < 1e-4, "Each shape min_x must match group min_x");
        }

        // 3. Align Top
        app.align_selection(AlignMode::Top);
        let b_top = app.selection_bounds().expect("bounds");
        for &idx in &app.selected_shape_indices {
            let sb = app.doc.shapes[idx].bounds();
            assert!((sb.min_y - b_top.min_y).abs() < 1e-4, "Each shape min_y must match group min_y");
        }

        // 4. Distribute Horizontally
        // Reset X coordinates to test distribution
        if let Shape::Rect { ref mut x, .. } = app.doc.shapes[0] { *x = 10.0; }
        if let Shape::Rect { ref mut x, .. } = app.doc.shapes[1] { *x = 30.0; } // uneven
        if let Shape::Rect { ref mut x, .. } = app.doc.shapes[2] { *x = 90.0; }
        app.distribute_selection(DistributeMode::Horizontal);

        let c0 = (app.doc.shapes[0].bounds().min_x + app.doc.shapes[0].bounds().max_x) / 2.0;
        let c1 = (app.doc.shapes[1].bounds().min_x + app.doc.shapes[1].bounds().max_x) / 2.0;
        let c2 = (app.doc.shapes[2].bounds().min_x + app.doc.shapes[2].bounds().max_x) / 2.0;
        let d1 = c1 - c0;
        let d2 = c2 - c1;
        assert!((d1 - d2).abs() < 1e-4, "Centers must be equally spaced after distribute: d1={}, d2={}", d1, d2);
    }

    #[test]
    fn test_preview_job_simulation_data() {
        let mut doc = Document::new("Preview Test", 300.0, 300.0);
        doc.add_shape(Shape::Rect { layer_index: 0, x: 20.0, y: 20.0, width: 40.0, height: 40.0, corner_radius: 0.0 });
        doc.add_shape(Shape::Circle { layer_index: 0, center: Point2D::new(100.0, 100.0), radius: 15.0 });

        let job_data = PreviewJobData::build(&doc, 0.0);
        assert!(!job_data.moves.is_empty(), "Moves must not be empty");
        assert!(job_data.total_time_sec > 0.0, "Total time must be greater than zero");
        assert!(job_data.total_cut_distance > 0.0, "Total cut distance must be positive");
        assert!(job_data.total_rapid_distance > 0.0, "Total rapid distance must be positive");

        let cut_moves = job_data.moves.iter().filter(|m| m.is_cut).count();
        let rapid_moves = job_data.moves.iter().filter(|m| !m.is_cut).count();
        assert!(cut_moves >= 2, "Must contain cut segments for both shapes");
        assert!(rapid_moves >= 1, "Must contain rapid travel segments");
    }

    #[test]
    fn test_auto_nesting_gui() {
        let mut app = LaserBurnApp::default();
        app.doc.width = 400.0;
        app.doc.height = 400.0;
        app.doc.shapes.clear();

        for i in 0..6 {
            app.doc.add_shape(Shape::Rect {
                layer_index: 0,
                x: 10.0 + (i as f64) * 35.0,
                y: 10.0 + (i as f64) * 35.0,
                width: 30.0,
                height: 25.0,
                corner_radius: 0.0,
            });
        }

        let indices: Vec<usize> = (0..app.doc.shapes.len()).collect();
        let res = crate::nesting::run_nesting(&app.doc.shapes, &indices, &app.nesting_options);
        assert_eq!(res.placed_count, 6);
        assert_eq!(res.unplaced_count, 0);

        crate::nesting::apply_nesting_result(&mut app.doc, &res);
        assert_eq!(app.doc.shapes.len(), 6);
    }

    #[test]
    fn test_floating_panels_defaults_and_toggle() {
        let mut app = LaserBurnApp::default();
        // Properties & Layers default is open
        assert!(app.show_properties_window);
        // Objects list and Job control default to closed until requested
        assert!(!app.show_objects_window);
        assert!(!app.show_job_window);

        // Starting a cut job opens Job Control HUD automatically
        app.show_job_window = false;
        app.doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
            corner_radius: 0.0,
        });
        // Mock connection
        app.device.status = crate::device::ConnectionStatus::Connected;
        app.start_job();
        assert!(app.show_job_window, "Starting a job must automatically open the Job Control window");
    }

    #[test]
    fn test_generators_integration() {
        let app = LaserBurnApp::default();
        assert!(!app.show_box_maker_window);
        assert!(!app.show_living_hinge_window);
        assert!(!app.show_qr_code_window);
        assert!(!app.show_variable_text_window);

        // Test QR code generation
        let qr_shapes = crate::generator::generate_qr_shapes(&app.qr_code_params, 0).expect("QR gen");
        assert!(!qr_shapes.is_empty());

        // Test Living hinge generation
        let hinge_cuts = crate::generator::generate_living_hinge(&app.living_hinge_params);
        assert!(!hinge_cuts.is_empty());

        // Test Box maker generation
        let box_parts = crate::generator::generate_box_parts(&app.box_maker_params);
        assert_eq!(box_parts.len(), 6);
        let box_shapes = crate::generator::layout_box_shapes(&box_parts, 10.0, 10.0, 5.0, 0);
        assert_eq!(box_shapes.len(), 6);

        // Test Variable text generation
        let var_shapes = crate::generator::generate_variable_text_shapes(&app.variable_text_params, 0, 1);
        assert_eq!(var_shapes.len(), 10);
    }

    #[test]
    fn test_open_3d_mesh_relief() {
        let ascii_stl = r#"solid coin
  facet normal 0 0 1
    outer loop
      vertex 0.0 0.0 2.0
      vertex 30.0 0.0 2.0
      vertex 15.0 30.0 3.0
    endloop
  endfacet
endsolid coin
"#;
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_coin.stl");
        std::fs::write(&file_path, ascii_stl).unwrap();

        let mut app = LaserBurnApp::default();
        let res = app.open_file(&file_path);
        assert!(res.is_ok(), "Failed to open 3D STL file: {:?}", res);
        assert_eq!(app.doc.shapes.len(), 1);

        // Verify it was imported as an Image shape with Sliced3D relief layer
        if let Shape::Image { layer_index, raw_pixels, width, height, .. } = &app.doc.shapes[0] {
            assert_eq!(*layer_index, 1);
            assert!(raw_pixels.is_some());
            assert!(*width > 0.0);
            assert!(*height > 0.0);
            let layer = app.doc.layers.get(layer_index).expect("Layer 1 must exist");
            assert_eq!(layer.cut_setting.dither_mode, DitherMode::Sliced3D);
            assert_eq!(layer.cut_setting.relief_slices, 16);
        } else {
            panic!("Expected Shape::Image for 3D mesh import");
        }

        let _ = std::fs::remove_file(file_path);
    }

    #[test]
    fn test_engraving_depth_test_multipass() {
        let mut app = LaserBurnApp::default();
        app.depth_test_type = DepthTestType::MultiPass;
        app.depth_test_steps = 6;
        app.depth_test_speed = 120.0;
        app.depth_test_power = 65.0;
        app.depth_test_interval = 0.08;
        app.depth_test_cross_hatch = true;
        app.depth_test_cut_outline = true;

        app.generate_depth_test_grid();

        assert!(!app.doc.shapes.is_empty(), "Must generate shapes for depth test");
        assert!(!app.selected_shape_indices.is_empty(), "Generated shapes must be selected for positioning");

        // Verify layers 1..6 have passes 1..6
        for p in 1..=6 {
            let l = app.doc.layers.get(&p).expect("Layer for pass must exist");
            assert_eq!(l.cut_setting.num_passes, p as u32);
            assert_eq!(l.cut_setting.power_max, 65.0);
            assert_eq!(l.cut_setting.speed, 120.0);
            assert_eq!(l.cut_setting.mode, CutMode::Fill);
            assert!(l.cut_setting.cross_hatch);
        }
    }

    #[test]
    fn test_engraving_depth_test_power_steps() {
        let mut app = LaserBurnApp::default();
        app.depth_test_type = DepthTestType::PowerSteps;
        app.depth_test_steps = 5;
        app.depth_test_min_power = 20.0;
        app.depth_test_max_power = 80.0;

        app.generate_depth_test_grid();

        assert!(!app.doc.shapes.is_empty());
        let l1 = app.doc.layers.get(&1).expect("Layer 1 must exist");
        let l5 = app.doc.layers.get(&5).expect("Layer 5 must exist");
        assert_eq!(l1.cut_setting.power_max, 20.0);
        assert_eq!(l5.cut_setting.power_max, 80.0);
        assert_eq!(l1.cut_setting.mode, CutMode::Fill);
    }

    #[test]
    fn test_engraving_depth_test_line_interval() {
        let mut app = LaserBurnApp::default();
        app.depth_test_type = DepthTestType::LineInterval;
        app.generate_depth_test_grid();

        assert!(!app.doc.shapes.is_empty());
        let l1 = app.doc.layers.get(&1).expect("Layer 1 must exist");
        assert_eq!(l1.cut_setting.line_interval, 0.05);
        assert_eq!(l1.cut_setting.mode, CutMode::Fill);
    }

    #[test]
    fn test_ui_layout_persistence_and_window_geometry() {
        let mut app = LaserBurnApp::default();
        app.show_objects_window = true;
        app.show_material_library = true;
        app.zoom = 3.2;
        app.pan = egui::Vec2::new(310.0, 150.0);
        app.jog_step = 50.0;
        app.jog_speed = 6000.0;

        app.record_window_rect(
            "floating_properties_window",
            egui::Rect::from_min_size(egui::pos2(500.0, 120.0), egui::vec2(380.0, 600.0)),
        );

        let layout = app.capture_ui_layout();
        assert_eq!(layout.show_objects_window, true);
        assert_eq!(layout.show_material_library, true);
        assert_eq!(layout.show_camera_window, false);
        assert!((layout.zoom - 3.2).abs() < 1e-4);
        assert!((layout.pan_x - 310.0).abs() < 1e-4);
        assert!((layout.pan_y - 150.0).abs() < 1e-4);
        assert!((layout.jog_step - 50.0).abs() < 1e-4);
        assert!((layout.jog_speed - 6000.0).abs() < 1e-4);

        let (pos, size) = app.window_pos_size("floating_properties_window", [0.0, 0.0], [100.0, 100.0]);
        assert_eq!(pos, [500.0, 120.0]);
        assert_eq!(size, [380.0, 600.0]);

        // Create new app instance and restore
        let mut new_app = LaserBurnApp::default();
        new_app.apply_ui_layout(layout);
        assert_eq!(new_app.show_objects_window, true);
        assert_eq!(new_app.show_material_library, true);
        assert!((new_app.zoom - 3.2).abs() < 1e-4);
        assert!((new_app.pan.x - 310.0).abs() < 1e-4);
        let (restored_pos, restored_size) = new_app.window_pos_size("floating_properties_window", [0.0, 0.0], [100.0, 100.0]);
        assert_eq!(restored_pos, [500.0, 120.0]);
        assert_eq!(restored_size, [380.0, 600.0]);
    }
}

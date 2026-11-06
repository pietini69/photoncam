use std::collections::HashMap;
use std::path::PathBuf;
use super::LengthUnit;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WindowRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UiLayoutSettings {
    // Open/Closed Window States
    pub show_properties_window: bool,
    pub show_objects_window: bool,
    pub show_job_window: bool,
    pub show_connection_window: bool,
    pub show_settings_window: bool,
    pub show_move_window: bool,
    pub show_camera_window: bool,
    pub show_traffic_panel: bool,
    pub show_tools_palette: bool,
    pub detach_tools_window: bool,
    pub show_preview_window: bool,
    pub show_material_library: bool,
    pub show_nesting_window: bool,
    pub show_box_maker_window: bool,
    pub show_living_hinge_window: bool,
    pub show_qr_code_window: bool,
    pub show_variable_text_window: bool,
    pub show_trace_window: bool,
    pub show_material_test_window: bool,
    pub show_depth_test_window: bool,
    pub show_grid_array_window: bool,
    pub show_radial_array_window: bool,
    pub show_offset_window: bool,
    pub show_text_dialog: bool,
    pub show_device_profiles_window: bool,
    pub show_web_remote_window: bool,

    // Window Positions & Sizes
    pub window_rects: HashMap<String, WindowRect>,

    // General UI Preferences & Canvas
    pub length_unit: LengthUnit,
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub jog_step: f64,
    pub jog_speed: f64,
    pub hide_ok_in_traffic_log: bool,
    pub explain_traffic_codes: bool,

    // Main window viewport
    pub main_window_width: Option<f32>,
    pub main_window_height: Option<f32>,
}

impl Default for UiLayoutSettings {
    fn default() -> Self {
        Self {
            show_properties_window: true,
            show_objects_window: false,
            show_job_window: false,
            show_connection_window: false,
            show_settings_window: false,
            show_move_window: false,
            show_camera_window: false,
            show_traffic_panel: true,
            show_tools_palette: true,
            detach_tools_window: false,
            show_preview_window: false,
            show_material_library: false,
            show_nesting_window: false,
            show_box_maker_window: false,
            show_living_hinge_window: false,
            show_qr_code_window: false,
            show_variable_text_window: false,
            show_trace_window: false,
            show_material_test_window: false,
            show_depth_test_window: false,
            show_grid_array_window: false,
            show_radial_array_window: false,
            show_offset_window: false,
            show_text_dialog: false,
            show_device_profiles_window: false,
            show_web_remote_window: false,

            window_rects: HashMap::new(),

            length_unit: LengthUnit::Millimeters,
            zoom: 1.6,
            pan_x: 120.0,
            pan_y: 80.0,
            jog_step: 10.0,
            jog_speed: 3000.0,
            hide_ok_in_traffic_log: false,
            explain_traffic_codes: true,

            main_window_width: Some(1880.0),
            main_window_height: Some(1050.0),
        }
    }
}

impl UiLayoutSettings {
    pub fn config_path() -> Option<PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()?;
        let dir = PathBuf::from(home).join(".config").join("laser_burn");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join("ui_layout.json"))
    }

    pub fn load_or_default() -> Self {
        if cfg!(test) {
            return Self::default();
        }
        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(layout) = serde_json::from_str::<UiLayoutSettings>(&content) {
                        return layout;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save_to_disk(&self) -> Result<(), String> {
        if cfg!(test) {
            return Ok(());
        }
        if let Some(path) = Self::config_path() {
            let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
            std::fs::write(&path, json).map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err("Could not locate home directory to save UI layout".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_layout_serde_roundtrip() {
        let mut layout = UiLayoutSettings::default();
        layout.show_objects_window = true;
        layout.show_material_library = true;
        layout.zoom = 2.75;
        layout.pan_x = 240.0;
        layout.pan_y = 180.0;
        layout.jog_step = 25.0;
        layout.jog_speed = 4500.0;
        layout.main_window_width = Some(1920.0);
        layout.main_window_height = Some(1080.0);

        layout.window_rects.insert(
            "floating_properties_window".to_string(),
            WindowRect {
                x: 1400.0,
                y: 80.0,
                width: 380.0,
                height: 720.0,
            },
        );
        layout.window_rects.insert(
            "material_library_dialog".to_string(),
            WindowRect {
                x: 200.0,
                y: 150.0,
                width: 820.0,
                height: 580.0,
            },
        );

        let json = serde_json::to_string_pretty(&layout).expect("Serialize UI layout failed");
        let decoded: UiLayoutSettings = serde_json::from_str(&json).expect("Deserialize UI layout failed");

        assert_eq!(decoded.show_objects_window, true);
        assert_eq!(decoded.show_material_library, true);
        assert_eq!(decoded.show_camera_window, false);
        assert!((decoded.zoom - 2.75).abs() < 1e-4);
        assert!((decoded.pan_x - 240.0).abs() < 1e-4);
        assert!((decoded.jog_step - 25.0).abs() < 1e-4);
        assert_eq!(decoded.window_rects.len(), 2);

        let prop_rect = decoded.window_rects.get("floating_properties_window").unwrap();
        assert!((prop_rect.x - 1400.0).abs() < 1e-4);
        assert!((prop_rect.width - 380.0).abs() < 1e-4);

        let mat_rect = decoded.window_rects.get("material_library_dialog").unwrap();
        assert!((mat_rect.x - 200.0).abs() < 1e-4);
        assert!((mat_rect.height - 580.0).abs() < 1e-4);
    }
}

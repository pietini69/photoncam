//! Device communication, controller driver, and laser motion interface for LaserBurn.
//! Supports GRBL (USB/Serial), Smoothieware, and virtual/offline controller modes.

use crate::geometry::Point2D;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Physical link to the controller: a local USB/serial port or a network (TCP) link
/// to a PhotonCAM Bridge running on a Raspberry Pi (`tcp://host:2323`).
enum LinkPort {
    Serial(Box<dyn serialport::SerialPort>),
    Tcp(std::net::TcpStream),
}

/// Default TCP port of the PhotonCAM Bridge raw serial link.
pub const DEFAULT_BRIDGE_TCP_PORT: u16 = 2323;

/// Returns `Some("host:port")` when the port name is a network address (`tcp://host[:port]`).
pub fn parse_network_port(name: &str) -> Option<String> {
    let rest = name.trim().strip_prefix("tcp://")?;
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        return None;
    }
    if rest.contains(':') { Some(rest.to_string()) } else { Some(format!("{}:{}", rest, DEFAULT_BRIDGE_TCP_PORT)) }
}

impl std::io::Write for LinkPort {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            LinkPort::Serial(p) => p.write(buf),
            LinkPort::Tcp(t) => t.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            LinkPort::Serial(p) => p.flush(),
            LinkPort::Tcp(t) => t.flush(),
        }
    }
}

impl LinkPort {
    /// Non-blocking read of whatever bytes are currently available.
    /// `Err(())` means the link was closed by the other side.
    fn read_available(&mut self) -> Result<Vec<u8>, ()> {
        use std::io::Read;
        match self {
            LinkPort::Serial(p) => {
                if let Ok(available) = p.bytes_to_read() {
                    if available > 0 {
                        let mut buf = vec![0u8; available as usize];
                        if let Ok(n) = p.read(&mut buf) {
                            buf.truncate(n);
                            return Ok(buf);
                        }
                    }
                }
                Ok(Vec::new())
            }
            LinkPort::Tcp(t) => {
                let _ = t.set_nonblocking(true);
                let mut buf = [0u8; 4096];
                let res = match t.read(&mut buf) {
                    Ok(0) => Err(()),
                    Ok(n) => Ok(buf[..n].to_vec()),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::Interrupted => Ok(Vec::new()),
                    Err(_) => Err(()),
                };
                let _ = t.set_nonblocking(false);
                res
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerType {
    Grbl,
    Smoothieware,
    RuidaUdp,
    VirtualDemo,
}

impl ControllerType {
    pub fn name(&self) -> &'static str {
        match self {
            ControllerType::Grbl => "GRBL 1.1f / 1.1h (USB/Serial)",
            ControllerType::Smoothieware => "Smoothieware (USB/Serial)",
            ControllerType::RuidaUdp => "Ruida DSP (Ethernet/UDP)",
            ControllerType::VirtualDemo => "Virtual Device (Simulator)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Busy,
    Alarm,
}

/// Non-blocking state machine for safely stopping a physical job while preserving coordinate origin
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AbortSequencePhase {
    HoldIssued { stopped_x: f64, stopped_y: f64, started_at: Instant },
    ResetIssued { stopped_x: f64, stopped_y: f64, started_at: Instant },
    RestoreSent { stopped_x: f64, stopped_y: f64, started_at: Instant },
}

/// Non-blocking state machine for physical limit switch homing & zero-point calibration
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SwitchHomingPhase {
    /// Initializing sequence (clear alarm, ensure laser off)
    Starting { started_at: Instant },
    /// Moving X axis towards the X limit switch
    MovingX { started_at: Instant },
    /// X limit switch hit: waiting for soft reset reboot before unlocking and pulling off
    ResettingX { started_at: Instant },
    /// X limit switch disengaged: pulling off 5mm to release switch
    PullingOffX { started_at: Instant },
    /// Moving Y axis towards the Y limit switch
    MovingY { started_at: Instant },
    /// Y limit switch hit: waiting for soft reset reboot before unlocking and pulling off
    ResettingY { started_at: Instant },
    /// Y limit switch disengaged: pulling off 5mm to release switch
    PullingOffY { started_at: Instant },
    /// Finalizing: set G90, G92 X0 Y0, update LaserBurn zero point
    Finalizing { started_at: Instant },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomBoundaries {
    #[serde(default)]
    pub enabled: bool,
    pub min_x: Option<f64>,
    pub max_x: Option<f64>,
    pub min_y: Option<f64>,
    pub max_y: Option<f64>,
    // Saved corner positions (X, Y in work coordinates)
    pub corner_bottom_left: Option<(f64, f64)>,
    pub corner_bottom_right: Option<(f64, f64)>,
    pub corner_top_left: Option<(f64, f64)>,
    pub corner_top_right: Option<(f64, f64)>,
}

impl Default for CustomBoundaries {
    fn default() -> Self {
        Self {
            enabled: false,
            min_x: None,
            max_x: None,
            min_y: None,
            max_y: None,
            corner_bottom_left: None,
            corner_bottom_right: None,
            corner_top_left: None,
            corner_top_right: None,
        }
    }
}

impl CustomBoundaries {
    pub fn update_limits(&mut self) {
        let mut xs = Vec::new();
        let mut ys = Vec::new();

        if let Some((x, y)) = self.corner_bottom_left { xs.push(x); ys.push(y); }
        if let Some((x, y)) = self.corner_bottom_right { xs.push(x); ys.push(y); }
        if let Some((x, y)) = self.corner_top_left { xs.push(x); ys.push(y); }
        if let Some((x, y)) = self.corner_top_right { xs.push(x); ys.push(y); }

        if !xs.is_empty() && !ys.is_empty() {
            let min_x = xs.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min_y = ys.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_y = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

            self.min_x = Some(min_x);
            self.max_x = Some(max_x);
            self.min_y = Some(min_y);
            self.max_y = Some(max_y);
        }
    }

    pub fn is_within(&self, x: f64, y: f64) -> bool {
        if !self.enabled {
            return true;
        }
        if let Some(min_x) = self.min_x {
            if x < min_x - 1e-3 { return false; }
        }
        if let Some(max_x) = self.max_x {
            if x > max_x + 1e-3 { return false; }
        }
        if let Some(min_y) = self.min_y {
            if y < min_y - 1e-3 { return false; }
        }
        if let Some(max_y) = self.max_y {
            if y > max_y + 1e-3 { return false; }
        }
        true
    }

    pub fn clamp_target(&self, x: f64, y: f64) -> (f64, f64) {
        if !self.enabled {
            return (x, y);
        }
        let cx = match (self.min_x, self.max_x) {
            (Some(min), Some(max)) => x.clamp(min, max),
            (Some(min), None) => x.max(min),
            (None, Some(max)) => x.min(max),
            (None, None) => x,
        };
        let cy = match (self.min_y, self.max_y) {
            (Some(min), Some(max)) => y.clamp(min, max),
            (Some(min), None) => y.max(min),
            (None, Some(max)) => y.min(max),
            (None, None) => y,
        };
        (cx, cy)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSettings {
    // Bed dimensions
    pub bed_width: f64,
    pub bed_height: f64,
    pub origin_corner: OriginCorner,

    // Physical Limit Switches
    #[serde(default)]
    pub enable_limit_switches: bool, // When false, all switch-based homing and buttons are disabled for machine protection

    // Learned Custom Boundaries (Soft Limits per Corner)
    #[serde(default)]
    pub custom_boundaries: CustomBoundaries,

    // Laser & Optics
    pub max_s_value: f64,          // e.g. 1000 for GRBL $30
    pub weak_light_percent: f64,   // e.g. 1.0% for aiming / framing without burning
    pub weak_light_enabled: bool,
    pub laser_type: LaserType,
    #[serde(default = "default_kerf")]
    pub default_kerf: f64,         // Default beam kerf compensation in mm (0.0 = disabled)

    // Speeds
    pub rapid_speed: f64,          // G0 travel speed mm/min
    pub jog_speed: f64,            // Jog move speed mm/min
    pub frame_speed: f64,          // Test framing speed mm/min
    #[serde(default = "default_homing_speed")]
    pub homing_speed: f64,         // Homing / switch test seek speed mm/min (default 400.0)

    // Cut Path Optimization Settings
    #[serde(default = "default_true")]
    pub opt_inner_cuts_first: bool,  // Deepest nested holes and internal cutouts cut before outer perimeter
    #[serde(default = "default_true")]
    pub opt_minimize_travel: bool,   // Nearest-neighbor TSP rapid travel optimization
    #[serde(default = "default_true")]
    pub opt_allow_reversal: bool,    // Allow direction reversal on open toolpaths to save rapid travel
    #[serde(default)]
    pub opt_show_cut_order: bool,    // Show sequential numbered badges (1, 2, 3...) in simulation & canvas

    // Connection
    pub default_port: String,
    pub default_baud: u32,
    pub auto_connect: bool,
    #[serde(default)]
    pub save_traffic_log: bool,
    #[serde(default)]
    pub log_status_polls: bool,
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default = "default_true")]
    pub enable_camera_support: bool,

    // macOS Apple Shortcuts Automation (On Job Start / On Job Complete)
    #[serde(default)]
    pub run_shortcut_on_start: bool,
    #[serde(default)]
    pub shortcut_on_start_name: String,
    #[serde(default)]
    pub run_shortcut_on_complete: bool,
    #[serde(default)]
    pub shortcut_on_complete_name: String,

    // Multiple Machine Profiles
    #[serde(default = "default_current_profile_name")]
    pub current_profile_name: String,
    #[serde(default = "default_profiles_list")]
    pub profiles: Vec<DeviceProfile>,

    // Localization
    #[serde(default)]
    pub language: crate::i18n::Language,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceProfile {
    pub name: String,
    pub bed_width: f64,
    pub bed_height: f64,
    pub origin_corner: OriginCorner,
    #[serde(default)]
    pub enable_limit_switches: bool,
    #[serde(default)]
    pub custom_boundaries: CustomBoundaries,
    pub max_s_value: f64,
    pub weak_light_percent: f64,
    pub weak_light_enabled: bool,
    pub laser_type: LaserType,
    #[serde(default = "default_kerf")]
    pub default_kerf: f64,
    pub rapid_speed: f64,
    pub jog_speed: f64,
    pub frame_speed: f64,
    #[serde(default = "default_homing_speed")]
    pub homing_speed: f64,
    pub default_port: String,
    pub default_baud: u32,
    pub auto_connect: bool,
    #[serde(default = "default_true")]
    pub enable_camera_support: bool,
}

fn default_current_profile_name() -> String {
    "Oma 10W (Standard)".to_string()
}

fn default_true() -> bool {
    true
}

fn default_profiles_list() -> Vec<DeviceProfile> {
    vec![
        DeviceProfile {
            name: "Oma 10W (Standard)".to_string(),
            bed_width: 400.0,
            bed_height: 400.0,
            origin_corner: OriginCorner::BottomLeft,
            enable_limit_switches: false,
            custom_boundaries: CustomBoundaries::default(),
            max_s_value: 1000.0,
            weak_light_percent: 1.0,
            weak_light_enabled: false,
            laser_type: LaserType::Diode,
            default_kerf: 0.0,
            rapid_speed: 6000.0,
            jog_speed: 3000.0,
            frame_speed: 4000.0,
            homing_speed: 400.0,
            default_port: String::new(),
            default_baud: 115200,
            auto_connect: false,
            enable_camera_support: true,
        },
        DeviceProfile {
            name: "Compact Diode (200x200)".to_string(),
            bed_width: 200.0,
            bed_height: 200.0,
            origin_corner: OriginCorner::BottomLeft,
            enable_limit_switches: false,
            custom_boundaries: CustomBoundaries::default(),
            max_s_value: 1000.0,
            weak_light_percent: 1.0,
            weak_light_enabled: false,
            laser_type: LaserType::Diode,
            default_kerf: 0.0,
            rapid_speed: 4000.0,
            jog_speed: 2500.0,
            frame_speed: 3000.0,
            homing_speed: 300.0,
            default_port: String::new(),
            default_baud: 115200,
            auto_connect: false,
            enable_camera_support: true,
        },
        DeviceProfile {
            name: "Large Format / CO2 (600x400)".to_string(),
            bed_width: 600.0,
            bed_height: 400.0,
            origin_corner: OriginCorner::TopLeft,
            enable_limit_switches: true,
            custom_boundaries: CustomBoundaries::default(),
            max_s_value: 1000.0,
            weak_light_percent: 0.0,
            weak_light_enabled: false,
            laser_type: LaserType::Co2,
            default_kerf: 0.15,
            rapid_speed: 12000.0,
            jog_speed: 5000.0,
            frame_speed: 6000.0,
            homing_speed: 600.0,
            default_port: String::new(),
            default_baud: 115200,
            auto_connect: false,
            enable_camera_support: true,
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OriginCorner {
    BottomLeft,
    TopLeft,
    BottomRight,
    TopRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaserType {
    Diode,
    Co2,
    FiberGalvo,
}

fn default_homing_speed() -> f64 {
    400.0
}

fn default_kerf() -> f64 {
    0.0
}

impl DeviceSettings {
    pub fn config_path() -> Option<PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()?;
        let dir = PathBuf::from(home).join(".config").join("laser_burn");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join("settings.json"))
    }

    /// Switches to the specified profile by name and copies its settings into active fields.
    pub fn apply_profile(&mut self, name: &str) -> bool {
        if let Some(prof) = self.profiles.iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned() {
            self.bed_width = prof.bed_width;
            self.bed_height = prof.bed_height;
            self.origin_corner = prof.origin_corner;
            self.enable_limit_switches = prof.enable_limit_switches;
            self.custom_boundaries = prof.custom_boundaries;
            self.max_s_value = prof.max_s_value;
            self.weak_light_percent = prof.weak_light_percent;
            self.weak_light_enabled = prof.weak_light_enabled;
            self.laser_type = prof.laser_type;
            self.default_kerf = prof.default_kerf;
            self.rapid_speed = prof.rapid_speed;
            self.jog_speed = prof.jog_speed;
            self.frame_speed = prof.frame_speed;
            self.homing_speed = prof.homing_speed;
            self.default_port = prof.default_port;
            self.default_baud = prof.default_baud;
            self.auto_connect = prof.auto_connect;
            self.enable_camera_support = prof.enable_camera_support;
            self.current_profile_name = prof.name;
            true
        } else {
            false
        }
    }

    /// Saves current active settings under the specified profile name (updating or appending).
    pub fn save_current_as_profile(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let prof = DeviceProfile {
            name: name.to_string(),
            bed_width: self.bed_width,
            bed_height: self.bed_height,
            origin_corner: self.origin_corner,
            enable_limit_switches: self.enable_limit_switches,
            custom_boundaries: self.custom_boundaries.clone(),
            max_s_value: self.max_s_value,
            weak_light_percent: self.weak_light_percent,
            weak_light_enabled: self.weak_light_enabled,
            laser_type: self.laser_type,
            default_kerf: self.default_kerf,
            rapid_speed: self.rapid_speed,
            jog_speed: self.jog_speed,
            frame_speed: self.frame_speed,
            homing_speed: self.homing_speed,
            default_port: self.default_port.clone(),
            default_baud: self.default_baud,
            auto_connect: self.auto_connect,
            enable_camera_support: self.enable_camera_support,
        };
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.name.eq_ignore_ascii_case(name)) {
            *existing = prof;
        } else {
            self.profiles.push(prof);
        }
        self.current_profile_name = name.to_string();
    }

    /// Deletes a profile by name. Preserves at least 1 profile.
    pub fn delete_profile(&mut self, name: &str) -> bool {
        if self.profiles.len() <= 1 {
            return false;
        }
        let initial = self.profiles.len();
        self.profiles.retain(|p| !p.name.eq_ignore_ascii_case(name));
        if self.profiles.len() < initial {
            if self.current_profile_name.eq_ignore_ascii_case(name) {
                if let Some(first) = self.profiles.first().cloned() {
                    self.apply_profile(&first.name);
                }
            }
            true
        } else {
            false
        }
    }

    pub fn load_saved() -> Self {
        if cfg!(test) {
            return Self::default();
        }
        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(mut settings) = serde_json::from_str::<DeviceSettings>(&content) {
                        if settings.profiles.is_empty() {
                            settings.profiles = default_profiles_list();
                        }
                        if settings.current_profile_name.is_empty() {
                            settings.current_profile_name = settings.profiles[0].name.clone();
                        }
                        return settings;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        if cfg!(test) {
            return Ok(());
        }
        if let Some(path) = Self::config_path() {
            let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
            std::fs::write(&path, json).map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err("Could not locate home directory to save settings".to_string())
        }
    }

    pub fn traffic_log_path() -> Option<PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()?;
        let docs = PathBuf::from(&home).join("Documents");
        if docs.exists() {
            Some(docs.join("PhotonCAM_traffic.log"))
        } else {
            Some(PathBuf::from(home).join("PhotonCAM_traffic.log"))
        }
    }
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            bed_width: 400.0,
            bed_height: 400.0,
            origin_corner: OriginCorner::BottomLeft, // Default standard Front-Left origin (GRBL standard)
            enable_limit_switches: false, // Disabled by default for machine safety!
            custom_boundaries: CustomBoundaries::default(),

            max_s_value: 1000.0,
            weak_light_percent: 1.0, // 1% = S10, gentle diode glow
            weak_light_enabled: false,
            laser_type: LaserType::Diode,
            default_kerf: 0.0,

            rapid_speed: 6000.0,
            jog_speed: 3000.0,
            frame_speed: 4000.0,
            homing_speed: 400.0,

            opt_inner_cuts_first: true,
            opt_minimize_travel: true,
            opt_allow_reversal: true,
            opt_show_cut_order: false,

            default_port: "".to_string(),
            default_baud: 115200,
            auto_connect: false,
            save_traffic_log: false,
            log_status_polls: false,
            dark_mode: false,
            enable_camera_support: true,
            run_shortcut_on_start: false,
            shortcut_on_start_name: String::new(),
            run_shortcut_on_complete: false,
            shortcut_on_complete_name: String::new(),

            current_profile_name: default_current_profile_name(),
            profiles: default_profiles_list(),
            language: crate::i18n::Language::English,
        }
    }
}

pub struct DeviceManager {
    pub settings: DeviceSettings,
    pub controller_type: ControllerType,
    pub selected_port: String,
    pub selected_baud: u32,
    pub status: ConnectionStatus,

    pub machine_x: f64,
    pub machine_y: f64,
    pub work_x: f64,
    pub work_y: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub reported_x: f64,
    pub reported_y: f64,
    pub grbl_state: String,
    pub pending_ok_count: usize,
    pub framing_target_sent: bool,
    pub rx_buffer: String,
    pub last_status_poll: Option<Instant>,

    // Real-time limit switch & input pin state (GRBL Pn: reporting via $10=19)
    pub pin_state: String,
    pub pin_limit_x: bool,
    pub pin_limit_y: bool,
    pub pin_limit_z: bool,
    pub pin_probe: bool,

    pub is_weak_light_on: bool,
    pub is_framing_active: bool,
    pub framing_points: Vec<Point2D>,
    pub framing_idx: usize,

    // Work Coordinate Offset (WCO) & Stop Coordinate Retention
    pub last_wco: (f64, f64),
    pub abort_phase: Option<AbortSequencePhase>,
    pub homing_phase: Option<SwitchHomingPhase>,

    // Real-time Job Streaming
    pub is_job_running: bool,
    pub is_job_paused: bool,
    pub is_laser_firing: bool,
    pub job_lines: Vec<String>,
    pub job_idx: usize,
    pub job_total_lines: usize,
    pub job_progress: f32,
    pub current_command: String,
    pub job_start_instant: Option<Instant>,
    pub job_estimated_total_sec: f64,

    pub console_history: VecDeque<String>,
    pub max_history: usize,

    active_port: Option<LinkPort>,

    /// Raw RX byte subscribers (used by the Bridge to relay controller output to TCP clients).
    pub raw_subscribers: Vec<std::sync::mpsc::Sender<Vec<u8>>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        let settings = DeviceSettings::load_saved();
        let default_port = settings.default_port.clone();
        let default_baud = settings.default_baud;

        let mut dm = Self {
            settings,
            controller_type: ControllerType::Grbl,
            selected_port: default_port.clone(),
            selected_baud: default_baud,
            status: ConnectionStatus::Disconnected, // Realistically starts disconnected

            machine_x: 0.0,
            machine_y: 0.0,
            work_x: 0.0,
            work_y: 0.0,
            target_x: 0.0,
            target_y: 0.0,
            reported_x: 0.0,
            reported_y: 0.0,
            grbl_state: "Idle".to_string(),
            pending_ok_count: 0,
            framing_target_sent: false,
            rx_buffer: String::new(),
            last_status_poll: None,

            pin_state: String::new(),
            pin_limit_x: false,
            pin_limit_y: false,
            pin_limit_z: false,
            pin_probe: false,

            is_weak_light_on: false,
            is_framing_active: false,
            framing_points: Vec::new(),
            framing_idx: 0,

            last_wco: (0.0, 0.0),
            abort_phase: None,
            homing_phase: None,

            is_job_running: false,
            is_job_paused: false,
            is_laser_firing: false,
            job_lines: Vec::new(),
            job_idx: 0,
            job_total_lines: 0,
            job_progress: 0.0,
            current_command: "".to_string(),
            job_start_instant: None,
            job_estimated_total_sec: 0.0,

            console_history: VecDeque::new(),
            max_history: 2000,

            active_port: None,
            raw_subscribers: Vec::new(),
        };

        dm.log("[PhotonCAM] Controller subsystem initialized.");

        // Auto-connect to previous device if enabled and port exists
        if dm.settings.auto_connect && !default_port.trim().is_empty() {
            if parse_network_port(&default_port).is_some() || std::path::Path::new(&default_port).exists() {
                dm.log(&format!("[AUTO-CONNECT] Connecting to previous device: {} ({} baud)...", default_port, default_baud));
                let _ = dm.connect(&default_port, default_baud);
            } else {
                dm.log(&format!("[AUTO-CONNECT] Previous port {} is not connected. Connect laser USB and power.", default_port));
            }
        } else {
            dm.log("[CONNECTION] No device connected. Connect laser power and USB cable, then click Connect.");
        }

        dm
    }

    pub fn log(&mut self, line: &str) {
        if self.console_history.len() >= self.max_history {
            self.console_history.pop_front();
        }
        self.console_history.push_back(line.to_string());

        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/laserburn.log") {
            let _ = writeln!(f, "{}", line);
        }

        if self.settings.save_traffic_log && !cfg!(test) {
            if let Some(log_path) = DeviceSettings::traffic_log_path() {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                    let secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let s = secs % 60;
                    let m = (secs / 60) % 60;
                    let h = (secs / 3600) % 24;
                    if let Some(expl) = Self::explain_traffic_line(line) {
                        let _ = writeln!(f, "[{:02}:{:02}:{:02} UTC] {}  ({})", h, m, s, line, expl);
                    } else {
                        let _ = writeln!(f, "[{:02}:{:02}:{:02} UTC] {}", h, m, s, line);
                    }
                }
            }
        }
    }

    /// Returns a human-readable English explanation for a G-code command, GRBL system command,
    /// status report, setting readout, or controller response.
    pub fn explain_traffic_line(line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('[') {
            return None;
        }

        let is_rx = trimmed.starts_with('<') && !trimmed.starts_with("<Idle") && !trimmed.starts_with("<Run") && !trimmed.starts_with("<Hold") && !trimmed.starts_with("<Jog") && !trimmed.starts_with("<Alarm") && !trimmed.starts_with("<Home");
        let raw = trimmed
            .strip_prefix("> ")
            .or_else(|| trimmed.strip_prefix('<').map(|s| s.trim_start()))
            .unwrap_or(trimmed)
            .trim();

        if raw.is_empty() {
            return None;
        }

        // 1. Standard GRBL responses
        if raw.eq_ignore_ascii_case("ok") {
            return Some("Command accepted".to_string());
        }
        if raw.starts_with("Grbl ") || raw.starts_with("GrblHal") {
            return Some("Controller firmware initialized and ready".to_string());
        }
        if let Some(err_num) = raw.strip_prefix("error:") {
            let desc = match err_num.trim() {
                "1" => "G-code words consist of a letter and a value",
                "2" => "Numeric value format is not valid",
                "3" => "GRBL '$' system command was not recognized",
                "4" => "Negative value received for an expected positive value",
                "5" => "Homing cycle is not enabled via settings ($22)",
                "6" => "Minimum step pulse time must be greater than 3 usec",
                "7" => "EEPROM read failed; defaults restored",
                "8" => "GRBL '$' command cannot be used unless IDLE",
                "9" => "G-code locked out during alarm or jog state ($X to unlock)",
                "10" => "Soft limits cannot be enabled without homing ($22)",
                "11" => "Max characters per line exceeded",
                "12" => "GRBL '$' setting value exceeds maximum step rate",
                "13" => "Safety door detected as opened",
                "14" => "Build info or startup line exceeded EEPROM line length",
                "15" => "Jog target exceeds machine travel",
                "16" => "Invalid jog command",
                "17" => "Laser mode requires PWM output",
                "20" => "Unsupported or invalid G-code command",
                "21" => "Multiple G-code commands from same modal group",
                "22" => "Feed rate (F) has not been set or is undefined",
                "23" => "G-code command requires an integer value",
                "24" => "Two G-code commands both require XYZ words",
                "25" => "A G-code word was repeated in the block",
                "26" => "G-code command requires XYZ coordinates",
                "31" => "Unused axis words in block",
                "33" => "Motion command has an invalid target",
                _ => "GRBL controller command error",
            };
            return Some(format!("Error: {}", desc));
        }
        if let Some(alarm_num) = raw.strip_prefix("ALARM:") {
            let desc = match alarm_num.trim() {
                "1" => "Hard limit triggered — position lost ($X to unlock)",
                "2" => "Soft limit exceeded — target outside machine bed",
                "3" => "Reset while in motion — position may be lost",
                "4" => "Probe fail — not in expected initial state",
                "5" => "Probe fail — did not contact workpiece",
                "6" => "Homing fail — reset during active homing cycle",
                "7" => "Homing fail — safety door opened during homing",
                "8" => "Homing fail — pull-off failed to clear limit switch",
                "9" => "Homing fail — limit switch not found within search distance",
                _ => "Hardware alarm triggered ($X to unlock)",
            };
            return Some(format!("Alarm: {}", desc));
        }

        // 2. Real-time status frames (<Idle|MPos:0.000,0.000,0.000|FS:0,0>)
        let status_candidate = if trimmed.starts_with("< ") {
            trimmed.trim_start_matches("< ").trim()
        } else {
            raw
        };
        if status_candidate.starts_with('<') && status_candidate.ends_with('>') {
            let inner = &status_candidate[1..status_candidate.len() - 1];
            let mut parts = inner.split('|');
            let state = parts.next().unwrap_or("Unknown");
            let mut pos_str = String::new();
            for p in parts {
                if let Some(coords) = p.strip_prefix("MPos:").or_else(|| p.strip_prefix("WPos:")) {
                    let mut c = coords.split(',');
                    if let (Some(x), Some(y)) = (c.next(), c.next()) {
                        pos_str = format!(", Pos X={} Y={} mm", x, y);
                    }
                }
            }
            return Some(format!("Status: {}{}", state, pos_str));
        }
        let _ = is_rx;

        // 3. GRBL system commands & settings ($...)
        if raw == "$$" {
            return Some("Read all GRBL firmware settings".to_string());
        }
        if raw.eq_ignore_ascii_case("$X") {
            return Some("Clear alarm lock ($X)".to_string());
        }
        if raw.eq_ignore_ascii_case("$H") {
            return Some("Home machine to physical limit switches".to_string());
        }
        if raw.eq_ignore_ascii_case("$I") {
            return Some("Query firmware build info".to_string());
        }
        if raw.eq_ignore_ascii_case("$G") {
            return Some("Query active G-code parser state".to_string());
        }
        if raw.eq_ignore_ascii_case("$#") {
            return Some("Query work coordinate offsets".to_string());
        }
        if raw == "?" {
            return Some("Query real-time machine status".to_string());
        }
        if raw == "!" {
            return Some("Feed hold (Pause motion)".to_string());
        }
        if raw == "~" {
            return Some("Cycle start / Resume motion".to_string());
        }
        if let Some(jog_body) = raw.strip_prefix("$J=") {
            let mut mode = "Jog";
            let mut x_str = None;
            let mut y_str = None;
            let mut f_str = None;
            for tok in jog_body.split_whitespace() {
                let up = tok.to_uppercase();
                if up == "G91" {
                    mode = "Relative jog";
                } else if up == "G90" {
                    mode = "Absolute jog";
                } else if let Some(v) = up.strip_prefix('X') {
                    x_str = Some(v.to_string());
                } else if let Some(v) = up.strip_prefix('Y') {
                    y_str = Some(v.to_string());
                } else if let Some(v) = up.strip_prefix('F') {
                    if let Ok(f_val) = v.parse::<f64>() {
                        f_str = Some(format!("{:.1} mm/s", f_val / 60.0));
                    }
                }
            }
            let mut details = Vec::new();
            if let Some(x) = x_str {
                details.push(format!("X={}", x));
            }
            if let Some(y) = y_str {
                details.push(format!("Y={} mm", y));
            }
            if let Some(f) = f_str {
                details.push(format!("at {}", f));
            }
            return Some(format!("{} {}", mode, details.join(" ")));
        }
        if let Some(rest) = raw.strip_prefix('$') {
            if let Some((num, val)) = rest.split_once('=') {
                let setting_name = match num.trim() {
                    "0" => "Step pulse time (us)",
                    "1" => "Step idle delay (ms)",
                    "2" => "Step port invert mask",
                    "3" => "Direction port invert mask",
                    "4" => "Step enable invert",
                    "5" => "Limit pins invert",
                    "6" => "Probe pin invert",
                    "10" => "Status report mask",
                    "11" => "Junction deviation (mm)",
                    "12" => "Arc tolerance (mm)",
                    "13" => "Report inches (0=mm, 1=in)",
                    "20" => "Soft limits (0=off, 1=on)",
                    "21" => "Hard limits (0=off, 1=on)",
                    "22" => "Homing cycle (0=off, 1=on)",
                    "23" => "Homing dir invert mask",
                    "24" => "Homing feed rate (mm/min)",
                    "25" => "Homing seek rate (mm/min)",
                    "26" => "Homing debounce (ms)",
                    "27" => "Homing pull-off (mm)",
                    "30" => "Max laser S-value (1000=100%)",
                    "31" => "Min laser S-value",
                    "32" => "Laser mode (1=ON, 0=CNC)",
                    "100" => "X-axis steps/mm",
                    "101" => "Y-axis steps/mm",
                    "102" => "Z-axis steps/mm",
                    "110" => "X-axis max rate (mm/min)",
                    "111" => "Y-axis max rate (mm/min)",
                    "112" => "Z-axis max rate (mm/min)",
                    "120" => "X-axis acceleration (mm/s^2)",
                    "121" => "Y-axis acceleration (mm/s^2)",
                    "122" => "Z-axis acceleration (mm/s^2)",
                    "130" => "X-axis max travel (mm)",
                    "131" => "Y-axis max travel (mm)",
                    "132" => "Z-axis max travel (mm)",
                    _ => "GRBL firmware setting",
                };
                return Some(format!("Setting ${}: {} = {}", num.trim(), setting_name, val.trim()));
            }
        }

        // 4. Standard G-code / M-code instructions
        let code_part = raw.split(';').next().unwrap_or(raw).trim();
        if code_part.is_empty() {
            return None;
        }

        let mut actions: Vec<String> = Vec::new();
        let mut x_val: Option<String> = None;
        let mut y_val: Option<String> = None;
        let mut f_val: Option<f64> = None;
        let mut s_val: Option<f64> = None;
        let mut p_val: Option<String> = None;

        for token in code_part.split_whitespace() {
            let up = token.to_uppercase();
            match up.as_str() {
                "G0" | "G00" => actions.push("Rapid move (Laser OFF)".to_string()),
                "G1" | "G01" => actions.push("Laser cut/move".to_string()),
                "G2" | "G02" => actions.push("Clockwise arc move".to_string()),
                "G3" | "G03" => actions.push("Counter-clockwise arc move".to_string()),
                "G4" | "G04" => actions.push("Dwell / Pause".to_string()),
                "G20" => actions.push("Units: Inches".to_string()),
                "G21" => actions.push("Units: Millimeters".to_string()),
                "G28" => actions.push("Go to home position".to_string()),
                "G53" => actions.push("Machine coordinates".to_string()),
                "G54" => actions.push("Work coordinates G54".to_string()),
                "G90" => actions.push("Absolute positioning".to_string()),
                "G91" => actions.push("Relative positioning".to_string()),
                "G92" => actions.push("Set work coordinate origin".to_string()),
                "M3" | "M03" => actions.push("Laser ON (Constant power)".to_string()),
                "M4" | "M04" => actions.push("Laser ON (Dynamic power)".to_string()),
                "M5" | "M05" => actions.push("Laser OFF".to_string()),
                "M8" | "M08" => actions.push("Air Assist ON".to_string()),
                "M9" | "M09" => actions.push("Air Assist OFF".to_string()),
                "M2" | "M02" | "M30" => actions.push("End of program".to_string()),
                _ => {
                    if let Some(v) = up.strip_prefix('X') {
                        x_val = Some(v.to_string());
                    } else if let Some(v) = up.strip_prefix('Y') {
                        y_val = Some(v.to_string());
                    } else if let Some(v) = up.strip_prefix('F') {
                        if let Ok(fv) = v.parse::<f64>() {
                            f_val = Some(fv);
                        }
                    } else if let Some(v) = up.strip_prefix('S') {
                        if let Ok(sv) = v.parse::<f64>() {
                            s_val = Some(sv);
                        }
                    } else if let Some(v) = up.strip_prefix('P') {
                        p_val = Some(v.to_string());
                    }
                }
            }
        }

        if actions.is_empty() && (x_val.is_some() || y_val.is_some()) {
            actions.push("Move".to_string());
        }

        let mut parts = Vec::new();
        if !actions.is_empty() {
            parts.push(actions.join(", "));
        }
        match (&x_val, &y_val) {
            (Some(x), Some(y)) => parts.push(format!("to X={} Y={} mm", x, y)),
            (Some(x), None) => parts.push(format!("to X={} mm", x)),
            (None, Some(y)) => parts.push(format!("to Y={} mm", y)),
            (None, None) => {}
        }
        if let Some(f) = f_val {
            parts.push(format!("Speed {:.1} mm/s", f / 60.0));
        }
        if let Some(s) = s_val {
            let pct = (s / 10.0).clamp(0.0, 100.0);
            parts.push(format!("Power {:.1}%", pct));
        }
        if let Some(p) = p_val {
            parts.push(format!("Wait {} s", p));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }

    /// Checks if a physical hardware serial port is currently open
    pub fn is_physically_connected(&self) -> bool {
        self.active_port.is_some()
    }

    /// Enumerate all currently available system serial / USB ports
    pub fn scan_available_ports() -> Vec<String> {
        let mut ports = Vec::new();

        if let Ok(available) = serialport::available_ports() {
            for p in available {
                ports.push(p.port_name);
            }
        }

        // Add macOS / Linux standard laser port candidates ONLY if the device file actually exists on filesystem
        let standard_candidates = [
            "/dev/tty.usbserial-10",
            "/dev/tty.usbserial-11",
            "/dev/tty.usbserial-0001",
            "/dev/tty.usbmodem101",
            "/dev/tty.wchusbserial1410",
            "/dev/cu.usbserial",
            "/dev/ttyUSB0",
            "/dev/ttyUSB1",
            "/dev/ttyACM0",
            "/dev/ttyACM1",
        ];

        for &c in &standard_candidates {
            if std::path::Path::new(c).exists() && !ports.contains(&c.to_string()) {
                ports.push(c.to_string());
            }
        }

        // Always provide a Virtual demo port for testing without laser hardware
        if !ports.contains(&"Virtual Port (Demo / Offline)".to_string()) {
            ports.push("Virtual Port (Demo / Offline)".to_string());
        }

        ports
    }

    /// Connect to selected serial port
    pub fn connect(&mut self, port_name: &str, baud: u32) -> Result<(), String> {
        self.selected_port = port_name.to_string();
        self.selected_baud = baud;

        if port_name == "Virtual Port (Demo / Offline)" || port_name == "Virtuaaliportti (Demo / Offline)" || self.controller_type == ControllerType::VirtualDemo {
            self.status = ConnectionStatus::Connected;
            self.log(&format!("[SIMULATOR] Connected to virtual controller ({} baud, offline mode).", baud));
            self.log("[GRBL] Grbl 1.1h ['$' for help]");
            return Ok(());
        }

        if !port_name.is_empty() {
            let link: Result<LinkPort, String> = if let Some(addr) = parse_network_port(port_name) {
                self.log(&format!("[CONNECTION] Connecting over network to {} ...", addr));
                Self::open_tcp(&addr).map(LinkPort::Tcp)
            } else {
                self.log(&format!("[CONNECTION] Connecting to port {} ({} baud)...", port_name, baud));
                serialport::new(port_name, baud)
                    .timeout(Duration::from_millis(100))
                    .open()
                    .map(LinkPort::Serial)
                    .map_err(|e| e.to_string())
            };
            match link {
                Ok(port) => {
                    self.active_port = Some(port);
                    self.status = ConnectionStatus::Connected;
                    self.rx_buffer.clear();
                    self.pending_ok_count = 0;
                    self.framing_target_sent = false;
                    self.grbl_state = "Idle".to_string();
                    self.abort_phase = None;
                    self.homing_phase = None;

                    // Remember successful physical connection and auto-save settings
                    if port_name != "Virtual Port (Demo / Offline)" && port_name != "Virtuaaliportti (Demo / Offline)" {
                        self.settings.default_port = port_name.to_string();
                        self.settings.default_baud = baud;
                        self.settings.auto_connect = true;
                        let _ = self.settings.save();
                    }

                    self.log(&format!("[CONNECTION] ✅ Successfully connected to device: {} ({} baud)", port_name, baud));
                    // Send wake-up reset to GRBL, enable pin reporting ($10=19: MPos+Buffers+PinState), hard limits ($21=1) and immediate status poll
                    self.send_raw("\r\n\r\n");
                    self.send_raw("$10=19");
                    self.send_raw("$21=1");
                    self.send_raw("$$");
                    self.send_realtime(b'?');
                    return Ok(());
                }
                Err(e) => {
                    self.active_port = None;
                    self.status = ConnectionStatus::Disconnected;
                    let err_msg = if parse_network_port(port_name).is_some() {
                        format!("Cannot connect to {}: {}. Check that the Raspberry Pi is powered on, on the same network, and photoncam-bridge is running.", port_name, e)
                    } else {
                        format!("Cannot connect to port {}: {}. Ensure laser is powered on and USB cable is connected!", port_name, e)
                    };
                    self.log(&format!("[ERROR] ❌ {}", err_msg));
                    return Err(err_msg);
                }
            }
        }

        self.status = ConnectionStatus::Disconnected;
        let err_msg = "Select a port before connecting".to_string();
        self.log(&format!("[WARNING] {}", err_msg));
        Err(err_msg)
    }

    /// Open a TCP link to a PhotonCAM Bridge (`host:port`) with a short connect timeout.
    fn open_tcp(addr: &str) -> Result<std::net::TcpStream, String> {
        use std::net::ToSocketAddrs;
        let addrs = addr.to_socket_addrs().map_err(|e| format!("cannot resolve '{}': {}", addr, e))?;
        let mut last_err = String::from("no address found");
        for a in addrs {
            match std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)) {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    return Ok(stream);
                }
                Err(e) => last_err = e.to_string(),
            }
        }
        Err(last_err)
    }

    /// Write raw bytes straight to the controller link (no newline added, no logging).
    pub fn write_raw_bytes(&mut self, bytes: &[u8]) {
        if let Some(ref mut port) = self.active_port {
            use std::io::Write;
            let _ = port.write_all(bytes);
            let _ = port.flush();
        }
    }

    /// Disconnect from port
    pub fn disconnect(&mut self) {
        self.active_port = None;
        self.status = ConnectionStatus::Disconnected;
        self.is_weak_light_on = false;
        self.is_framing_active = false;
        self.abort_phase = None;
        self.homing_phase = None;
        self.log("[CONNECTION] Disconnected.");
    }

    /// Send raw string command to controller
    pub fn send_raw(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        if !trimmed.is_empty() {
            self.log(&format!("> {}", trimmed));
        }

        if let Some(ref mut port) = self.active_port {
            use std::io::Write;
            let _ = writeln!(port, "{}", trimmed);
            let _ = port.flush();
        }

        // Mock echo response for virtual mode
        if !self.is_physically_connected() {
            if trimmed == "$$" {
                self.log("$0=10 (Step pulse time, microseconds)");
                self.log("$1=25 (Step idle delay, milliseconds)");
                self.log("$30=1000 (Max spindle speed, RPM/S-value)");
                self.log("$31=0 (Min spindle speed, RPM)");
                self.log("$32=1 (Laser-mode enable)");
                self.log("$100=80.000 (X-axis travel resolution, step/mm)");
                self.log("$101=80.000 (Y-axis travel resolution, step/mm)");
                self.log("$110=6000.000 (X-axis maximum rate, mm/min)");
                self.log("$111=6000.000 (Y-axis maximum rate, mm/min)");
                self.log("$130=400.000 (X-axis maximum travel, mm)");
                self.log("$131=400.000 (Y-axis maximum travel, mm)");
                self.log("ok");
            } else if trimmed == "?" {
                self.log(&format!("<Idle|MPos:{:.3},{:.3},0.000|FS:0,0|WCO:0.000,0.000,0.000>", self.machine_x, self.machine_y));
                self.log("ok");
            } else if trimmed.starts_with("$J=") || trimmed.starts_with("G0") || trimmed.starts_with("G1") || trimmed == "$H" || trimmed.starts_with("G92") {
                self.log("ok");
            }
        }
    }

    /// Send a single real-time byte directly without newline (e.g. '?' for status, '!' for feed hold, '~' for cycle start)
    pub fn send_realtime(&mut self, byte: u8) {
        if let Some(ref mut port) = self.active_port {
            use std::io::Write;
            let _ = port.write_all(&[byte]);
            let _ = port.flush();
        }
    }

    /// Direction vector (dir_x, dir_y) towards the limit switches of the configured OriginCorner
    pub fn origin_corner_direction(&self) -> (f64, f64) {
        match self.settings.origin_corner {
            // For TopRight with $3=3 inverted axes:
            // -X moves Right towards switch, -Y moves Up/Rear towards switch
            OriginCorner::TopRight => (-1.0, -1.0),
            OriginCorner::TopLeft => (1.0, -1.0),
            OriginCorner::BottomRight => (-1.0, 1.0),
            OriginCorner::BottomLeft => (1.0, 1.0),
        }
    }

    /// UI arrow jog helper: translates screen arrow directions (left/right, up/down)
    /// to machine coordinate delta based on origin corner so arrows always move head in visual direction.
    pub fn jog_arrow(&mut self, arrow_x: f64, arrow_y: f64, step: f64, feed: f64) {
        let (dir_x, dir_y) = match self.settings.origin_corner {
            OriginCorner::TopRight => (-arrow_x, -arrow_y),
            OriginCorner::TopLeft => (arrow_x, -arrow_y),
            OriginCorner::BottomRight => (-arrow_x, arrow_y),
            OriginCorner::BottomLeft => (arrow_x, arrow_y),
        };
        self.jog(dir_x * step, dir_y * step, feed);
    }

    /// Real-time JOG move (GRBL $J=G91 G21 X... Y... F...)
    pub fn jog(&mut self, dx: f64, dy: f64, feed: f64) {
        if dx.abs() < 1e-4 && dy.abs() < 1e-4 {
            return;
        }

        // Enforce custom learned boundaries (soft limits)
        let (eff_dx, eff_dy) = if self.settings.custom_boundaries.enabled {
            let cur_x = if self.is_physically_connected() { self.work_x } else { self.target_x };
            let cur_y = if self.is_physically_connected() { self.work_y } else { self.target_y };

            let target_x = cur_x + dx;
            let target_y = cur_y + dy;

            if !self.settings.custom_boundaries.is_within(target_x, target_y) {
                let (clamped_x, clamped_y) = self.settings.custom_boundaries.clamp_target(target_x, target_y);
                let allowed_dx = clamped_x - cur_x;
                let allowed_dy = clamped_y - cur_y;

                if allowed_dx.abs() < 1e-4 && allowed_dy.abs() < 1e-4 {
                    self.log(&format!(
                        "[LIMIT] Motion blocked: Laser head reached soft limit (X: {:.1}, Y: {:.1} mm)!",
                        cur_x, cur_y
                    ));
                    return;
                }
                (allowed_dx, allowed_dy)
            } else {
                (dx, dy)
            }
        } else {
            (dx, dy)
        };

        if !self.is_physically_connected() {
            let mut new_x = (self.target_x + eff_dx).clamp(0.0, self.settings.bed_width);
            let mut new_y = (self.target_y + eff_dy).clamp(0.0, self.settings.bed_height);
            if self.settings.custom_boundaries.enabled {
                let (cx, cy) = self.settings.custom_boundaries.clamp_target(new_x, new_y);
                new_x = cx;
                new_y = cy;
            }
            self.target_x = new_x;
            self.target_y = new_y;
            self.machine_x = new_x;
            self.machine_y = new_y;
            self.work_x = new_x;
            self.work_y = new_y;
            return;
        }

        // If controller is currently in Alarm lock, unlock it automatically with $X so jog can proceed!
        if self.grbl_state.starts_with("Alarm") {
            self.send_raw("$X");
        }

        // Clean command formatting: only include axes that actually move (prevents Y-0.000 / X-0.000)
        let cmd = if eff_dx.abs() >= 1e-4 && eff_dy.abs() >= 1e-4 {
            format!("$J=G91 X{:.3} Y{:.3} F{:.0}", eff_dx, eff_dy, feed)
        } else if eff_dx.abs() >= 1e-4 {
            format!("$J=G91 X{:.3} F{:.0}", eff_dx, feed)
        } else {
            format!("$J=G91 Y{:.3} F{:.0}", eff_dy, feed)
        };
        self.send_raw(&cmd);
        self.send_realtime(b'?');
    }

    /// Save current laser head position as a named boundary corner
    pub fn save_boundary_corner(&mut self, corner_name: &str) {
        let x = if self.is_physically_connected() { self.work_x } else { self.target_x };
        let y = if self.is_physically_connected() { self.work_y } else { self.target_y };

        match corner_name {
            "bottom_left" => self.settings.custom_boundaries.corner_bottom_left = Some((x, y)),
            "bottom_right" => self.settings.custom_boundaries.corner_bottom_right = Some((x, y)),
            "top_left" => self.settings.custom_boundaries.corner_top_left = Some((x, y)),
            "top_right" => self.settings.custom_boundaries.corner_top_right = Some((x, y)),
            _ => {}
        }
        self.settings.custom_boundaries.update_limits();
        self.settings.custom_boundaries.enabled = true; // Auto-enable when a corner is taught!
        let _ = self.settings.save();
        self.log(&format!("[LIMITS] Taught corner [{}]: X={:.2}, Y={:.2} mm", corner_name, x, y));
    }

    /// Clear all custom learned boundary corners
    pub fn clear_boundary_corners(&mut self) {
        self.settings.custom_boundaries = CustomBoundaries::default();
        let _ = self.settings.save();
        self.log("[LIMITS] Custom learned boundaries cleared.");
    }

    /// Home the laser ($H) / Reset to mechanical switches
    pub fn home(&mut self) {
        if !self.settings.enable_limit_switches {
            self.log("[HOME] Physical limit switches disabled in settings. Enable in Settings.");
            return;
        }
        self.start_mechanical_home_reset();
    }

    /// Set current position as Work Origin (0, 0)
    pub fn set_origin_here(&mut self) {
        self.target_x = 0.0;
        self.target_y = 0.0;
        self.machine_x = 0.0;
        self.machine_y = 0.0;
        self.work_x = 0.0;
        self.work_y = 0.0;
        self.last_wco = (0.0, 0.0);
        self.send_raw("G92 X0 Y0");
        self.send_realtime(b'?');
        self.log("[ORIGIN] Current head position calibrated as Work Origin (0,0) (G92 X0 Y0)!");
    }

    /// Go to Work Origin (0, 0)
    pub fn go_to_origin(&mut self) {
        self.go_to_position(0.0, 0.0);
    }

    /// Get target machine coordinates for a corner ("top_left", "top_right", "bottom_left", "bottom_right")
    pub fn get_corner_coordinates(&self, corner_name: &str) -> (f64, f64) {
        // 1. If the user has saved custom taught corners, use them:
        match corner_name {
            "top_left" => {
                if let Some(pt) = self.settings.custom_boundaries.corner_top_left {
                    return pt;
                }
            }
            "top_right" => {
                if let Some(pt) = self.settings.custom_boundaries.corner_top_right {
                    return pt;
                }
            }
            "bottom_left" => {
                if let Some(pt) = self.settings.custom_boundaries.corner_bottom_left {
                    return pt;
                }
            }
            "bottom_right" => {
                if let Some(pt) = self.settings.custom_boundaries.corner_bottom_right {
                    return pt;
                }
            }
            _ => {}
        }

        // 2. Fallback to machine bed bounds based on origin corner:
        let doc_pt = match corner_name {
            "top_left" => Point2D::new(0.0, 0.0),
            "top_right" => Point2D::new(self.settings.bed_width, 0.0),
            "bottom_left" => Point2D::new(0.0, self.settings.bed_height),
            "bottom_right" => Point2D::new(self.settings.bed_width, self.settings.bed_height),
            _ => Point2D::new(0.0, 0.0),
        };
        let m = self.doc_to_machine(doc_pt);
        (m.x, m.y)
    }

    /// Move laser head to a specific corner ("top_left", "top_right", "bottom_left", "bottom_right")
    pub fn go_to_corner(&mut self, corner_name: &str) {
        let (raw_x, raw_y) = self.get_corner_coordinates(corner_name);
        self.go_to_position(raw_x, raw_y);
    }

    /// Move laser head to specific absolute position (G0 G90 X... Y... F...)
    pub fn go_to_position(&mut self, x: f64, y: f64) {
        let (target_x, target_y) = if self.settings.custom_boundaries.enabled {
            self.settings.custom_boundaries.clamp_target(x, y)
        } else {
            (
                x.clamp(0.0, self.settings.bed_width),
                y.clamp(0.0, self.settings.bed_height),
            )
        };

        self.target_x = target_x;
        self.target_y = target_y;
        let feed = self.settings.rapid_speed;

        // If in Alarm state, unlock first:
        if self.grbl_state.starts_with("Alarm") {
            self.send_raw("$X");
        }

        self.send_raw(&format!("G90 G0 X{:.3} Y{:.3} F{:.0}", target_x, target_y, feed));
        self.send_realtime(b'?');
        self.log(&format!(
            "[JOG] Moving to position: X={:.1}, Y={:.1} mm...",
            target_x, target_y
        ));

        if !self.is_physically_connected() {
            self.machine_x = target_x;
            self.machine_y = target_y;
            self.work_x = target_x;
            self.work_y = target_y;
        }
    }

    /// Convert document coordinates (where X=0 is left, Y=0 is top)
    /// to machine coordinates relative to machine's OriginCorner setting.
    pub fn doc_to_machine(&self, pt: Point2D) -> Point2D {
        match self.settings.origin_corner {
            OriginCorner::TopLeft => Point2D::new(
                pt.x.clamp(0.0, self.settings.bed_width),
                pt.y.clamp(0.0, self.settings.bed_height),
            ),
            OriginCorner::TopRight => Point2D::new(
                (self.settings.bed_width - pt.x).clamp(0.0, self.settings.bed_width),
                pt.y.clamp(0.0, self.settings.bed_height),
            ),
            OriginCorner::BottomLeft => Point2D::new(
                pt.x.clamp(0.0, self.settings.bed_width),
                (self.settings.bed_height - pt.y).clamp(0.0, self.settings.bed_height),
            ),
            OriginCorner::BottomRight => Point2D::new(
                (self.settings.bed_width - pt.x).clamp(0.0, self.settings.bed_width),
                (self.settings.bed_height - pt.y).clamp(0.0, self.settings.bed_height),
            ),
        }
    }

    /// Convert machine coordinates (relative to machine OriginCorner) to document coordinates
    pub fn machine_to_doc(&self, m_pt: Point2D) -> Point2D {
        self.doc_to_machine(m_pt)
    }

    /// Go to arbitrary document point (converts from document coordinates to machine coordinates)
    pub fn go_to_point(&mut self, pt: Point2D) {
        let m_pt = self.doc_to_machine(pt);
        self.target_x = m_pt.x;
        self.target_y = m_pt.y;
        let feed = self.settings.rapid_speed;
        self.send_raw(&format!("G90 G0 X{:.3} Y{:.3} F{:.0}", self.target_x, self.target_y, feed));
        self.log(&format!("[CONTROLLER] Moving to point (X{:.1} Y{:.1})...", self.target_x, self.target_y));
        if !self.is_physically_connected() {
            self.machine_x = self.target_x;
            self.machine_y = self.target_y;
            self.work_x = self.target_x;
            self.work_y = self.target_y;
        }
    }

    /// Toggle Weak Light on or off
    /// Uses M3 S<power> in laser mode ($32=1) for gentle diode beam spot
    pub fn toggle_weak_light(&mut self) -> bool {
        self.set_weak_light(!self.is_weak_light_on)
    }

    /// Set Weak Light state
    pub fn set_weak_light(&mut self, enabled: bool) -> bool {
        self.is_weak_light_on = enabled;
        if self.is_weak_light_on {
            let s_val = ((self.settings.weak_light_percent / 100.0) * self.settings.max_s_value).max(1.0).round() as u32;
            // GRBL laser mode requires G1 motion state to emit power
            self.send_raw(&format!("M3 S{} G1 F100", s_val));
            self.log(&format!("[BEAM] Weak beam turned on for alignment ({:.1}% power, S{})", self.settings.weak_light_percent, s_val));
        } else {
            self.send_raw("M5");
            self.log("[BEAM] Weak beam turned off (M5).");
        }
        self.is_weak_light_on
    }

    /// Start Bounding Box Frame Test with Weak Light
    pub fn start_box_frame_test(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        let pts = vec![
            Point2D::new(min_x, min_y),
            Point2D::new(max_x, min_y),
            Point2D::new(max_x, max_y),
            Point2D::new(min_x, max_y),
            Point2D::new(min_x, min_y),
        ];

        self.start_points_frame_test(pts, "Box Framing");
    }

    /// Start Contour / Path Trace Test with Weak Light
    pub fn start_contour_frame_test(&mut self, path_points: Vec<Point2D>) {
        if path_points.is_empty() {
            self.log("[ERROR] No contour points to test.");
            return;
        }
        self.start_points_frame_test(path_points, "Contour Framing");
    }

    fn start_points_frame_test(&mut self, points: Vec<Point2D>, test_name: &str) {
        if points.is_empty() {
            return;
        }

        self.framing_points = points.into_iter().map(|p| self.doc_to_machine(p)).collect();
        self.framing_idx = 0;
        self.framing_target_sent = false;
        self.is_framing_active = true;

        // Turn on weak light
        self.set_weak_light(true);

        self.log(&format!("[TEST] {} started ({} points). Laser tracing contour with weak beam...", test_name, self.framing_points.len()));
    }

    /// Advance the test framing simulation / motion
    pub fn step_frame_test(&mut self) -> bool {
        if !self.is_framing_active || self.framing_points.is_empty() {
            return false;
        }

        self.poll_serial();

        if self.framing_idx < self.framing_points.len() {
            let target = self.framing_points[self.framing_idx];

            if !self.framing_target_sent {
                let feed = self.settings.frame_speed;
                self.target_x = target.x;
                self.target_y = target.y;
                let cmd_type = if self.framing_idx == 0 { "G0" } else { "G1" };
                self.send_raw(&format!("G90 {} X{:.3} Y{:.3} F{:.0}", cmd_type, target.x, target.y, feed));
                self.framing_target_sent = true;

                if !self.is_physically_connected() {
                    self.machine_x = target.x;
                    self.machine_y = target.y;
                    self.work_x = target.x;
                    self.work_y = target.y;
                }
            }

            // Check if head has arrived at target
            let arrived = if self.is_physically_connected() {
                let dist = ((self.machine_x - target.x).powi(2) + (self.machine_y - target.y).powi(2)).sqrt();
                dist < 1.0 || (self.grbl_state == "Idle" && self.framing_target_sent)
            } else {
                true
            };

            if arrived {
                self.framing_idx += 1;
                self.framing_target_sent = false;
            }
            true
        } else {
            // Finished framing test
            self.stop_frame_test();
            self.log("[TEST] ✅ Framing test completed successfully!");
            false
        }
    }

    /// Stop test framing and turn off weak light
    pub fn stop_frame_test(&mut self) {
        self.is_framing_active = false;
        self.framing_points.clear();
        self.framing_idx = 0;
        self.framing_target_sent = false;
        self.set_weak_light(false);
    }

    /// Step non-blocking abort sequence to flush planner buffer and preserve coordinate origin
    pub fn step_abort_sequence(&mut self) {
        if let Some(phase) = self.abort_phase {
            match phase {
                AbortSequencePhase::HoldIssued { stopped_x, stopped_y, started_at } => {
                    // Wait 40ms for physical deceleration to halt motion
                    if started_at.elapsed() >= Duration::from_millis(40) {
                        // Soft-reset GRBL (0x18) to flush planner buffer
                        self.send_realtime(0x18);
                        self.abort_phase = Some(AbortSequencePhase::ResetIssued {
                            stopped_x,
                            stopped_y,
                            started_at: Instant::now(),
                        });
                    }
                }
                AbortSequencePhase::ResetIssued { stopped_x, stopped_y, started_at } => {
                    // Wait 60ms for GRBL microcontroller to complete soft reset boot
                    if started_at.elapsed() >= Duration::from_millis(60) {
                        // Clear any alarm lock ($X), force laser off (M5), absolute mode (G90)
                        // and re-establish the coordinate reference (G92) so (0,0) remains the original origin!
                        self.send_raw("$X");
                        self.send_raw("M5");
                        self.send_raw("G90");
                        self.send_raw(&format!("G92 X{:.3} Y{:.3}", stopped_x, stopped_y));
                        self.abort_phase = Some(AbortSequencePhase::RestoreSent {
                            stopped_x,
                            stopped_y,
                            started_at: Instant::now(),
                        });
                        self.log(&format!("[CONTROLLER] Work coordinate origin (0,0) preserved. Laser head at X={:.2}, Y={:.2} mm.", stopped_x, stopped_y));
                    }
                }
                AbortSequencePhase::RestoreSent { stopped_x, stopped_y, started_at } => {
                    // Allow G92 command to be processed by controller
                    if started_at.elapsed() >= Duration::from_millis(100) {
                        self.abort_phase = None;
                        self.target_x = stopped_x;
                        self.target_y = stopped_y;
                        self.machine_x = stopped_x;
                        self.machine_y = stopped_y;
                        self.work_x = stopped_x;
                        self.work_y = stopped_y;
                        self.send_realtime(b'?');
                    }
                }
            }
        }
    }

    /// Starts physical mechanical limit switch homing and zero-point calibration.
    /// Drives X and Y sequentially towards the configured OriginCorner switches regardless of software limits,
    /// stops when the mechanical switches trip, pulls off to release contact, and sets G92 X0 Y0.
    pub fn start_mechanical_home_reset(&mut self) {
        if !self.settings.enable_limit_switches {
            self.log("[RESET] ⚠️ Physical limit switches disabled in settings (switches disabled for machine protection). Enable in: ⚙️ Settings.");
            return;
        }
        if self.homing_phase.is_some() {
            return;
        }

        // Halt any running cutting job or framing test
        self.is_job_running = false;
        self.is_job_paused = false;
        self.is_laser_firing = false;
        self.is_framing_active = false;
        self.job_lines.clear();
        self.set_weak_light(false);

        let corner = self.settings.origin_corner;

        if !self.is_physically_connected() {
            // Virtual Demo / Simulator mode:
            self.log(&format!("[RESET] 🔄 Virtual Mode: Moving head to {:?} origin switches (X0 Y0)...", corner));
            self.target_x = 0.0;
            self.target_y = 0.0;
            self.machine_x = 0.0;
            self.machine_y = 0.0;
            self.work_x = 0.0;
            self.work_y = 0.0;
            self.last_wco = (0.0, 0.0);
            self.homing_phase = None;
            return;
        }

        self.log(&format!("[RESET] 🔄 Starting hardware reset towards {:?} limit switches...", corner));
        if self.grbl_state.starts_with("Alarm") {
            self.send_raw("$X");
        }
        self.send_raw("M5");
        self.send_raw("$21=1");
        self.homing_phase = Some(SwitchHomingPhase::Starting { started_at: Instant::now() });
    }

    /// Step non-blocking mechanical limit switch homing & zero-point calibration sequence
    pub fn step_homing_sequence(&mut self) {
        let (dir_x, dir_y) = self.origin_corner_direction();
        let travel_x = (self.settings.bed_width * 1.2).max(100.0) * dir_x;
        let travel_y = (self.settings.bed_height * 1.2).max(100.0) * dir_y;
        let seek_feed = self.settings.homing_speed.clamp(100.0, 1500.0);
        let pull_feed = (seek_feed * 0.75).clamp(100.0, 400.0);
        let pull_x = -dir_x * 5.0;
        let pull_y = -dir_y * 5.0;

        let timeout_x_secs = (travel_x.abs() / (seek_feed / 60.0)).max(30.0) + 15.0;
        let timeout_y_secs = (travel_y.abs() / (seek_feed / 60.0)).max(30.0) + 15.0;

        if let Some(phase) = self.homing_phase {
            match phase {
                SwitchHomingPhase::Starting { started_at } => {
                    if started_at.elapsed() >= Duration::from_millis(100) {
                        self.log(&format!("[RESET] 🔄 Moving X axis towards limit switch ({:+.1} mm @ {:.0} mm/min)...", travel_x, seek_feed));
                        self.send_raw(&format!("G91 G1 X{:.1} F{:.0}", travel_x, seek_feed));
                        self.homing_phase = Some(SwitchHomingPhase::MovingX { started_at: Instant::now() });
                    }
                }
                SwitchHomingPhase::MovingX { started_at } => {
                    let switch_tripped = self.grbl_state.starts_with("Alarm") || self.pin_limit_x;
                    let timed_out = started_at.elapsed() >= Duration::from_secs_f64(timeout_x_secs);

                    if switch_tripped || timed_out {
                        self.send_realtime(0x18);
                        if switch_tripped {
                            self.log(&format!("[RESET] 🛑 X limit switch triggered (Pn:{}). Halting motion instantly with soft reset...", self.pin_state));
                        } else {
                            self.log(&format!("[RESET] ⚠️ Search distance exceeded ({:.0}s timeout). Halting motion...", timeout_x_secs));
                        }
                        self.homing_phase = Some(SwitchHomingPhase::ResettingX { started_at: Instant::now() });
                    }
                }
                SwitchHomingPhase::ResettingX { started_at } => {
                    // Wait 900ms for GRBL to finish soft-reset boot sequence before unlocking
                    if started_at.elapsed() >= Duration::from_millis(900) {
                        self.log("[RESET] 🔄 Microcontroller reboot complete. Unlocking ($X) and pulling off 5 mm...");
                        self.send_raw("$X");
                        self.send_raw(&format!("G91 G1 X{:.1} F{:.0}", pull_x, pull_feed));
                        self.homing_phase = Some(SwitchHomingPhase::PullingOffX { started_at: Instant::now() });
                    }
                }
                SwitchHomingPhase::PullingOffX { started_at } => {
                    let switch_released = !self.pin_limit_x;
                    let timed_out = started_at.elapsed() >= Duration::from_secs(5);
                    if started_at.elapsed() >= Duration::from_millis(1200) && (switch_released || timed_out) {
                        if !switch_released && timed_out {
                            self.log("[RESET] ⚠️ Warning: X limit switch pin still indicates closed after pull-off.");
                        }
                        self.log(&format!("[RESET] ✅ X axis switch disengaged. Moving Y axis towards limit switch ({:+.1} mm @ {:.0} mm/min)...", travel_y, seek_feed));
                        self.send_raw(&format!("G91 G1 Y{:.1} F{:.0}", travel_y, seek_feed));
                        self.homing_phase = Some(SwitchHomingPhase::MovingY { started_at: Instant::now() });
                    }
                }
                SwitchHomingPhase::MovingY { started_at } => {
                    let switch_tripped = self.grbl_state.starts_with("Alarm") || self.pin_limit_y;
                    let timed_out = started_at.elapsed() >= Duration::from_secs_f64(timeout_y_secs);

                    if switch_tripped || timed_out {
                        self.send_realtime(0x18);
                        if switch_tripped {
                            self.log(&format!("[RESET] 🛑 Y limit switch triggered (Pn:{}). Halting motion instantly with soft reset...", self.pin_state));
                        } else {
                            self.log(&format!("[RESET] ⚠️ Search distance exceeded ({:.0}s timeout). Halting motion...", timeout_y_secs));
                        }
                        self.homing_phase = Some(SwitchHomingPhase::ResettingY { started_at: Instant::now() });
                    }
                }
                SwitchHomingPhase::ResettingY { started_at } => {
                    // Wait 900ms for GRBL to finish soft-reset boot sequence before unlocking
                    if started_at.elapsed() >= Duration::from_millis(900) {
                        self.log("[RESET] 🔄 Microcontroller reboot complete. Unlocking ($X) and pulling off 5 mm...");
                        self.send_raw("$X");
                        self.send_raw(&format!("G91 G1 Y{:.1} F{:.0}", pull_y, pull_feed));
                        self.homing_phase = Some(SwitchHomingPhase::PullingOffY { started_at: Instant::now() });
                    }
                }
                SwitchHomingPhase::PullingOffY { started_at } => {
                    let switch_released = !self.pin_limit_y;
                    let timed_out = started_at.elapsed() >= Duration::from_secs(5);
                    if started_at.elapsed() >= Duration::from_millis(1200) && (switch_released || timed_out) {
                        if !switch_released && timed_out {
                            self.log("[RESET] ⚠️ Warning: Y limit switch pin still indicates closed after pull-off.");
                        }
                        self.send_raw("M5");
                        self.send_raw("G90");
                        self.send_raw("G92 X0 Y0");
                        self.homing_phase = Some(SwitchHomingPhase::Finalizing { started_at: Instant::now() });
                    }
                }
                SwitchHomingPhase::Finalizing { started_at } => {
                    if started_at.elapsed() >= Duration::from_millis(200) {
                        self.homing_phase = None;
                        self.machine_x = 0.0;
                        self.machine_y = 0.0;
                        self.work_x = 0.0;
                        self.work_y = 0.0;
                        self.target_x = 0.0;
                        self.target_y = 0.0;
                        self.last_wco = (0.0, 0.0);
                        self.send_realtime(b'?');
                        self.log("[RESET] 🎯 Zero point (0,0) successfully calibrated and saved at limit switches!");
                    }
                }
            }
        }
    }

    /// Poll serial port for incoming responses (non-blocking) and query status
    pub fn poll_serial(&mut self) {
        self.step_abort_sequence();
        self.step_homing_sequence();

        if self.is_physically_connected() {
            let now = Instant::now();
            let poll_interval = if self.is_job_running || self.is_framing_active || self.homing_phase.is_some() || self.grbl_state == "Run" || self.grbl_state == "Jog" || self.grbl_state == "Home" {
                Duration::from_millis(50)
            } else {
                Duration::from_millis(150)
            };

            if self.last_status_poll.map_or(true, |t| now.duration_since(t) >= poll_interval) {
                self.last_status_poll = Some(now);
                self.send_realtime(b'?');
            }

            let mut link_closed = false;
            if let Some(ref mut port) = self.active_port {
                match port.read_available() {
                    Ok(bytes) => {
                        if !bytes.is_empty() {
                            let s = String::from_utf8_lossy(&bytes);
                            self.rx_buffer.push_str(&s);
                            self.raw_subscribers.retain(|tx| tx.send(bytes.clone()).is_ok());
                        }
                    }
                    Err(()) => link_closed = true,
                }
            }
            if link_closed {
                self.log("[CONNECTION] ⚠️ Connection lost (network link closed).");
                self.disconnect();
                return;
            }

            // Process complete lines from rx_buffer
            while let Some(idx) = self.rx_buffer.find(|c| c == '\n' || c == '\r') {
                let line = self.rx_buffer[..idx].trim().to_string();
                self.rx_buffer = self.rx_buffer[idx + 1..].to_string();
                if line.is_empty() {
                    continue;
                }
                if line.starts_with('<') && line.ends_with('>') {
                    self.parse_grbl_status(&line);
                    if self.settings.log_status_polls {
                        self.log(&format!("< {}", line));
                    }
                } else {
                    self.log(&format!("< {}", line));
                    if line == "ok" {
                        self.pending_ok_count = self.pending_ok_count.saturating_sub(1);
                    } else if line.contains("ALARM:1") || line.contains("ALARM") {
                        self.grbl_state = "Alarm".to_string();

                        if matches!(self.homing_phase, Some(SwitchHomingPhase::MovingX { .. })) {
                            self.log("[RESET] X limit switch triggered (ALARM). Halting motion instantly with soft reset...");
                            self.send_realtime(0x18);
                            self.homing_phase = Some(SwitchHomingPhase::ResettingX { started_at: Instant::now() });
                        } else if matches!(self.homing_phase, Some(SwitchHomingPhase::MovingY { .. })) {
                            self.log("[RESET] Y limit switch triggered (ALARM). Halting motion instantly with soft reset...");
                            self.send_realtime(0x18);
                            self.homing_phase = Some(SwitchHomingPhase::ResettingY { started_at: Instant::now() });
                        }
                    }
                }
            }

            if self.rx_buffer.len() > 2048 {
                self.rx_buffer.clear();
            }
        }
    }

    /// Parse GRBL real-time status report `<State|MPos:X,Y,Z|...>`
    pub fn parse_grbl_status(&mut self, status_line: &str) {
        let inner = &status_line[1..status_line.len() - 1];
        let parts: Vec<&str> = inner.split('|').collect();
        if parts.is_empty() {
            return;
        }

        self.grbl_state = parts[0].to_string();

        let mut parsed_mpos = None;
        let mut parsed_wpos = None;
        let mut found_pn = false;

        for part in &parts[1..] {
            if let Some(coords_str) = part.strip_prefix("MPos:") {
                let coords: Vec<&str> = coords_str.split(',').collect();
                if coords.len() >= 2 {
                    if let (Ok(x), Ok(y)) = (coords[0].parse::<f64>(), coords[1].parse::<f64>()) {
                        parsed_mpos = Some((x, y));
                    }
                }
            } else if let Some(coords_str) = part.strip_prefix("WPos:") {
                let coords: Vec<&str> = coords_str.split(',').collect();
                if coords.len() >= 2 {
                    if let (Ok(x), Ok(y)) = (coords[0].parse::<f64>(), coords[1].parse::<f64>()) {
                        parsed_wpos = Some((x, y));
                    }
                }
            } else if let Some(coords_str) = part.strip_prefix("WCO:") {
                let coords: Vec<&str> = coords_str.split(',').collect();
                if coords.len() >= 2 {
                    if let (Ok(x), Ok(y)) = (coords[0].parse::<f64>(), coords[1].parse::<f64>()) {
                        self.last_wco = (x, y);
                    }
                }
            } else if let Some(pn_str) = part.strip_prefix("Pn:") {
                found_pn = true;
                if self.pin_state != pn_str {
                    self.log(&format!("[PIN] ⚡ Switch state changed: Pn:{}", pn_str));
                }
                self.pin_state = pn_str.to_string();
                self.pin_limit_x = pn_str.contains('X');
                self.pin_limit_y = pn_str.contains('Y');
                self.pin_limit_z = pn_str.contains('Z');
                self.pin_probe = pn_str.contains('P');
            } else if let Some(fs_str) = part.strip_prefix("FS:") {
                let fs: Vec<&str> = fs_str.split(',').collect();
                if fs.len() >= 2 {
                    if let Ok(s) = fs[1].parse::<f64>() {
                        if s > 0.0 && (self.grbl_state == "Run" || self.grbl_state == "Jog") {
                            self.is_laser_firing = true;
                        } else if s == 0.0 {
                            self.is_laser_firing = false;
                        }
                    }
                }
            }
        }

        if !found_pn {
            self.pin_state.clear();
            self.pin_limit_x = false;
            self.pin_limit_y = false;
            self.pin_limit_z = false;
            self.pin_probe = false;
        }

        // Determine real-time position
        let (raw_x, raw_y) = if let Some((wx, wy)) = parsed_wpos {
            (wx, wy)
        } else if let Some((mx, my)) = parsed_mpos {
            (mx - self.last_wco.0, my - self.last_wco.1)
        } else {
            return;
        };

        // Coordinates in machine bed workspace:
        let real_x = raw_x.abs().clamp(0.0, self.settings.bed_width);
        let real_y = raw_y.abs().clamp(0.0, self.settings.bed_height);

        // Protection during stop / abort sequence:
        // When GRBL resets, it briefly reports 0,0 before our G92 restore takes effect.
        // Prevent that transient 0,0 from overwriting our preserved coordinates!
        if let Some(phase) = self.abort_phase {
            let (sx, sy) = match phase {
                AbortSequencePhase::HoldIssued { stopped_x, stopped_y, .. } => (stopped_x, stopped_y),
                AbortSequencePhase::ResetIssued { stopped_x, stopped_y, .. } => (stopped_x, stopped_y),
                AbortSequencePhase::RestoreSent { stopped_x, stopped_y, .. } => (stopped_x, stopped_y),
            };
            if real_x.abs() < 0.05 && real_y.abs() < 0.05 && (sx.abs() > 0.1 || sy.abs() > 0.1) {
                return;
            }
        }

        self.reported_x = real_x;
        self.reported_y = real_y;
        self.machine_x = real_x;
        self.machine_y = real_y;
        self.work_x = real_x;
        self.work_y = real_y;
    }

    /// Returns true if machine is physically moving or simulating motion
    pub fn is_moving(&self) -> bool {
        self.grbl_state == "Run"
            || self.grbl_state == "Jog"
            || self.grbl_state == "Home"
            || self.is_framing_active
            || self.is_job_running
            || self.homing_phase.is_some()
            || (self.machine_x - self.target_x).abs() > 0.2
            || (self.machine_y - self.target_y).abs() > 0.2
    }

    /// Simulates motion when offline so crosshair glides smoothly at real feed speed
    pub fn update_motion_simulation(&mut self, dt: f64) {
        if !self.is_physically_connected() {
            let dx = self.target_x - self.machine_x;
            let dy = self.target_y - self.machine_y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > 0.05 {
                let speed_mm_s = if self.is_framing_active {
                    self.settings.frame_speed / 60.0
                } else if self.is_job_running {
                    self.settings.rapid_speed / 60.0
                } else {
                    self.settings.jog_speed / 60.0
                };
                let step = speed_mm_s * dt;
                if step >= dist {
                    self.machine_x = self.target_x;
                    self.machine_y = self.target_y;
                } else {
                    self.machine_x += (dx / dist) * step;
                    self.machine_y += (dy / dist) * step;
                }
                self.work_x = self.machine_x;
                self.work_y = self.machine_y;
            } else {
                self.machine_x = self.target_x;
                self.machine_y = self.target_y;
                self.work_x = self.machine_x;
                self.work_y = self.machine_y;
            }
        }
    }

    /// Start executing a G-code job
    pub fn start_job(&mut self, gcode: &str, estimated_time_sec: f64) {
        self.job_lines = gcode
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with(';'))
            .collect();
        self.job_idx = 0;
        self.pending_ok_count = 0;
        self.job_total_lines = self.job_lines.len();
        self.is_job_running = !self.job_lines.is_empty();
        self.is_job_paused = false;
        self.job_progress = 0.0;
        self.job_start_instant = Some(Instant::now());
        self.job_estimated_total_sec = estimated_time_sec;
        let est_str = crate::simulator::Simulator::format_duration(estimated_time_sec);
        self.log(&format!("[JOB] Cut job started ({} G-code lines, ~{}).", self.job_total_lines, est_str));

        #[cfg(target_os = "macos")]
        if self.settings.run_shortcut_on_start {
            let name = self.settings.shortcut_on_start_name.trim().to_string();
            if !name.is_empty() {
                self.trigger_macos_shortcut(&name, "On Job Start");
            }
        }
    }

    /// Queries the macOS `shortcuts list` CLI to return available Apple Shortcuts on this Mac.
    #[cfg(target_os = "macos")]
    pub fn list_macos_shortcuts() -> Vec<String> {
        if let Ok(output) = std::process::Command::new("shortcuts").arg("list").output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                return text
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
            }
        }
        Vec::new()
    }

    /// Triggers an Apple Shortcut asynchronously in the background on macOS (`shortcuts run "<name>"`).
    #[cfg(target_os = "macos")]
    pub fn trigger_macos_shortcut(&mut self, shortcut_name: &str, reason: &str) {
        let trimmed = shortcut_name.trim();
        if trimmed.is_empty() {
            return;
        }
        self.log(&format!("[SHORTCUT] Running Apple Shortcut '{}' ({})...", trimmed, reason));
        if !cfg!(test) {
            let _ = std::process::Command::new("shortcuts")
                .arg("run")
                .arg(trimmed)
                .spawn();
        }
    }

    /// Pause the active job
    pub fn pause_job(&mut self) {
        if self.is_job_running && !self.is_job_paused {
            self.is_job_paused = true;
            self.is_laser_firing = false;
            // Real-time feed hold command '!' and laser off M5
            self.send_realtime(b'!');
            self.send_raw("M5");
            self.log("[JOB] Job paused.");
        }
    }

    /// Resume the paused job
    pub fn resume_job(&mut self) {
        if self.is_job_running && self.is_job_paused {
            self.is_job_paused = false;
            // Real-time cycle start command '~'
            self.send_realtime(b'~');
            self.log("[JOB] Job resumed.");
        }
    }

    /// Cleanly and immediately stop the active laser job without losing or resetting the coordinate origin (0,0).
    pub fn stop_job(&mut self) {
        let (stopped_x, stopped_y) = match self.abort_phase {
            Some(AbortSequencePhase::HoldIssued { stopped_x, stopped_y, .. })
            | Some(AbortSequencePhase::ResetIssued { stopped_x, stopped_y, .. })
            | Some(AbortSequencePhase::RestoreSent { stopped_x, stopped_y, .. }) => (stopped_x, stopped_y),
            None => (self.work_x, self.work_y),
        };

        // 1. Immediately halt motion and laser via GRBL real-time Feed Hold character '!' (0x21).
        // Real-time byte bypasses serial buffer and decelerates axes to a complete stop instantly.
        self.send_realtime(b'!');

        // 2. Clear job streaming queue so no further G-code lines are sent
        self.is_job_running = false;
        self.is_job_paused = false;
        self.is_laser_firing = false;
        if self.homing_phase.is_some() {
            self.send_raw("$21=1");
        }
        self.homing_phase = None;
        self.job_lines.clear();
        self.job_idx = 0;
        self.pending_ok_count = 0;
        self.job_progress = 0.0;
        self.job_start_instant = None;

        // 3. Keep target and coordinates anchored at the exact stopped location
        self.target_x = stopped_x;
        self.target_y = stopped_y;
        self.machine_x = stopped_x;
        self.machine_y = stopped_y;
        self.work_x = stopped_x;
        self.work_y = stopped_y;

        if self.is_physically_connected() {
            // Begin safe buffer-flush and coordinate-restoration sequence
            self.abort_phase = Some(AbortSequencePhase::HoldIssued {
                stopped_x,
                stopped_y,
                started_at: Instant::now(),
            });
            self.log(&format!("[JOB] Emergency stop at X={:.2}, Y={:.2} mm. Halting laser and preserving origin...", stopped_x, stopped_y));
        } else {
            self.abort_phase = None;
            self.log(&format!("[JOB] Job stopped at X={:.2}, Y={:.2} mm. Origin (0,0) preserved.", stopped_x, stopped_y));
        }
    }

    /// Step streaming the active job
    pub fn step_job(&mut self) -> bool {
        if !self.is_job_running || self.is_job_paused || self.job_lines.is_empty() {
            return false;
        }

        self.poll_serial();

        if self.is_physically_connected() {
            while self.pending_ok_count < 3 && self.job_idx < self.job_lines.len() {
                let line = self.job_lines[self.job_idx].clone();
                self.current_command = line.clone();
                self.send_raw(&line);
                self.pending_ok_count += 1;

                // Update laser firing state
                if line.contains("M3") || line.contains("M4") {
                    self.is_laser_firing = true;
                } else if line.contains("M5") {
                    self.is_laser_firing = false;
                }

                self.job_idx += 1;
                self.job_progress = (self.job_idx as f32) / (self.job_total_lines as f32);
            }
        } else {
            // Offline / Virtual Demo simulation:
            if self.job_idx < self.job_lines.len() {
                let line = self.job_lines[self.job_idx].clone();
                self.current_command = line.clone();
                self.send_raw(&line);

                if line.contains("M3") || line.contains("M4") {
                    self.is_laser_firing = true;
                } else if line.contains("M5") {
                    self.is_laser_firing = false;
                }

                let upper = line.to_uppercase();
                if upper.starts_with("G0") || upper.starts_with("G1") || upper.starts_with("G00") || upper.starts_with("G01") {
                    for part in upper.split_whitespace() {
                        if part.starts_with('X') {
                            if let Ok(val) = part[1..].parse::<f64>() {
                                self.target_x = val;
                            }
                        } else if part.starts_with('Y') {
                            if let Ok(val) = part[1..].parse::<f64>() {
                                self.target_y = val;
                            }
                        }
                    }
                }

                self.machine_x = self.target_x;
                self.machine_y = self.target_y;
                self.work_x = self.machine_x;
                self.work_y = self.machine_y;

                self.job_idx += 1;
                self.job_progress = (self.job_idx as f32) / (self.job_total_lines as f32);
            }
        }

        if self.job_idx >= self.job_lines.len() {
            let is_done = if self.is_physically_connected() {
                self.pending_ok_count == 0 && self.grbl_state == "Idle"
            } else {
                true
            };

            if is_done {
                self.is_job_running = false;
                self.is_laser_firing = false;
                self.send_raw("M5");
                let dur_str = if let Some(st) = self.job_start_instant.take() {
                    format!(" in {}", crate::simulator::Simulator::format_duration(st.elapsed().as_secs_f64()))
                } else {
                    String::new()
                };
                self.log(&format!("[JOB] Job completed successfully{}!", dur_str));

                #[cfg(target_os = "macos")]
                if self.settings.run_shortcut_on_complete {
                    let name = self.settings.shortcut_on_complete_name.trim().to_string();
                    if !name.is_empty() {
                        self.trigger_macos_shortcut(&name, "On Job Complete");
                    }
                }

                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_init_and_jog() {
        let mut dm = DeviceManager::new();
        dm.settings = DeviceSettings::default();
        assert_eq!(dm.status, ConnectionStatus::Disconnected);
        assert!(!dm.is_physically_connected());
        assert_eq!(dm.machine_x, 0.0);
        assert_eq!(dm.machine_y, 0.0);

        // Connect to virtual demo for offline simulation
        let res = dm.connect("Virtuaaliportti (Demo / Offline)", 115200);
        assert!(res.is_ok());
        assert_eq!(dm.status, ConnectionStatus::Connected);

        dm.jog(15.0, 25.0, 3000.0);
        assert_eq!(dm.machine_x, 15.0);
        assert_eq!(dm.machine_y, 25.0);

        dm.jog(-5.0, -10.0, 3000.0);
        assert_eq!(dm.machine_x, 10.0);
        assert_eq!(dm.machine_y, 15.0);

        // Without limit switches enabled, home() protects the machine and does not move
        dm.home();
        assert_eq!(dm.machine_x, 10.0);

        // With limit switches enabled, home() resets to origin
        dm.settings.enable_limit_switches = true;
        dm.home();
        assert_eq!(dm.machine_x, 0.0);
        assert_eq!(dm.machine_y, 0.0);
    }

    #[test]
    fn test_weak_light_toggle() {
        let mut dm = DeviceManager::new();
        assert!(!dm.is_weak_light_on);

        dm.toggle_weak_light();
        assert!(dm.is_weak_light_on);

        dm.toggle_weak_light();
        assert!(!dm.is_weak_light_on);
    }

    #[test]
    fn test_box_frame_test() {
        let mut dm = DeviceManager::new();
        dm.start_box_frame_test(10.0, 10.0, 100.0, 80.0);
        assert!(dm.is_framing_active);
        assert!(dm.is_weak_light_on);

        while dm.is_framing_active {
            dm.step_frame_test();
        }

        assert!(!dm.is_framing_active);
        assert!(!dm.is_weak_light_on);
    }

    #[test]
    fn test_job_execution() {
        let mut dm = DeviceManager::new();
        let gcode = "G21\nG90\nG0 X15.0 Y25.0\nM4 S600\nG1 X45.0 Y65.0 F3000\nM5\n";
        dm.start_job(gcode, 5.0);
        assert!(dm.is_job_running);
        assert!(!dm.is_job_paused);
        assert_eq!(dm.job_total_lines, 6);

        // Advance job
        while dm.is_job_running {
            dm.step_job();
        }

        assert!(!dm.is_job_running);
        assert_eq!(dm.machine_x, 45.0);
        assert_eq!(dm.machine_y, 65.0);
        assert!(!dm.is_laser_firing);
    }

    #[test]
    fn test_doc_to_machine_mapping() {
        let mut dm = DeviceManager::new();
        dm.settings.bed_width = 465.0;
        dm.settings.bed_height = 440.0;

        // 1. TopRight: Machine (0,0) is Doc (465, 0)
        dm.settings.origin_corner = OriginCorner::TopRight;
        let doc_pt = Point2D::new(200.0, 220.0);
        let m_pt = dm.doc_to_machine(doc_pt);
        assert_eq!(m_pt.x, 265.0);
        assert_eq!(m_pt.y, 220.0);
        assert_eq!(dm.doc_to_machine(Point2D::new(465.0, 0.0)), Point2D::new(0.0, 0.0));
        assert_eq!(dm.machine_to_doc(Point2D::new(0.0, 0.0)), Point2D::new(465.0, 0.0));

        // 2. TopLeft: Machine (0,0) is Doc (0, 0)
        dm.settings.origin_corner = OriginCorner::TopLeft;
        assert_eq!(dm.doc_to_machine(Point2D::new(0.0, 0.0)), Point2D::new(0.0, 0.0));
        assert_eq!(dm.machine_to_doc(Point2D::new(0.0, 0.0)), Point2D::new(0.0, 0.0));

        // 3. BottomLeft: Machine (0,0) is Doc (0, 440)
        dm.settings.origin_corner = OriginCorner::BottomLeft;
        assert_eq!(dm.doc_to_machine(Point2D::new(0.0, 440.0)), Point2D::new(0.0, 0.0));
        assert_eq!(dm.machine_to_doc(Point2D::new(0.0, 0.0)), Point2D::new(0.0, 440.0));

        // 4. BottomRight: Machine (0,0) is Doc (465, 440)
        dm.settings.origin_corner = OriginCorner::BottomRight;
        assert_eq!(dm.doc_to_machine(Point2D::new(465.0, 440.0)), Point2D::new(0.0, 0.0));
        assert_eq!(dm.machine_to_doc(Point2D::new(0.0, 0.0)), Point2D::new(465.0, 440.0));
    }

    #[test]
    fn test_grbl_status_parsing() {
        let mut dm = DeviceManager::new();
        dm.parse_grbl_status("<Run|MPos:15.250,30.500,0.000|FS:2000,500>");
        assert_eq!(dm.grbl_state, "Run");
        assert!((dm.machine_x - 15.250).abs() < 1e-3);
        assert!((dm.machine_y - 30.500).abs() < 1e-3);
        assert!(dm.is_laser_firing);

        dm.parse_grbl_status("<Idle|MPos:0.000,0.000,0.000|FS:0,0|WCO:0.000,0.000,0.000>");
        assert_eq!(dm.grbl_state, "Idle");
        assert_eq!(dm.machine_x, 0.0);
        assert_eq!(dm.machine_y, 0.0);
        assert!(!dm.is_laser_firing);
    }

    #[test]
    fn test_stop_job_preserves_origin_and_coordinates() {
        let mut dm = DeviceManager::new();
        dm.settings.custom_boundaries = CustomBoundaries::default();
        dm.disconnect();
        // Move to some position and start job
        dm.machine_x = 10.0;
        dm.machine_y = 10.0;
        dm.work_x = 10.0;
        dm.work_y = 10.0;

        let gcode = "G0 X30.0 Y40.0\nM4 S500\nG1 X60.0 Y80.0 F2000\nM5\n";
        dm.start_job(gcode, 10.0);
        assert!(dm.is_job_running);

        // Step job until mid-execution
        dm.step_job();
        dm.step_job();
        assert_eq!(dm.machine_x, 30.0);
        assert_eq!(dm.machine_y, 40.0);

        // User presses STOP!
        dm.stop_job();

        // Job must be stopped and laser off
        assert!(!dm.is_job_running);
        assert!(!dm.is_laser_firing);

        // Crucial: coordinates MUST remain at stopped location (30.0, 40.0), NOT reset to 0,0!
        assert_eq!(dm.machine_x, 30.0);
        assert_eq!(dm.machine_y, 40.0);
        assert_eq!(dm.work_x, 30.0);
        assert_eq!(dm.work_y, 40.0);

        // Moving to origin returns to (0, 0) relative to original origin
        dm.go_to_origin();
        assert_eq!(dm.target_x, 0.0);
        assert_eq!(dm.target_y, 0.0);
    }

    #[test]
    fn test_abort_sequence_phases_and_grbl_zero_protection() {
        let mut dm = DeviceManager::new();
        dm.work_x = 75.3;
        dm.work_y = 42.1;
        dm.machine_x = 75.3;
        dm.machine_y = 42.1;

        // Simulate physical abort phase
        dm.abort_phase = Some(AbortSequencePhase::ResetIssued {
            stopped_x: 75.3,
            stopped_y: 42.1,
            started_at: Instant::now(),
        });

        // While GRBL is soft-resetting, it reports 0,0,0:
        dm.parse_grbl_status("<Idle|MPos:0.000,0.000,0.000|FS:0,0|WCO:0.000,0.000,0.000>");

        // Protection check: coordinates MUST NOT be overwritten with 0.0!
        assert_eq!(dm.work_x, 75.3);
        assert_eq!(dm.machine_x, 75.3);

        // Once G92 has restored position, GRBL reports WCO or WPos:
        dm.abort_phase = None;
        dm.parse_grbl_status("<Idle|WPos:75.300,42.100,0.000|FS:0,0>");
        assert_eq!(dm.work_x, 75.3);
        assert_eq!(dm.machine_x, 75.3);
    }

    #[test]
    fn test_mechanical_home_reset_sequence() {
        let mut dm = DeviceManager::new();
        dm.settings = DeviceSettings::default();
        dm.machine_x = 120.0;
        dm.machine_y = 80.0;
        dm.work_x = 120.0;
        dm.work_y = 80.0;

        // When switches are disabled (safe default), reset does nothing:
        assert!(!dm.settings.enable_limit_switches);
        dm.start_mechanical_home_reset();
        assert_eq!(dm.machine_x, 120.0);
        assert_eq!(dm.work_x, 120.0);

        // When enabled by user in settings:
        dm.settings.enable_limit_switches = true;
        // Offline mode: resets immediately
        dm.start_mechanical_home_reset();
        assert_eq!(dm.machine_x, 0.0);
        assert_eq!(dm.machine_y, 0.0);
        assert_eq!(dm.work_x, 0.0);
        assert_eq!(dm.work_y, 0.0);
        assert!(dm.homing_phase.is_none());

        // Test physical state machine transitions:
        dm.machine_x = 50.0;
        dm.machine_y = 50.0;
        dm.homing_phase = Some(SwitchHomingPhase::Starting {
            started_at: Instant::now() - Duration::from_millis(150),
        });
        dm.step_homing_sequence();
        assert!(matches!(dm.homing_phase, Some(SwitchHomingPhase::MovingX { .. })));

        // X switch hit (Alarm):
        dm.grbl_state = "Alarm".to_string();
        dm.step_homing_sequence();
        assert!(matches!(dm.homing_phase, Some(SwitchHomingPhase::ResettingX { .. })));

        // After soft-reset reboot delay (900ms), transitions to PullingOffX:
        dm.homing_phase = Some(SwitchHomingPhase::ResettingX {
            started_at: Instant::now() - Duration::from_millis(1000),
        });
        dm.step_homing_sequence();
        assert!(matches!(dm.homing_phase, Some(SwitchHomingPhase::PullingOffX { .. })));

        // After X pull-off delay, moves to Y axis:
        dm.homing_phase = Some(SwitchHomingPhase::PullingOffX {
            started_at: Instant::now() - Duration::from_millis(1500),
        });
        dm.step_homing_sequence();
        assert!(matches!(dm.homing_phase, Some(SwitchHomingPhase::MovingY { .. })));

        // Y switch hit (Alarm):
        dm.grbl_state = "Alarm".to_string();
        dm.step_homing_sequence();
        assert!(matches!(dm.homing_phase, Some(SwitchHomingPhase::ResettingY { .. })));

        // After soft-reset reboot delay (900ms), transitions to PullingOffY:
        dm.homing_phase = Some(SwitchHomingPhase::ResettingY {
            started_at: Instant::now() - Duration::from_millis(1000),
        });
        dm.step_homing_sequence();
        assert!(matches!(dm.homing_phase, Some(SwitchHomingPhase::PullingOffY { .. })));

        // After Y pull-off delay, transitions to Finalizing:
        dm.homing_phase = Some(SwitchHomingPhase::PullingOffY {
            started_at: Instant::now() - Duration::from_millis(1500),
        });
        dm.step_homing_sequence();
        assert!(matches!(dm.homing_phase, Some(SwitchHomingPhase::Finalizing { .. })));

        // After finalizing delay, origin is calibrated and saved as (0, 0):
        dm.homing_phase = Some(SwitchHomingPhase::Finalizing {
            started_at: Instant::now() - Duration::from_millis(250),
        });
        dm.step_homing_sequence();
        assert!(dm.homing_phase.is_none());
        assert_eq!(dm.machine_x, 0.0);
        assert_eq!(dm.machine_y, 0.0);
        assert_eq!(dm.work_x, 0.0);
        assert_eq!(dm.work_y, 0.0);
    }

    #[test]
    fn test_custom_learned_boundaries_and_jog_enforcement() {
        let mut dm = DeviceManager::new();
        dm.settings.custom_boundaries = CustomBoundaries::default();
        dm.target_x = 50.0;
        dm.target_y = 50.0;
        dm.work_x = 50.0;
        dm.work_y = 50.0;

        // Teach 4 corners
        // Bottom-Left at (10, 10)
        dm.target_x = 10.0;
        dm.target_y = 10.0;
        dm.work_x = 10.0;
        dm.work_y = 10.0;
        dm.save_boundary_corner("bottom_left");

        // Top-Right at (200, 180)
        dm.target_x = 200.0;
        dm.target_y = 180.0;
        dm.work_x = 200.0;
        dm.work_y = 180.0;
        dm.save_boundary_corner("top_right");

        assert!(dm.settings.custom_boundaries.enabled);
        assert_eq!(dm.settings.custom_boundaries.min_x, Some(10.0));
        assert_eq!(dm.settings.custom_boundaries.max_x, Some(200.0));
        assert_eq!(dm.settings.custom_boundaries.min_y, Some(10.0));
        assert_eq!(dm.settings.custom_boundaries.max_y, Some(180.0));

        // Test jogging within boundaries
        dm.target_x = 100.0;
        dm.target_y = 100.0;
        dm.work_x = 100.0;
        dm.work_y = 100.0;
        dm.jog(10.0, 10.0, 3000.0);
        assert_eq!(dm.target_x, 110.0);
        assert_eq!(dm.target_y, 110.0);

        // Test jog clamped at boundary
        dm.target_x = 195.0;
        dm.target_y = 175.0;
        dm.work_x = 195.0;
        dm.work_y = 175.0;
        dm.jog(15.0, 15.0, 3000.0);
        assert_eq!(dm.target_x, 200.0);
        assert_eq!(dm.target_y, 180.0);

        // Test go to taught corners
        dm.go_to_corner("bottom_left");
        assert_eq!(dm.target_x, 10.0);
        assert_eq!(dm.target_y, 10.0);

        dm.go_to_corner("top_right");
        assert_eq!(dm.target_x, 200.0);
        assert_eq!(dm.target_y, 180.0);

        // Test move exceeding max X (attempt to move to 250 -> clamped to 200)
        dm.jog(140.0, 0.0, 3000.0);
        assert_eq!(dm.target_x, 200.0);

        // Attempting to move past 200.0 is blocked completely
        dm.jog(10.0, 0.0, 3000.0);
        assert_eq!(dm.target_x, 200.0);

        // Clear boundaries
        dm.clear_boundary_corners();
        assert!(!dm.settings.custom_boundaries.enabled);
        assert!(dm.settings.custom_boundaries.min_x.is_none());
    }

    #[test]
    fn test_device_profiles_management() {
        let mut settings = DeviceSettings::default();
        assert!(!settings.profiles.is_empty());
        assert_eq!(settings.current_profile_name, "Oma 10W (Standard)");

        // Switch to CO2 Laser profile
        assert!(settings.apply_profile("Large Format / CO2 (600x400)"));
        assert_eq!(settings.bed_width, 600.0);
        assert_eq!(settings.bed_height, 400.0);
        assert_eq!(settings.laser_type, LaserType::Co2);
        assert_eq!(settings.origin_corner, OriginCorner::TopLeft);
        assert_eq!(settings.current_profile_name, "Large Format / CO2 (600x400)");

        // Modify and save as new custom profile
        settings.bed_width = 300.0;
        settings.bed_height = 300.0;
        settings.save_current_as_profile("Workshop Custom 300");
        assert_eq!(settings.current_profile_name, "Workshop Custom 300");
        assert!(settings.profiles.iter().any(|p| p.name == "Workshop Custom 300"));

        // Switch back to Oma 10W
        assert!(settings.apply_profile("Oma 10W (Standard)"));
        assert_eq!(settings.bed_width, 400.0);

        // Delete the custom profile
        assert!(settings.delete_profile("Workshop Custom 300"));
        assert!(!settings.profiles.iter().any(|p| p.name == "Workshop Custom 300"));

        // Non-existent profile fails gracefully
        assert!(!settings.apply_profile("NonExistentMachine"));
    }

    #[test]
    fn test_network_port_parsing() {
        assert_eq!(parse_network_port("tcp://photoncam.local"), Some("photoncam.local:2323".to_string()));
        assert_eq!(parse_network_port("tcp://192.168.68.90:2323"), Some("192.168.68.90:2323".to_string()));
        assert_eq!(parse_network_port("tcp://"), None);
        assert_eq!(parse_network_port("/dev/ttyACM0"), None);
    }

    #[test]
    fn test_network_link_roundtrip() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 256];
            while let Ok(n) = conn.read(&mut buf) {
                if n == 0 { break; }
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                if text.contains('?') {
                    let _ = conn.write_all(b"<Idle|MPos:1.000,2.000,0.000|FS:0,0>\r\n");
                }
                if text.contains("G0") {
                    let _ = conn.write_all(b"ok\r\n");
                }
            }
        });
        let mut dm = DeviceManager::new();
        dm.settings.auto_connect = false;
        let res = dm.connect(&format!("tcp://127.0.0.1:{}", port), 115200);
        assert!(res.is_ok(), "{:?}", res);
        assert!(dm.is_physically_connected());
        dm.send_raw("G0 X1 Y2");
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            dm.poll_serial();
            if dm.console_history.iter().any(|l| l.trim() == "< ok") { break; }
        }
        assert!(dm.console_history.iter().any(|l| l.trim() == "< ok"), "no ok received over network link");
        assert!((dm.machine_x - 1.0).abs() < 1e-6);
        dm.disconnect();
        assert!(!dm.is_physically_connected());
    }

    #[test]
    fn test_network_connect_failure_reports_error() {
        let mut dm = DeviceManager::new();
        dm.settings.auto_connect = false;
        let res = dm.connect("tcp://127.0.0.1:1", 115200);
        assert!(res.is_err());
        assert!(!dm.is_physically_connected());
    }
}

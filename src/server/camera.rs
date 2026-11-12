//! Raspberry Pi & Web Remote Camera Bridge
//!
//! Captures bed snapshots and streams video using native Raspberry Pi OS camera tools
//! (`rpicam-still`, `libcamera-still`), USB webcams via V4L2 (`fswebcam`, `ffmpeg`),
//! or a synthetic calibration pattern when running offline or in development.

use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraDriver {
    /// Official Raspberry Pi OS Bookworm CSI camera stack (`rpicam-still`)
    RpiCam,
    /// Legacy Raspberry Pi CSI camera tool (`libcamera-still`)
    LibCamera,
    /// Linux V4L2 USB webcam capture via `fswebcam`
    FsWebcam,
    /// Linux V4L2 USB webcam capture via `ffmpeg`
    Ffmpeg,
    /// Synthetic test pattern for development, unit tests, or headless mode without camera
    TestPattern,
}

impl CameraDriver {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RpiCam => "rpicam-still (Raspberry Pi CSI)",
            Self::LibCamera => "libcamera-still (Raspberry Pi CSI)",
            Self::FsWebcam => "fswebcam (V4L2 USB)",
            Self::Ffmpeg => "ffmpeg (V4L2 USB)",
            Self::TestPattern => "Synthetic Test Pattern (Demo)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraStatus {
    pub available: bool,
    pub driver: String,
    pub device: String,
    pub width: u32,
    pub height: u32,
    pub cached: bool,
    pub timestamp_ms: u64,
}

pub struct CameraBridge {
    driver: Mutex<Option<CameraDriver>>,
    cached_jpeg: Mutex<Option<(Vec<u8>, Instant)>>,
    cache_duration: Duration,
    width: u32,
    height: u32,
}

impl Default for CameraBridge {
    fn default() -> Self {
        Self::new(1920, 1080)
    }
}

impl CameraBridge {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            driver: Mutex::new(None),
            cached_jpeg: Mutex::new(None),
            cache_duration: Duration::from_millis(300), // 300ms cache protects Pi CPU
            width,
            height,
        }
    }

    /// Detect the best available camera driver on the system
    pub fn detect_driver(&self) -> CameraDriver {
        let mut d_lock = self.driver.lock().unwrap();
        if let Some(driver) = *d_lock {
            return driver;
        }

        let detected = Self::probe_available_driver();
        *d_lock = Some(detected);
        detected
    }

    fn probe_available_driver() -> CameraDriver {
        // 1. Try modern Raspberry Pi OS Bookworm `rpicam-still`
        if Command::new("rpicam-still")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return CameraDriver::RpiCam;
        }

        // 2. Try legacy Raspberry Pi OS `libcamera-still`
        if Command::new("libcamera-still")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return CameraDriver::LibCamera;
        }

        // 3. Try Linux V4L2 USB Webcams
        if Path::new("/dev/video0").exists() {
            if Command::new("fswebcam")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return CameraDriver::FsWebcam;
            }

            if Command::new("ffmpeg")
                .arg("-version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return CameraDriver::Ffmpeg;
            }
        }

        // 4. Default fallback: synthetic test pattern
        CameraDriver::TestPattern
    }

    /// Capture or retrieve a cached JPEG snapshot of the laser bed
    pub fn get_snapshot(&self, force_refresh: bool) -> Result<Vec<u8>, String> {
        let now = Instant::now();

        // 1. Check cache unless force_refresh is requested
        if !force_refresh {
            let cache = self.cached_jpeg.lock().unwrap();
            if let Some((ref bytes, timestamp)) = *cache {
                if now.duration_since(timestamp) < self.cache_duration {
                    return Ok(bytes.clone());
                }
            }
        }

        // 2. Perform fresh capture
        let driver = self.detect_driver();
        let jpeg_bytes = match driver {
            CameraDriver::RpiCam => self.capture_rpicam()?,
            CameraDriver::LibCamera => self.capture_libcamera()?,
            CameraDriver::FsWebcam => self.capture_fswebcam()?,
            CameraDriver::Ffmpeg => self.capture_ffmpeg()?,
            CameraDriver::TestPattern => self.generate_synthetic_test_pattern(),
        };

        // 3. Store in cache
        {
            let mut cache = self.cached_jpeg.lock().unwrap();
            *cache = Some((jpeg_bytes.clone(), now));
        }

        Ok(jpeg_bytes)
    }

    fn capture_rpicam(&self) -> Result<Vec<u8>, String> {
        let out = Command::new("rpicam-still")
            .args([
                "-n",
                "-t",
                "200",
                "-q",
                "85",
                "--width",
                &self.width.to_string(),
                "--height",
                &self.height.to_string(),
                "--immediate",
                "-o",
                "-",
            ])
            .output()
            .map_err(|e| format!("Failed to execute rpicam-still: {}", e))?;

        if !out.status.success() || out.stdout.is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("rpicam-still capture failed: {}", err));
        }

        Ok(out.stdout)
    }

    fn capture_libcamera(&self) -> Result<Vec<u8>, String> {
        let out = Command::new("libcamera-still")
            .args([
                "-n",
                "-t",
                "200",
                "-q",
                "85",
                "--width",
                &self.width.to_string(),
                "--height",
                &self.height.to_string(),
                "--immediate",
                "-o",
                "-",
            ])
            .output()
            .map_err(|e| format!("Failed to execute libcamera-still: {}", e))?;

        if !out.status.success() || out.stdout.is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("libcamera-still capture failed: {}", err));
        }

        Ok(out.stdout)
    }

    fn capture_fswebcam(&self) -> Result<Vec<u8>, String> {
        let res_str = format!("{}x{}", self.width, self.height);
        let out = Command::new("fswebcam")
            .args([
                "-q",
                "-d",
                "/dev/video0",
                "--no-banner",
                "-r",
                &res_str,
                "--jpeg",
                "85",
                "-",
            ])
            .output()
            .map_err(|e| format!("Failed to execute fswebcam: {}", e))?;

        if !out.status.success() || out.stdout.is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("fswebcam capture failed: {}", err));
        }

        Ok(out.stdout)
    }

    fn capture_ffmpeg(&self) -> Result<Vec<u8>, String> {
        let res_str = format!("{}x{}", self.width, self.height);
        let out = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "v4l2",
                "-video_size",
                &res_str,
                "-i",
                "/dev/video0",
                "-vframes",
                "1",
                "-f",
                "image2pipe",
                "-vcodec",
                "mjpeg",
                "-",
            ])
            .output()
            .map_err(|e| format!("Failed to execute ffmpeg: {}", e))?;

        if !out.status.success() || out.stdout.is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("ffmpeg v4l2 capture failed: {}", err));
        }

        Ok(out.stdout)
    }

    /// Generate an accurate synthetic test pattern representing the laser bed
    /// with grid markers and crosshair (used when offline or in test environments)
    pub fn generate_synthetic_test_pattern(&self) -> Vec<u8> {
        let w = self.width.min(1280);
        let h = self.height.min(720);

        let mut img = image::RgbImage::new(w, h);

        // Fill background with realistic laser honeycomb bed slate gray (#1a1d24)
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([26, 29, 36]);
        }

        // Draw honeycomb/grid lines every 50 pixels
        for y in 0..h {
            for x in 0..w {
                if x % 40 == 0 || y % 40 == 0 {
                    img.put_pixel(x, y, image::Rgb([42, 47, 58]));
                }
            }
        }

        // Draw Bed Border / Work Area Outline (margin 40px)
        let min_x = 40.min(w - 1);
        let max_x = (w - 40).max(min_x);
        let min_y = 40.min(h - 1);
        let max_y = (h - 40).max(min_y);

        for x in min_x..=max_x {
            img.put_pixel(x, min_y, image::Rgb([0, 180, 216])); // Cyan border
            img.put_pixel(x, max_y, image::Rgb([0, 180, 216]));
        }
        for y in min_y..=max_y {
            img.put_pixel(min_x, y, image::Rgb([0, 180, 216]));
            img.put_pixel(max_x, y, image::Rgb([0, 180, 216]));
        }

        // Center Crosshair (Laser Zero / Bed Center)
        let cx = w / 2;
        let cy = h / 2;
        for dx in 0..20 {
            if cx + dx < w { img.put_pixel(cx + dx, cy, image::Rgb([239, 68, 68])); } // Red reticle
            if cx >= dx { img.put_pixel(cx - dx, cy, image::Rgb([239, 68, 68])); }
        }
        for dy in 0..20 {
            if cy + dy < h { img.put_pixel(cx, cy + dy, image::Rgb([239, 68, 68])); }
            if cy >= dy { img.put_pixel(cx, cy - dy, image::Rgb([239, 68, 68])); }
        }

        // 4 Calibration Target Corners (Circles/Squares at 4 standard points)
        let corners = [
            (min_x + 30, min_y + 30),
            (max_x - 30, min_y + 30),
            (max_x - 30, max_y - 30),
            (min_x + 30, max_y - 30),
        ];

        for &(px, py) in &corners {
            for dy in 0..9 {
                for dx in 0..9 {
                    let qx = (px + dx).saturating_sub(4).min(w - 1);
                    let qy = (py + dy).saturating_sub(4).min(h - 1);
                    img.put_pixel(qx, qy, image::Rgb([255, 215, 0])); // Gold target marks
                }
            }
        }

        let mut buf = Cursor::new(Vec::new());
        let _ = img.write_to(&mut buf, image::ImageFormat::Jpeg);
        buf.into_inner()
    }

    /// Get current camera status report
    pub fn status(&self) -> CameraStatus {
        let driver = self.detect_driver();
        let cached = self.cached_jpeg.lock().unwrap().is_some();
        let dev_str = match driver {
            CameraDriver::RpiCam | CameraDriver::LibCamera => "Raspberry Pi CSI Port",
            CameraDriver::FsWebcam | CameraDriver::Ffmpeg => "/dev/video0",
            CameraDriver::TestPattern => "Virtual Test Pattern Generator",
        };

        CameraStatus {
            available: driver != CameraDriver::TestPattern || true,
            driver: driver.as_str().to_string(),
            device: dev_str.to_string(),
            width: self.width,
            height: self.height,
            cached,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthetic_test_pattern_generation() {
        let bridge = CameraBridge::new(640, 480);
        let jpeg = bridge.generate_synthetic_test_pattern();
        assert!(!jpeg.is_empty());
        // Verify valid JPEG header (0xFF, 0xD8)
        assert_eq!(jpeg[0], 0xFF);
        assert_eq!(jpeg[1], 0xD8);
    }

    #[test]
    fn test_camera_bridge_status() {
        let bridge = CameraBridge::new(1280, 720);
        let status = bridge.status();
        assert!(status.available);
        assert_eq!(status.width, 1280);
        assert_eq!(status.height, 720);
    }

    #[test]
    fn test_camera_bridge_snapshot_and_cache() {
        let bridge = CameraBridge::new(320, 240);
        let snap1 = bridge.get_snapshot(false).expect("Snapshot 1");
        assert!(!snap1.is_empty());

        let snap2 = bridge.get_snapshot(false).expect("Snapshot 2 cached");
        assert_eq!(snap1, snap2);
    }
}

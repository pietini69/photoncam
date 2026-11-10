# 📜 PhotonCAM Changelog & Release History

All notable changes, new features, and bug fixes for **PhotonCAM** across every release.

---

## [v1.2.9] – 2026-11-10

### 🐛 Bug Fixes & Vector Import Hardening
* **📐 SVG Elliptical Arc Tessellation (Fix for "Cut Off" Rounded Corners):**
  * Fully implemented W3C SVG 1.1 Appendix F.6 compliant elliptical arc tessellation algorithm (`A` and `a` path commands) in `src/formats/svg.rs`.
  * Corrected start angle $\theta_1$, angular sweep $\Delta\theta$, large-arc and sweep flag orientation, and radii auto-scaling ($\Lambda > 1.0$).
  * Handles concatenated SVG boolean flags without whitespace (e.g. `01` or `00`).
  * Resolves issue where rounded corners appeared clipped with straight diagonal chords.
* **🔄 LightBurn `.lbrn2` MirrorY Coordinate Correction (Fix for Upside-Down Import):**
  * Fixed canvas inversion for `.lbrn2` projects created on laser machines with Bottom-Left / Front-Left machine origins (`MirrorY="True"`, such as Creality Falcon 2 Pro / GRBL).
  * Automatically inverts shapes vertically around the project's bounding box center in `src/formats/lbrn2.rs` to maintain exact visual orientation matching LightBurn.
  * Corrected font glyph polyline rendering for synthesized text paths when `MirrorY="True"`.
* **🧊 3D Sliced Relief Engraving & Depth Matrix Generators:**
  * Multi-pass heightmap progressive slicing with collision prevention and safe Z retract planes.
  * Direct GLB, glTF, and STL 3D mesh import with automated heightmap relief projection.
  * Caliper Engraving Depth Test generator for precision laser calibration.

---

## [v1.2.8] – 2026-10-31

### 🌟 New Features & Major Vector Fill (Engrave & Hatch) Upgrades
* **📱 Web-CAD & iPad Remote Shape Fill & Engrave Controls:**
  * **Shape Burning Modes:** Direct selection in Properties panel between **Viiva (Line)** (cut/score outline), **Täyttö (Fill)** (raster engrave interior), and **Täyttö + Viiva (Fill + Line)** (engrave interior followed by a clean contour cut).
  * **Hatch Scan Angle & Spacing Presets:** Choose raster direction (Vaaka 0°, Pysty 90°, 45° angle) or bi-directional **Ristiinpoltto (Cross-Hatch)** with configurable line interval (default 0.10 mm / ~254 DPI).
  * **Real-time Canvas Hatch Rendering:** Rectangles, circles, and polygons dynamically render their hatch patterns and semi-transparent fill preview directly on the Web-CAD canvas.
  * **Automatic QR Code Module Fill:** Built-in QR generator automatically assigns vector fill mode to all generated barcode data modules.
* **🖥️ Desktop PhotonCAM Canvas & Layer Settings:**
  * **Filled Shape Canvas Previews:** Shapes assigned to `CutMode::Fill` or `CutMode::OffsetFill` now render directly on the canvas as semi-transparent filled shapes instead of wireframe outlines.
  * **Layer Scan Angle & Cross-Hatch Controls:** Added dedicated scan angle input (0°, 90°, 45° presets) and cross-hatch checkbox to the desktop layer settings panel.
* **⚡ CAM Optimizer & SVG Engine Enhancements:**
  * **Hatch Optimizer Fix:** Corrected scan angle and interval parameter order in `generate_hatch` ensuring precise zigzag toolpath computation.
  * **Intelligent SVG Fill Parser:** SVG loader automatically identifies filled vectors (`fill` attributes, inline styles, and web remote metadata) and generates high-efficiency hatch toolpaths.
  * **Toolpath SSE Stream:** Web remote preview displays real-time generated laser hatch toolpaths instead of raw geometric outlines.

---

## [v1.2.7] – 2026-10-29

### 🌟 New Features & Major Web-CAD Upgrades
* **📱 Full iPad & Web-CAD Remote Upgrade:**
  * **0,0 Origin Corner Selector:** Select machine origin directly in the Web Remote UI (Top-Left, Top-Right, Bottom-Left, Bottom-Right) with live orientation synchronization to the controller via `/api/settings`.
  * **Visible Origin Marker & Direction Arrows:** Live glowing reticle at (0,0) with `+X` and `+Y` vector orientation arrows drawn on the canvas.
  * **Finger-Joint Box Maker (Sormiliitokset):** Parametric laser box generator calculating exact male/female tabs and slots with material kerf compensation.
  * **Living Hinge & QR Code Generators:** Instant geometric spring-cut hinges and crisp vector QR code creation right from the tablet browser.
  * **Robust Polygon & Curve Hit-Testing:** Ray-casting point-in-polygon and distance-to-segment algorithm for seamless touch selection of any shape, box part, or line.
  * **Individual Object Deletion:** Direct canvas `[✕]` badge on selected shapes, properties panel deletion, and an interactive object list.
* **🍓 Raspberry Pi Systemd Boot Auto-start:**
  * Updated 1-line installer creates `/etc/systemd/system/photoncam-bridge.service` and enables automatic service start on boot (`systemctl enable --now`).
  * Ready-to-flash ARM64 Raspberry Pi OS image updated with latest bridge and full Web-CAD capabilities.
* **⚡ G-Code Coordinate Peeling & Mirroring:** `/api/upload` endpoint automatically maps and mirrors uploaded vector paths to match the machine's configured origin corner.

---

## [v1.2.6] – 2026-10-28

### 🌟 New Features & Fixes
* **🌐 Network connection to the Raspberry Pi Bridge from PhotonCAM desktop:** the Connection window now has a **USB / Serial** vs **Network (Raspberry Pi)** selector. Enter the Pi's hostname or IP (e.g. `photoncam.local` or `192.168.68.90`, default port `2323`) and connect — jogging, framing, job streaming and live status work over Wi-Fi/Ethernet exactly like USB. Network addresses can be saved and auto-connected on startup.
* **🔌 Bridge raw link is now fully bidirectional:** previously the Pi's TCP port only forwarded newline-terminated commands *to* the laser and never sent the controller's replies back. It now relays raw bytes both ways (including realtime commands `?`, `!`, `~`, soft-reset `0x18`) and streams all controller output to every connected client.
* Connection loss over the network is detected and reported (the app disconnects cleanly instead of appearing connected).
* New tests for network link parsing, round-trip I/O and connection failure.

---

## [v1.2.5] – 2026-10-27

### 🌟 New Features & Major Additions
* **🌐 Web Remote & iPad / Tablet Touch Support:**
  * Embedded zero-dependency HTTP/1.1 & Server-Sent Events (SSE) web server running on port 8080 (configurable).
  * Responsive HTML5 + Canvas Web Remote interface optimized for iPad, tablets, and mobile phones with dark mode and Apple PWA standalone support.
  * Interactive 2D bed preview, jog D-Pad with customizable steps, dual units toggle (mm/in), live coordinate streaming at 10Hz, file upload (`.gcode`, `.lbrn2`, `.svg`), start/pause/stop job streaming, focus light toggle, and instant E-Stop.
  * Transparent TCP-to-Serial bridge on port 2323 for wireless network connection.
  * One-click start/stop and browser launch directly from the Desktop toolbar (`🌐 Web Remote`) or headless via `laserburn-bridge`.
* **🍓 Raspberry Pi Headless Bridge & Ready-to-Flash OS Image:**
  * Standalone lightweight CLI daemon `laserburn-bridge` for Raspberry Pi (ARM64 & ARMv7) and Linux.
  * Automated 1-line installation script (`curl -sSL https://raw.githubusercontent.com/pietini69/laserburn/master/scripts/install_pi_bridge.sh | bash`).
  * Ready-to-flash Raspberry Pi OS Disk Image (`PhotonCAM-Pi-OS-arm64.img.xz`) with Avahi mDNS (`laserburn.local:8080`) and auto-launch systemd service.
* **📷 Optional Camera Mode (Distraction-Free Minimal UI):**
  * Added **"Enable Camera Support"** setting in Settings (`Cmd+,`).
  * When unchecked, all camera toolbar buttons (`Camera`, `Cam BG`, `Update Overlay`, `Snap to Tile`), Camera menus, and camera shortcuts are completely hidden from the workspace, providing a clean, distraction-free CAD/CAM layout for users without an overhead camera.
* **🔍 Job Preview & Cut Simulation:**
  * Interactive simulation window with animated laser head, scrub timeline, accurate time estimation, and color-coded rapid traverse vs cutting/engraving moves.
* **📏 Dual Units Display Improvements:**
  * Status bar coordinates and jog controls now dynamically format in inches when Imperial mode is active.

---

## [v1.2.0] – 2026-10-27

### 🌟 New Features & Major Capabilities
* **Centerline Trace (Keskiviivajäljitys / Zhang-Suen Morphological Skeletonization):**
  * Added single-stroke centerline vectorization alongside contour tracing in the *Trace Image* dialog.
  * Uses the two-phase Zhang-Suen morphological thinning algorithm to reduce thick bitmap lines into true 1-pixel-wide mathematical skeletons.
  * Graph-based 8-connected path generator with Ramer-Douglas-Peucker simplification, turning handwriting, ink drawings, and sketches into smooth open/closed vector cutting paths.
  * Perfect for handwritten family recipes, signatures, and line art — saves up to 90% cutting time compared to raster engraving.
* **Full Dual Units System (Imperial & Metric / Tuumat ja Millimetrit):**
  * One-click unit toggle: `[📏 mm]` / `[📏 in]` in top menubar, Transform panel header, and Settings dialog.
  * **Dynamic Canvas Rulers & Grid:** When Inches is active, rulers display inch markings (`1"`, `2"`, `3"`, `4"`, etc.) with 0.25", 0.5", and 1.0" subdivisions.
  * **Transform & Size Controls:** Direct position (`X`, `Y`) and dimensions (`Width`, `Height`) in inches with 0.001 in precision.
  * **Status Bar & Bed Dimensions:** Live machine coordinates and workbed dimensions format seamlessly in inches with dual metric helper.
  * **Jog Steps:** Siirtoaskeleet vaihtuvat tuumiksi (`0.1"`, `0.5"`, `1.0"`).
* **Interactive Lockable Layers (Lukittavat tasot):**
  * Interactive `[🔒]` / `[🔓]` lock button on every layer in the Cuts & Layers panel.
  * Protects shapes on locked layers from accidental mouse clicks, dragging, resizing, marquee selection, and deletion.
  * Ideal for alignment jigs, material cut boundaries, and background reference templates.
* **Parameter-Isolated Undo Stack (Turvallinen Undo):**
  * Geometric canvas Undo (`Cmd+Z` / `Ctrl+Z`) is strictly separated from laser cutting parameters.
  * Undoing drawing edits or shape moves will never reset carefully tuned laser speed, power, passes, or air-assist parameters.

---

## [v1.1.0] – 2026-10-26

### 🚀 Highlights
* **🍎 Apple Shortcuts Automation (macOS):**
  * Trigger native macOS Shortcuts automatically when a job starts or completes (turn on workshop exhaust fans, adjust lighting, send push notifications to iPhone/Apple Watch).
* **📂 Native LightBurn .lbrn2 XML Compatibility:**
  * Full two-way support for opening and saving modern LightBurn project files with vector paths, circles, rectangles, cut settings, kerf compensation, and scan speeds.
* **🛡️ Real-Time Pin Reporting & Safety Protection:**
  * Live GRBL pin status reporting (`$10=19`) for limit switches, door interlocks, and touch probes.
  * One-click hard limits protection (`$21=1`) and soft limit boundary monitoring.
* **🔄 Interactive Transform Handles & Free Resizing:**
  * 8 interactive boundary handles for free aspect-ratio scaling or constrained proportional scaling, with instant circle-to-oval conversion.
* **⚡ Sub-Second Rust G-Code Engine:**
  * Optimized path planning and real-time execution monitor with physics-based acceleration profiling.

---

## [v1.0.0] – Initial Release

### ⚡ Core Foundation
* **Multi-Controller Support:** Direct USB serial support for **GRBL 1.1+** and network/USB support for **Ruida DSP** controllers.
* **Studio-Grade Image Dithering:** 9 high-quality dithering modes including Atkinson, Floyd-Steinberg, Jarvis-Judice-Ninke, Stucki, Burkes, Sierra3, Ordered Bayer 8x8, Halftone, and True Grayscale.
* **Sub-Pixel Marching Squares Vectorizer:** High-speed contour vectorization from raster logos and silhouettes.
* **Ceramic Tile & Multi-Color Simulation:** Dynamic PWM laser power modulation with real-time tile layer preview.
* **Live Camera Bed Alignment:** USB webcam support with 4-point homography calibration and 1-click tile auto-fitting.
* **Cross-Platform:** Native builds for macOS (Apple Silicon & Intel), Windows 10/11, and Linux.

# 📜 PhotonCAM Changelog & Release History

All notable changes, new features, and bug fixes for **PhotonCAM** across every release.

---

## [v2.0.0-beta.1] – 2026-11-12

### 🎨 Modular UI & Parametric Generators
* **Photoshop-Style Tool Palette & Floating Inspector Panels:**
  * Slim (44px) vector tool palette, detachable and draggable anywhere on screen.
  * Floating, collapsible rollup panels for Properties & Layers, Objects List, Job Control HUD, and Laser Positioning/Jog.
  * Persistent window layout memory across application restarts (`UiLayoutSettings`).
* **Parametric Generators:**
  * **Box Maker:** Finger-joint tabbed boxes with material thickness and kerf compensation.
  * **Living Hinge:** Straight and wavy flex patterns for bent wood projects.
  * **QR Code:** Vector laser QR code generator with crisp module fill.
  * **Variable Text:** Batch serialization with sequential counters, dates, and CSV file templates.
  * **Caliper Engraving Depth Test:** Stepped wedge, multi-pass, and interval matrix tests for caliper depth measurement.
* **3D Relief & Mesh Import:**
  * Direct loading of STL, GLB, and glTF 3D meshes with automated heightmap relief projection.
  * 3D sliced multi-pass relief engraving with material safety and collision prevention.
* **Universal LightBurn Alignment:**
  * Fully unified Cartesian coordinate transformation for all `.lbrn2` files.

---

## [v1.2.10] – 2026-11-11

### 🔄 Coordinate System & Universal Alignment
* **Universal LightBurn (.lbrn2) Orientation Correction:**
  * Unified Cartesian +Y upwards coordinate transformation across all LightBurn projects regardless of machine origin setup (`MirrorY="True"` or `MirrorY="False"`, e.g. Creality Falcon 2 Pro, xTool, Sculpfun, Ortur, GRBL).
  * Designs, sign borders, vector paths, and text outlines consistently import with 100% upright visual orientation matching LightBurn's display.
  * Verified with complex multi-layer designs including `PetesRancho.lbrn2`.
* **Automated Raspberry Pi OS Image Build:**
  * Hardened GitHub Actions automated Pi OS image builder with full ARM64 `photoncam-bridge` integration.

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
  * Systemd service unit configured and enabled automatically on Pi to start `photoncam-bridge` on boot.

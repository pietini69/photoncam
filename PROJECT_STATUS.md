# PhotonCAM — Project Status & Architecture Context

## Current Status (v1.2.9 — November 10, 2026)
**PhotonCAM** (formerly LaserBurn) is a high-performance, modern laser CAM/CNC software engineered from the ground up in Rust (`eframe`/`egui`).
It is currently at version **v1.2.9** (all unit tests passing), deployed as a signed macOS Universal 2 DMG/ZIP (`dist/`), Windows x64 portable ZIP, Linux x86_64 tarball, and headless Raspberry Pi ARM64/ARMv7 daemons.
Official GitHub Repository: `https://github.com/pietini69/photoncam.git`
Live Gumroad Product: `https://shop.pietiniart.com/l/photoncam` (ID: `1SXqccPmq2GJB3uOomjR3Q==`)

---

### Key Milestones & Features Implemented up to v1.2.6:

1. **Rebranding to PhotonCAM:**
   - Global rebranding across app window titles, logs, binaries, material database, and documentation to ensure trademark protection and prepare for App Store distribution.

2. **Wireless Web Remote (HTML5 + SSE Server, Port 8080):**
   - Zero-dependency embedded HTTP/1.1 and Server-Sent Events server in `src/server/mod.rs`.
   - Touch-first responsive dark-mode web interface (`src/server/web/index.html`) optimized for iPad, Android tablets, and phones.
   - Interactive 2D bed canvas, jog D-pad with step selection (0.1–100mm), dual units (mm/in), live 10Hz position streaming, file upload (`.gcode`, `.lbrn2`, `.svg`), real-time job execution, and E-Stop.

3. **4-Quadrant Machine Origin & Coordinate Mapping:**
   - Support for all laser homing positions: Top-Left, Top-Right (Takaoikea default), Bottom-Left, Bottom-Right.
   - Independent X/Y axis mirroring and inverted jog D-pad controls for brands like Ortur, xTool, Sculpfun, Atomstack, Creality, Two Trees, Neje.
   - Live canvas coordinate transform projecting machine coordinates to screen coordinates without upside-down text or inverted rulers.

4. **Headless Raspberry Pi Bridge & Bidirectional TCP Protocol (Port 2323):**
   - Standalone CLI daemon `photoncam-bridge` for Raspberry Pi (ARM64 & ARMv7) and Linux.
   - Connects to laser via USB (`/dev/ttyACM0` or `/dev/ttyUSB0`) and serves Web Remote on port 8080.
   - Raw TCP bridge on port 2323 is **fully bidirectional**: streams realtime GRBL commands (`?`, `!`, `~`, soft-reset `0x18`) and broadcasts all controller replies back to connected desktop clients.
   - 1-line installer: `curl -sSL https://raw.githubusercontent.com/pietini69/photoncam/master/scripts/install_pi_bridge.sh | sudo bash`.
   - Automated GitHub Actions build (`build-pi-image.yml`) generating ready-to-flash disk images (`PhotonCAM-Pi-OS-arm64.img.xz`) with pre-configured user `pi:raspberry`.

5. **Desktop Wireless Network Connection:**
   - In PhotonCAM desktop Connection window, users can select **Network (Raspberry Pi)** instead of USB.
   - Connects directly to Pi's IP (e.g. `192.168.68.73:2323` or `photoncam.local:2323`).
   - Network links can be saved and auto-connected on startup; automatic connection-loss detection.

6. **Git Branching Strategy:**
   - `master`: Stable production release branch (currently at `v1.2.6`).
   - `v2-ui`: Active development branch for the upcoming V2 major UI overhaul (Feather Icons, SVGRepo/OpenSVGIcons/Iconstack vector icons, modern styling).

---

### Hardware Setup Reference (Niko Pietinen):
- **Laser Engraver:** Diode laser with Top-Right (Takana oikealla) machine origin, ~400×400 mm / 465×440 mm bed.
- **Raspberry Pi Server:** Dedicated wireless bridge connected to laser USB, located at `192.168.68.73` (or `192.168.68.90` / `photoncam.local`). Port 8080 (Web Remote), Port 2323 (Raw TCP bridge).
- **Credentials:** Default image user: `pi:raspberry` / Custom install user: `pietini:648352`.

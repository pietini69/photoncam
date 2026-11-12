use crate::geometry::{Point2D, Polyline, Vector2D};
use crate::Shape;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnclosurePreset {
    /// Fully custom box dimensions
    Custom,
    /// Raspberry Pi 4 with 3.5" GPIO Touchscreen LCD (74x50mm screen cutout + ports)
    RaspberryPi4WithDisplay35,
    /// Raspberry Pi 4 Standard case (standoff holes + USB-C / micro-HDMI / USB / Ethernet ports)
    RaspberryPi4Standard,
    /// Raspberry Pi 5 Standard case (standoff holes + power / micro-HDMI / Ethernet / USB ports)
    RaspberryPi5Standard,
}

impl Default for EnclosurePreset {
    fn default() -> Self {
        EnclosurePreset::Custom
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoxLidType {
    /// Open top box without lid
    Open,
    /// Fully enclosed box with finger joints on all 6 sides
    FingerJointLid,
    /// Box with slide-in flat lid
    SlideInLid,
    /// Simple flat lid resting on top
    FlatLid,
}

impl Default for BoxLidType {
    fn default() -> Self {
        BoxLidType::FingerJointLid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxMakerParams {
    /// Device enclosure preset (or Custom for standard box)
    #[serde(default)]
    pub preset: EnclosurePreset,
    /// Outer width in mm (X axis)
    pub width: f64,
    /// Outer depth in mm (Y axis)
    pub depth: f64,
    /// Outer height in mm (Z axis)
    pub height: f64,
    /// Material sheet thickness in mm (e.g. 3.0 or 4.0 mm)
    pub thickness: f64,
    /// Target finger/tab width in mm (e.g. 10.0 mm)
    pub tab_size: f64,
    /// Kerf compensation in mm (e.g. 0.05 - 0.15 mm for snug press-fit)
    pub kerf: f64,
    /// Lid style
    pub lid_type: BoxLidType,
    /// Internal dividers along X axis (number of divider walls)
    pub dividers_x: usize,
    /// Internal dividers along Y axis (number of divider walls)
    pub dividers_y: usize,
    /// Margin between unpacked 2D parts on canvas (mm)
    pub layout_margin: f64,
}

impl Default for BoxMakerParams {
    fn default() -> Self {
        Self {
            preset: EnclosurePreset::Custom,
            width: 100.0,
            depth: 80.0,
            height: 50.0,
            thickness: 3.0,
            tab_size: 10.0,
            kerf: 0.08,
            lid_type: BoxLidType::FingerJointLid,
            dividers_x: 0,
            dividers_y: 0,
            layout_margin: 5.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeType {
    /// Flat straight edge without fingers
    Flat,
    /// Male tabs protruding outwards (+thickness)
    Tabs,
    /// Female slots cutting inwards (-thickness)
    Slots,
}

#[derive(Debug, Clone)]
pub struct BoxPart {
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub polyline: Polyline,
    pub cutouts: Vec<Shape>,
}

/// Generates a single finger-jointed edge between start and end.
/// `normal` is a unit vector pointing towards the exterior of the part.
fn generate_edge_points(
    start: Point2D,
    end: Point2D,
    edge_type: EdgeType,
    thickness: f64,
    target_tab_size: f64,
    kerf: f64,
    normal: Vector2D,
) -> Vec<Point2D> {
    if edge_type == EdgeType::Flat {
        return vec![start, end];
    }

    let edge_vec = Vector2D::new(end.x - start.x, end.y - start.y);
    let edge_len = edge_vec.length();
    if edge_len < 1.0 {
        return vec![start, end];
    }

    let dir = Vector2D::new(edge_vec.dx / edge_len, edge_vec.dy / edge_len);

    // Calculate odd number of segments so start and end have symmetrical fingers
    let mut num_segments = (edge_len / target_tab_size.max(2.0)).round() as usize;
    if num_segments < 3 {
        num_segments = 3;
    }
    if num_segments % 2 == 0 {
        num_segments += 1;
    }

    let seg_len = edge_len / (num_segments as f64);
    let mut pts = Vec::with_capacity(num_segments * 4 + 2);
    pts.push(start);

    for i in 0..num_segments {
        let is_tab_segment = i % 2 == 0;
        let t0 = (i as f64) * seg_len;
        let t1 = ((i + 1) as f64) * seg_len;

        let is_protruding = match edge_type {
            EdgeType::Tabs => is_tab_segment,
            EdgeType::Slots => !is_tab_segment,
            EdgeType::Flat => false,
        };

        let offset = if is_protruding {
            // Outward tab: expand by kerf to make it tighter
            thickness
        } else {
            0.0
        };

        // Kerf adjustments along the edge direction:
        // Tabs are wider by +kerf; slots are narrower by -kerf
        let (adj0, adj1) = if is_protruding {
            (-kerf * 0.5, kerf * 0.5)
        } else {
            (kerf * 0.5, -kerf * 0.5)
        };

        let pt_a = Point2D::new(
            start.x + dir.dx * (t0 + adj0) + normal.dx * offset,
            start.y + dir.dy * (t0 + adj0) + normal.dy * offset,
        );
        let pt_b = Point2D::new(
            start.x + dir.dx * (t1 + adj1) + normal.dx * offset,
            start.y + dir.dy * (t1 + adj1) + normal.dy * offset,
        );

        pts.push(pt_a);
        pts.push(pt_b);
    }

    pts.push(end);
    pts
}

/// Generates a closed polyline panel defined by its 4 edges: bottom, right, top, left
pub fn generate_panel(
    name: &str,
    w: f64,
    h: f64,
    bottom: EdgeType,
    right: EdgeType,
    top: EdgeType,
    left: EdgeType,
    thickness: f64,
    tab_size: f64,
    kerf: f64,
) -> BoxPart {
    let p_bl = Point2D::new(0.0, 0.0);
    let p_br = Point2D::new(w, 0.0);
    let p_tr = Point2D::new(w, h);
    let p_tl = Point2D::new(0.0, h);

    let mut points = Vec::new();

    // 1. Bottom edge (BL -> BR, normal points DOWN: 0, -1)
    let pts_b = generate_edge_points(p_bl, p_br, bottom, thickness, tab_size, kerf, Vector2D::new(0.0, -1.0));
    points.extend(pts_b);

    // 2. Right edge (BR -> TR, normal points RIGHT: 1, 0)
    let pts_r = generate_edge_points(p_br, p_tr, right, thickness, tab_size, kerf, Vector2D::new(1.0, 0.0));
    points.extend(pts_r);

    // 3. Top edge (TR -> TL, normal points UP: 0, 1)
    let pts_t = generate_edge_points(p_tr, p_tl, top, thickness, tab_size, kerf, Vector2D::new(0.0, 1.0));
    points.extend(pts_t);

    // 4. Left edge (TL -> BL, normal points LEFT: -1, 0)
    let pts_l = generate_edge_points(p_tl, p_bl, left, thickness, tab_size, kerf, Vector2D::new(-1.0, 0.0));
    points.extend(pts_l);

    // Deduplicate consecutive identical points
    let mut clean_pts: Vec<Point2D> = Vec::with_capacity(points.len());
    for p in points {
        if let Some(last) = clean_pts.last() {
            if last.distance_squared_to(&p) < 1e-6 {
                continue;
            }
        }
        clean_pts.push(p);
    }

    BoxPart {
        name: name.to_string(),
        width: w,
        height: h,
        polyline: Polyline::new(clean_pts, true),
        cutouts: Vec::new(),
    }
}

/// Generates all box parts based on parameters
pub fn generate_box_parts(params: &BoxMakerParams) -> Vec<BoxPart> {
    let mut parts = Vec::new();

    let w = params.width.max(10.0);
    let d = params.depth.max(10.0);
    let h = params.height.max(10.0);
    let t = params.thickness.max(0.5);
    let tab = params.tab_size.max(2.0);
    let kerf = params.kerf.clamp(-1.0, 1.0);

    // 1. Bottom panel (W x D): All 4 edges have male Tabs
    let bottom = generate_panel(
        "Pohja (Bottom)",
        w, d,
        EdgeType::Tabs, EdgeType::Tabs, EdgeType::Tabs, EdgeType::Tabs,
        t, tab, kerf,
    );
    parts.push(bottom);

    // Side edges top style depends on lid
    let (side_top_edge, front_top_edge) = match params.lid_type {
        BoxLidType::Open => (EdgeType::Flat, EdgeType::Flat),
        BoxLidType::FingerJointLid => (EdgeType::Slots, EdgeType::Slots),
        BoxLidType::SlideInLid => (EdgeType::Flat, EdgeType::Flat),
        BoxLidType::FlatLid => (EdgeType::Flat, EdgeType::Flat),
    };

    // 2. Front & Back panels (W x H)
    // Bottom: Slots (fits bottom Tabs), Left & Right: Tabs, Top: side_top_edge
    let front = generate_panel(
        "Etuseinä (Front)",
        w, h,
        EdgeType::Slots, EdgeType::Tabs, front_top_edge, EdgeType::Tabs,
        t, tab, kerf,
    );
    let back = generate_panel(
        "Takaseinä (Back)",
        w, h,
        EdgeType::Slots, EdgeType::Tabs, front_top_edge, EdgeType::Tabs,
        t, tab, kerf,
    );
    parts.push(front);
    parts.push(back);

    // 3. Left & Right panels (D x H)
    // Bottom: Slots (fits bottom Tabs), Left & Right: Slots (fits front/back Tabs), Top: side_top_edge
    let left = generate_panel(
        "Vasen seinä (Left)",
        d, h,
        EdgeType::Slots, EdgeType::Slots, side_top_edge, EdgeType::Slots,
        t, tab, kerf,
    );
    let right = generate_panel(
        "Oikea seinä (Right)",
        d, h,
        EdgeType::Slots, EdgeType::Slots, side_top_edge, EdgeType::Slots,
        t, tab, kerf,
    );
    parts.push(left);
    parts.push(right);

    // 4. Lid panel (if requested)
    match params.lid_type {
        BoxLidType::Open => {}
        BoxLidType::FingerJointLid => {
            let lid = generate_panel(
                "Kansi (Lid)",
                w, d,
                EdgeType::Tabs, EdgeType::Tabs, EdgeType::Tabs, EdgeType::Tabs,
                t, tab, kerf,
            );
            parts.push(lid);
        }
        BoxLidType::FlatLid => {
            let lid = generate_panel(
                "Kansi (Flat Lid)",
                w, d,
                EdgeType::Flat, EdgeType::Flat, EdgeType::Flat, EdgeType::Flat,
                t, tab, kerf,
            );
            parts.push(lid);
        }
        BoxLidType::SlideInLid => {
            // Slide in lid slightly wider to fit side grooves
            let lid = generate_panel(
                "Liukukansi (Slide Lid)",
                w - 2.0 * t, d - t,
                EdgeType::Flat, EdgeType::Flat, EdgeType::Flat, EdgeType::Flat,
                t, tab, kerf,
            );
            parts.push(lid);
        }
    }

    // 5. Dividers (if requested)
    if params.dividers_x > 0 {
        for idx in 0..params.dividers_x {
            let name = format!("Jakaja X{} (Divider X)", idx + 1);
            let div_x = generate_panel(
                &name,
                d - 2.0 * t, h - t,
                EdgeType::Flat, EdgeType::Flat, EdgeType::Flat, EdgeType::Flat,
                t, tab, kerf,
            );
            parts.push(div_x);
        }
    }

    if params.dividers_y > 0 {
        for idx in 0..params.dividers_y {
            let name = format!("Jakaja Y{} (Divider Y)", idx + 1);
            let div_y = generate_panel(
                &name,
                w - 2.0 * t, h - t,
                EdgeType::Flat, EdgeType::Flat, EdgeType::Flat, EdgeType::Flat,
                t, tab, kerf,
            );
            parts.push(div_y);
        }
    }

    // 6. Apply Enclosure Presets (e.g. Raspberry Pi 4 + 3.5" Display)
    match params.preset {
        EnclosurePreset::Custom => {}
        EnclosurePreset::RaspberryPi4WithDisplay35 => {
            apply_rpi4_display35_cutouts(&mut parts, w, d, h, t);
        }
        EnclosurePreset::RaspberryPi4Standard => {
            apply_rpi4_standard_cutouts(&mut parts, w, d, h, t);
        }
        EnclosurePreset::RaspberryPi5Standard => {
            apply_rpi5_standard_cutouts(&mut parts, w, d, h, t);
        }
    }

    parts
}

fn apply_rpi4_display35_cutouts(parts: &mut [BoxPart], w: f64, d: f64, _h: f64, t: f64) {
    let cx = w * 0.5;
    let cy = d * 0.5;

    // 1. Bottom panel: 4x M2.5 standoff holes (58.0 x 49.0 mm Raspberry Pi 4 pattern) + cooling vents
    if let Some(bottom) = parts.iter_mut().find(|p| p.name.contains("Pohja") || p.name.contains("Bottom")) {
        let hole_r = 1.35; // M2.5 clearance hole
        let dx = 29.0;
        let dy = 24.5;
        bottom.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx - dx, cy - dy), radius: hole_r });
        bottom.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx + dx, cy - dy), radius: hole_r });
        bottom.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx + dx, cy + dy), radius: hole_r });
        bottom.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx - dx, cy + dy), radius: hole_r });

        // Passive cooling slots
        for s in -2..=2 {
            bottom.cutouts.push(Shape::Rect {
                layer_index: 0,
                x: cx - 18.0,
                y: cy + (s as f64) * 6.0 - 1.2,
                width: 36.0,
                height: 2.4,
                corner_radius: 1.2,
            });
        }
    }

    // 2. Lid panel: 3.5" GPIO Touchscreen viewing window (74.0 x 50.0 mm cutout)
    if let Some(lid) = parts.iter_mut().find(|p| p.name.contains("Kansi") || p.name.contains("Lid")) {
        lid.cutouts.push(Shape::Rect {
            layer_index: 0,
            x: cx - 37.0,
            y: cy - 25.0,
            width: 74.0,
            height: 50.0,
            corner_radius: 2.0,
        });
    }

    // 3. Front panel: USB-C power & micro-HDMI cutouts
    if let Some(front) = parts.iter_mut().find(|p| p.name.contains("Etuseinä") || p.name.contains("Front")) {
        // USB-C power
        front.cutouts.push(Shape::Rect {
            layer_index: 0,
            x: 10.0 + t,
            y: 3.5 + t,
            width: 11.0,
            height: 6.5,
            corner_radius: 2.0,
        });
        // Micro-HDMI 0 & 1
        front.cutouts.push(Shape::Rect {
            layer_index: 0,
            x: 25.5 + t,
            y: 3.5 + t,
            width: 9.5,
            height: 5.5,
            corner_radius: 1.5,
        });
        front.cutouts.push(Shape::Rect {
            layer_index: 0,
            x: 40.0 + t,
            y: 3.5 + t,
            width: 9.5,
            height: 5.5,
            corner_radius: 1.5,
        });
        // 3.5mm Audio jack
        front.cutouts.push(Shape::Circle {
            layer_index: 0,
            center: Point2D::new(54.0 + t, 6.5 + t),
            radius: 3.5,
        });
    }

    // 4. Right panel: USB-A ports & Ethernet RJ45
    if let Some(right) = parts.iter_mut().find(|p| p.name.contains("Oikea") || p.name.contains("Right")) {
        right.cutouts.push(Shape::Rect {
            layer_index: 0,
            x: 6.0 + t,
            y: 3.5 + t,
            width: 16.0,
            height: 16.5,
            corner_radius: 1.0,
        });
        right.cutouts.push(Shape::Rect {
            layer_index: 0,
            x: 25.0 + t,
            y: 3.5 + t,
            width: 16.0,
            height: 16.5,
            corner_radius: 1.0,
        });
        right.cutouts.push(Shape::Rect {
            layer_index: 0,
            x: 44.0 + t,
            y: 3.5 + t,
            width: 16.5,
            height: 14.5,
            corner_radius: 1.0,
        });
    }

    // 5. Left panel: MicroSD card slot cutout
    if let Some(left) = parts.iter_mut().find(|p| p.name.contains("Vasen") || p.name.contains("Left")) {
        left.cutouts.push(Shape::Rect {
            layer_index: 0,
            x: d * 0.5 - 7.0,
            y: 1.5 + t,
            width: 14.0,
            height: 3.5,
            corner_radius: 1.0,
        });
    }
}

fn apply_rpi4_standard_cutouts(parts: &mut [BoxPart], w: f64, d: f64, h: f64, t: f64) {
    apply_rpi4_display35_cutouts(parts, w, d, h, t);
    // For standard Pi without top screen, remove display cutout and add top fan vents
    if let Some(lid) = parts.iter_mut().find(|p| p.name.contains("Kansi") || p.name.contains("Lid")) {
        lid.cutouts.clear();
        let cx = w * 0.5;
        let cy = d * 0.5;
        // 30mm cooling fan center hole (radius 14mm) + 4 corner screw holes (24mm spacing)
        lid.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx, cy), radius: 14.0 });
        let fd = 12.0;
        lid.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx - fd, cy - fd), radius: 1.4 });
        lid.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx + fd, cy - fd), radius: 1.4 });
        lid.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx + fd, cy + fd), radius: 1.4 });
        lid.cutouts.push(Shape::Circle { layer_index: 0, center: Point2D::new(cx - fd, cy + fd), radius: 1.4 });
    }
}

fn apply_rpi5_standard_cutouts(parts: &mut [BoxPart], w: f64, d: f64, h: f64, t: f64) {
    apply_rpi4_standard_cutouts(parts, w, d, h, t);
    // Pi 5 specific: power button access hole on front panel
    if let Some(front) = parts.iter_mut().find(|p| p.name.contains("Etuseinä") || p.name.contains("Front")) {
        front.cutouts.push(Shape::Circle {
            layer_index: 0,
            center: Point2D::new(2.5 + t, 5.0 + t),
            radius: 2.0,
        });
    }
}

/// Unpacks and arranges all box parts neatly into 2D canvas space starting at (start_x, start_y).
/// Returns a list of `Shape::Path` objects ready to add to `doc.shapes`.
pub fn layout_box_shapes(
    parts: &[BoxPart],
    start_x: f64,
    start_y: f64,
    margin: f64,
    layer_index: usize,
) -> Vec<Shape> {
    let mut shapes = Vec::with_capacity(parts.len());
    let mut cur_x = start_x;
    let mut cur_y = start_y;
    let mut row_max_h = 0.0_f64;
    let max_row_width = 350.0_f64; // Wrap to next row if layout exceeds this width

    for part in parts {
        let b = part.polyline.bounds();
        let part_w = b.width().max(part.width);
        let part_h = b.height().max(part.height);

        if cur_x + part_w > start_x + max_row_width && cur_x > start_x {
            // New row
            cur_x = start_x;
            cur_y += row_max_h + margin;
            row_max_h = 0.0;
        }

        // Translate polyline points to current position
        let offset_x = cur_x - b.min_x;
        let offset_y = cur_y - b.min_y;

        let translated_points: Vec<Point2D> = part
            .polyline
            .points
            .iter()
            .map(|p| Point2D::new(p.x + offset_x, p.y + offset_y))
            .collect();

        shapes.push(Shape::Path {
            layer_index,
            polyline: Polyline::new(translated_points, true),
        });

        // Translate and add all internal cutouts for ports, mounting holes, and display windows
        for cutout in &part.cutouts {
            let mut c = cutout.clone();
            c.translate(offset_x, offset_y);
            c.set_layer_index(layer_index);
            shapes.push(c);
        }

        cur_x += part_w + margin;
        if part_h > row_max_h {
            row_max_h = part_h;
        }
    }

    shapes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_maker_generates_parts() {
        let params = BoxMakerParams {
            preset: EnclosurePreset::Custom,
            width: 80.0,
            depth: 60.0,
            height: 40.0,
            thickness: 3.0,
            tab_size: 10.0,
            kerf: 0.1,
            lid_type: BoxLidType::FingerJointLid,
            dividers_x: 1,
            dividers_y: 0,
            layout_margin: 5.0,
        };

        let parts = generate_box_parts(&params);
        // 1 bottom + 2 front/back + 2 left/right + 1 lid + 1 divider = 7 parts
        assert_eq!(parts.len(), 7);

        for part in &parts {
            assert!(part.polyline.points.len() >= 4, "Part {} has too few points", part.name);
            assert!(part.polyline.is_closed);
        }

        let shapes = layout_box_shapes(&parts, 20.0, 20.0, 5.0, 0);
        assert_eq!(shapes.len(), 7);
    }

    #[test]
    fn test_rpi4_display35_enclosure_preset() {
        let params = BoxMakerParams {
            preset: EnclosurePreset::RaspberryPi4WithDisplay35,
            width: 95.0,
            depth: 66.0,
            height: 38.0,
            thickness: 3.0,
            tab_size: 10.0,
            kerf: 0.08,
            lid_type: BoxLidType::FingerJointLid,
            dividers_x: 0,
            dividers_y: 0,
            layout_margin: 5.0,
        };

        let parts = generate_box_parts(&params);
        assert_eq!(parts.len(), 6);

        // Verify bottom has 4 standoff holes + cooling slots
        let bottom = parts.iter().find(|p| p.name.contains("Bottom") || p.name.contains("Pohja")).unwrap();
        assert!(bottom.cutouts.len() >= 9);

        // Verify lid has the 74x50mm screen cutout
        let lid = parts.iter().find(|p| p.name.contains("Lid") || p.name.contains("Kansi")).unwrap();
        assert_eq!(lid.cutouts.len(), 1);
        if let Shape::Rect { width, height, .. } = lid.cutouts[0] {
            assert_eq!(width, 74.0);
            assert_eq!(height, 50.0);
        } else {
            panic!("Expected rectangular screen cutout");
        }

        let shapes = layout_box_shapes(&parts, 10.0, 10.0, 5.0, 0);
        assert!(shapes.len() >= 20);
    }
}

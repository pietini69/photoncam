use crate::document::{CutMode, Document, Shape};
use crate::geometry::{tessellate_cubic_bezier, Point2D, Polyline};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }
}

impl Transform2D {
    pub fn parse(s: &str) -> Self {
        let parts: Vec<f64> = s
            .split_whitespace()
            .filter_map(|p| p.parse::<f64>().ok())
            .collect();
        if parts.len() >= 6 {
            Self {
                a: parts[0],
                b: parts[1],
                c: parts[2],
                d: parts[3],
                tx: parts[4],
                ty: parts[5],
            }
        } else {
            Self::default()
        }
    }

    pub fn multiply(&self, other: &Self) -> Self {
        Self {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            tx: self.a * other.tx + self.c * other.ty + self.tx,
            ty: self.b * other.tx + self.d * other.ty + self.ty,
        }
    }

    pub fn apply(&self, pt: Point2D) -> Point2D {
        Point2D::new(
            self.a * pt.x + self.c * pt.y + self.tx,
            self.b * pt.x + self.d * pt.y + self.ty,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LbVertex {
    pub pt: Point2D,
    pub c0: Point2D, // outgoing control point for Bezier
    pub c1: Point2D, // incoming control point for Bezier
}

#[derive(Debug, Clone)]
pub enum LbPrimCmd {
    Line(usize, usize),
    Bezier(usize, usize),
}

/// Parses LightBurn VertList string (e.g. "V383.77 174.42c0x1c1x382.58c1y174.38V...")
pub fn parse_vert_list(s: &str) -> Vec<LbVertex> {
    let mut vertices = Vec::new();

    for raw in s.split('V') {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Coordinates x y are everything before the first 'c'
        let mut parts = trimmed.split('c');
        let coord_part = parts.next().unwrap_or("").trim();
        let coords: Vec<&str> = coord_part.split_whitespace().collect();
        if coords.len() < 2 {
            continue;
        }

        let vx = coords[0].parse::<f64>().unwrap_or(0.0);
        let vy = coords[1].parse::<f64>().unwrap_or(0.0);
        let mut c0x = vx;
        let mut c0y = vy;
        let mut c1x = vx;
        let mut c1y = vy;

        for part in parts {
            if part.starts_with("0x") {
                let val_str = &part[2..];
                if val_str != "1" {
                    if let Ok(v) = val_str.parse::<f64>() {
                        c0x = v;
                    }
                }
            } else if part.starts_with("0y") {
                let val_str = &part[2..];
                if val_str != "1" {
                    if let Ok(v) = val_str.parse::<f64>() {
                        c0y = v;
                    }
                }
            } else if part.starts_with("1x") {
                let val_str = &part[2..];
                if val_str != "1" {
                    if let Ok(v) = val_str.parse::<f64>() {
                        c1x = v;
                    }
                }
            } else if part.starts_with("1y") {
                let val_str = &part[2..];
                if val_str != "1" {
                    if let Ok(v) = val_str.parse::<f64>() {
                        c1y = v;
                    }
                }
            }
        }

        vertices.push(LbVertex {
            pt: Point2D::new(vx, vy),
            c0: Point2D::new(c0x, c0y),
            c1: Point2D::new(c1x, c1y),
        });
    }

    vertices
}

/// Parses LightBurn PrimList string (e.g. "L0 1B1 2B2 3... 0")
pub fn parse_prim_list(s: &str) -> Vec<LbPrimCmd> {
    parse_prim_list_with_verts(s, 0)
}

/// Parses LightBurn PrimList string with vertex count context (supporting "LineClosed" and "LineOpen")
pub fn parse_prim_list_with_verts(s: &str, num_verts: usize) -> Vec<LbPrimCmd> {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("LineClosed") && num_verts > 1 {
        let mut cmds = Vec::with_capacity(num_verts);
        for i in 0..num_verts {
            cmds.push(LbPrimCmd::Line(i, (i + 1) % num_verts));
        }
        return cmds;
    }
    if trimmed.eq_ignore_ascii_case("LineOpen") && num_verts > 1 {
        let mut cmds = Vec::with_capacity(num_verts - 1);
        for i in 0..num_verts - 1 {
            cmds.push(LbPrimCmd::Line(i, i + 1));
        }
        return cmds;
    }

    let mut cmds = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let ch = bytes[i];
        if ch == b'L' || ch == b'B' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let start_from = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let from: usize = std::str::from_utf8(&bytes[start_from..i])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let start_to = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let to: usize = std::str::from_utf8(&bytes[start_to..i])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            if ch == b'L' {
                cmds.push(LbPrimCmd::Line(from, to));
            } else {
                cmds.push(LbPrimCmd::Bezier(from, to));
            }
        } else {
            i += 1;
        }
    }

    cmds
}

fn convert_path_to_polylines(
    verts: &[LbVertex],
    prims: &[LbPrimCmd],
    xform: Transform2D,
) -> Vec<Polyline> {
    if verts.is_empty() {
        return Vec::new();
    }

    if prims.is_empty() {
        let mut points: Vec<Point2D> = Vec::with_capacity(verts.len());
        for v in verts {
            points.push(xform.apply(v.pt));
        }
        let is_closed = verts.len() > 2 && verts[0].pt.distance_to(&verts.last().unwrap().pt) <= 1e-2;
        return vec![Polyline::new(points, is_closed)];
    }

    let mut result = Vec::new();
    let mut current_pts: Vec<Point2D> = Vec::new();
    let mut subpath_start: Option<usize> = None;
    let mut last_to: Option<usize> = None;

    for cmd in prims {
        let (from, to, is_bezier) = match *cmd {
            LbPrimCmd::Line(f, t) => (f, t, false),
            LbPrimCmd::Bezier(f, t) => (f, t, true),
        };

        if from >= verts.len() || to >= verts.len() {
            continue;
        }

        // If starting a new subpath or discontinuous jump:
        if subpath_start.is_some() && last_to != Some(from) {
            if current_pts.len() >= 2 {
                let closed = last_to == subpath_start;
                result.push(Polyline::new(std::mem::take(&mut current_pts), closed));
            } else {
                current_pts.clear();
            }
            subpath_start = None;
        }

        if subpath_start.is_none() {
            subpath_start = Some(from);
            let p0 = xform.apply(verts[from].pt);
            current_pts.push(p0);
        }

        let p0 = *current_pts.last().unwrap();
        let p_to = xform.apply(verts[to].pt);

        if is_bezier {
            let p1 = xform.apply(verts[from].c0);
            let p2 = xform.apply(verts[to].c1);
            tessellate_cubic_bezier(p0, p1, p2, p_to, 0.5, &mut current_pts);
        } else {
            current_pts.push(p_to);
        }

        last_to = Some(to);

        // If this segment closes the subpath
        if Some(to) == subpath_start {
            if current_pts.len() >= 2 {
                result.push(Polyline::new(std::mem::take(&mut current_pts), true));
            } else {
                current_pts.clear();
            }
            subpath_start = None;
            last_to = None;
        }
    }

    if current_pts.len() >= 2 {
        let closed = subpath_start.is_some() && last_to == subpath_start;
        result.push(Polyline::new(current_pts, closed));
    }

    result
}

pub struct Lbrn2File;

impl Lbrn2File {
    /// Loads a LightBurn project (.lbrn2 or .lbrn) from a string
    pub fn load_from_str(content: &str, doc_name: &str) -> Result<Document, String> {
        if content.trim_start().starts_with('<') {
            Self::parse_xml(content, doc_name)
        } else {
            Self::parse_json(content, doc_name)
        }
    }

    /// Loads a LightBurn project (.lbrn2 or .lbrn) XML or JSON file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Document, String> {
        let mut file = File::open(path.as_ref())
            .map_err(|e| format!("Failed to open LightBurn file: {}", e))?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| format!("Failed to read LightBurn file: {}", e))?;

        let doc_name = path
            .as_ref()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("LightBurn_Project");

        Self::load_from_str(&content, doc_name)
    }

    fn parse_xml(content: &str, doc_name: &str) -> Result<Document, String> {
        let doc_xml = roxmltree::Document::parse(content)
            .map_err(|e| format!("XML parse error in LightBurn file: {}", e))?;

        let root = doc_xml.root_element();
        if !root.has_tag_name("LightBurnProject") {
            return Err("Invalid LightBurn file: missing <LightBurnProject> root element".to_string());
        }

        let mut doc = Document::new(doc_name, 400.0, 400.0);
        // Clear default dummy layers so only layers actually defined or used in the file exist!
        doc.layers.clear();

        // 1. Parse CutSettings (case-insensitive for robust LightBurn version compatibility)
        for cs_node in root.descendants().filter(|n| n.tag_name().name().eq_ignore_ascii_case("CutSetting")) {
            let cut_type = cs_node
                .attributes()
                .find(|a| a.name().eq_ignore_ascii_case("type"))
                .map(|a| a.value())
                .unwrap_or("Cut");

            let find_child_attr = |tags: &[&str], attr_name: &str| -> Option<String> {
                for child in cs_node.children() {
                    let child_tag = child.tag_name().name();
                    if tags.iter().any(|&t| child_tag.eq_ignore_ascii_case(t)) {
                        if let Some(val) = child.attributes().find(|a| a.name().eq_ignore_ascii_case(attr_name)).map(|a| a.value()) {
                            return Some(val.to_string());
                        }
                        if let Some(text) = child.text() {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                return Some(trimmed.to_string());
                            }
                        }
                    }
                }
                for attr in cs_node.attributes() {
                    if tags.iter().any(|&t| attr.name().eq_ignore_ascii_case(t)) {
                        return Some(attr.value().to_string());
                    }
                }
                None
            };

            let get_val_f64 = |tags: &[&str], default: f64| -> f64 {
                find_child_attr(tags, "Value")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(default)
            };

            let get_val_u32 = |tags: &[&str], default: u32| -> u32 {
                find_child_attr(tags, "Value")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(default)
            };

            let idx = get_val_u32(&["index", "Index", "CutIndex"], 0) as usize;
            let layer = doc.get_or_create_layer(idx);

            if let Some(name) = find_child_attr(&["name", "Name"], "Value") {
                layer.name = name;
            }
            layer.cut_setting.speed = get_val_f64(&["speed", "Speed"], 50.0);
            layer.cut_setting.power_min = get_val_f64(&["minPower", "MinPower", "min_power", "power_min"], 10.0);
            layer.cut_setting.power_max = get_val_f64(&["maxPower", "MaxPower", "power", "Power", "max_power"], 80.0);
            layer.cut_setting.num_passes = get_val_u32(&["numPasses", "NumPasses", "passes", "Passes"], 1);
            layer.cut_setting.z_offset = get_val_f64(&["zOffset", "ZOffset"], 0.0);
            layer.cut_setting.air_assist =
                get_val_u32(&["enableAir", "EnableAir", "airAssist", "AirAssist"], 0) > 0;
            layer.cut_setting.kerf = get_val_f64(&["kerf", "Kerf"], 0.0);
            layer.cut_setting.line_interval = get_val_f64(&["interval", "Interval", "lineInterval", "LineInterval"], 0.08);

            if cut_type.eq_ignore_ascii_case("Scan") || cut_type.eq_ignore_ascii_case("Scan+Cut") {
                layer.cut_setting.mode = CutMode::Fill;
            } else if cut_type.eq_ignore_ascii_case("Image") {
                layer.cut_setting.mode = CutMode::Image;
            } else {
                layer.cut_setting.mode = CutMode::Line;
            }
        }

        // 2. Collect Shared PrimLists by PrimID
        let mut shared_prims: HashMap<String, Vec<LbPrimCmd>> = HashMap::new();
        for node in root.descendants().filter(|n| n.has_tag_name("Shape")) {
            if let Some(pid) = node.attribute("PrimID") {
                if let Some(pl_node) = node.children().find(|n| n.has_tag_name("PrimList")) {
                    if let Some(text) = pl_node.text() {
                        shared_prims.insert(pid.to_string(), parse_prim_list(text));
                    }
                }
            }
        }

        // 3. Recursively Parse Shapes (with group CutIndex inheritance)
        Self::parse_shapes_recursive(root, Transform2D::default(), &shared_prims, &mut doc, None);

        // In LightBurn, coordinates are stored in Cartesian space where Y+ points up.
        // On PhotonCAM's top-down canvas (and standard screen coordinates where Y+ points down),
        // we invert Y around the bounding box vertical center to preserve identical visual orientation
        // as seen on LightBurn's display window, exactly matching DXF and PDF import behavior.
        if !doc.shapes.is_empty() {
            let b = doc.bounds();
            let mid_y = b.min_y + b.height() * 0.5;
            for shape in &mut doc.shapes {
                Self::flip_shape_y(shape, mid_y);
            }
        }

        // Ensure all layers referenced by shapes exist in doc.layers
        let shape_layer_indices: Vec<usize> = doc.shapes.iter().map(|s| s.layer_index()).collect();
        for idx in shape_layer_indices {
            doc.get_or_create_layer(idx);
        }
        if doc.layers.is_empty() {
            doc.get_or_create_layer(0);
        }

        // Keep authentic coordinates and bed size intact! Do not shift shapes or resize bed.
        Ok(doc)
    }

    fn flip_shape_y(shape: &mut Shape, mid_y: f64) {
        match shape {
            Shape::Rect { y, height, .. } => {
                *y = 2.0 * mid_y - (*y + *height);
            }
            Shape::Circle { center, .. } => {
                center.y = 2.0 * mid_y - center.y;
            }
            Shape::Path { polyline, .. } => {
                for pt in &mut polyline.points {
                    pt.y = 2.0 * mid_y - pt.y;
                }
            }
            Shape::Group { shapes, .. } => {
                for s in shapes {
                    Self::flip_shape_y(s, mid_y);
                }
            }
            Shape::Image { y, height, .. } => {
                *y = 2.0 * mid_y - (*y + *height);
            }
        }
    }

    fn parse_shapes_recursive(
        parent_node: roxmltree::Node,
        parent_xform: Transform2D,
        shared_prims: &HashMap<String, Vec<LbPrimCmd>>,
        doc: &mut Document,
        inherited_cut_index: Option<usize>,
    ) {
        for node in parent_node.children() {
            if !node.has_tag_name("Shape") {
                // If it's a wrapper like <Children>, recurse through it
                if node.tag_name().name().eq_ignore_ascii_case("Children") {
                    Self::parse_shapes_recursive(node, parent_xform, shared_prims, doc, inherited_cut_index);
                }
                continue;
            }

            let shape_type = node
                .attributes()
                .find(|a| a.name().eq_ignore_ascii_case("type"))
                .map(|a| a.value())
                .unwrap_or("Path");

            let local_cut_index = node
                .attributes()
                .find(|a| a.name().eq_ignore_ascii_case("cutindex"))
                .and_then(|a| a.value().parse::<usize>().ok());

            let cut_index = local_cut_index.or(inherited_cut_index).unwrap_or(0);

            // Compute local and cumulative Transform2D
            let xform_str = node
                .children()
                .find(|n| n.tag_name().name().eq_ignore_ascii_case("xform"))
                .and_then(|n| n.text())
                .unwrap_or("");
            let local_xform = if !xform_str.is_empty() {
                Transform2D::parse(xform_str)
            } else {
                Transform2D::default()
            };
            let cur_xform = parent_xform.multiply(&local_xform);

            match shape_type {
                "Group" => {
                    // Recurse into children with current cumulative transform and cut_index
                    Self::parse_shapes_recursive(node, cur_xform, shared_prims, doc, Some(cut_index));
                }
                "Path" => {
                    // Check modern <VertList>
                    if let Some(vl_node) = node.children().find(|n| n.has_tag_name("VertList")) {
                        if let Some(vl_text) = vl_node.text() {
                            let verts = parse_vert_list(vl_text);

                            // Find PrimList either directly or via PrimID
                            let prims = if let Some(pl_node) =
                                node.children().find(|n| n.has_tag_name("PrimList"))
                            {
                                pl_node.text().map(|s| parse_prim_list_with_verts(s, verts.len())).unwrap_or_default()
                            } else if let Some(pid) = node.attribute("PrimID") {
                                shared_prims.get(pid).cloned().unwrap_or_default()
                            } else {
                                Vec::new()
                            };

                            let polylines = convert_path_to_polylines(&verts, &prims, cur_xform);
                            for polyline in polylines {
                                if !polyline.points.is_empty() {
                                    doc.add_shape(Shape::Path {
                                        layer_index: cut_index,
                                        polyline,
                                    });
                                }
                            }
                        }
                    } else {
                        // Check legacy <V> child elements (LightBurn FormatVersion 0 / 0.9.x)
                        let v_nodes: Vec<_> = node.children().filter(|n| n.has_tag_name("V")).collect();
                        if !v_nodes.is_empty() {
                            let mut verts = Vec::new();
                            for vn in v_nodes {
                                let vx = vn.attribute("vx").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                                let vy = vn.attribute("vy").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);

                                let c0x = vn.attribute("c0x").and_then(|s| s.parse::<f64>().ok()).unwrap_or(vx);
                                let c0y = vn.attribute("c0y").and_then(|s| s.parse::<f64>().ok()).unwrap_or(vy);
                                let c1x = vn.attribute("c1x").and_then(|s| s.parse::<f64>().ok()).unwrap_or(vx);
                                let c1y = vn.attribute("c1y").and_then(|s| s.parse::<f64>().ok()).unwrap_or(vy);

                                verts.push(LbVertex {
                                    pt: Point2D::new(vx, vy),
                                    c0: Point2D::new(c0x, c0y),
                                    c1: Point2D::new(c1x, c1y),
                                });
                            }

                            let p_nodes: Vec<_> = node.children().filter(|n| n.has_tag_name("P")).collect();
                            let mut prims = Vec::new();
                            for pn in p_nodes {
                                let t = pn.attribute("T").unwrap_or("L");
                                let p0 = pn.attribute("p0").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
                                let p1 = pn.attribute("p1").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
                                if t == "B" {
                                    prims.push(LbPrimCmd::Bezier(p0, p1));
                                } else {
                                    prims.push(LbPrimCmd::Line(p0, p1));
                                }
                            }

                            let polylines = convert_path_to_polylines(&verts, &prims, cur_xform);
                            for polyline in polylines {
                                if !polyline.points.is_empty() {
                                    doc.add_shape(Shape::Path {
                                        layer_index: cut_index,
                                        polyline,
                                    });
                                }
                            }
                        }
                    }
                }
                "Rect" => {
                    let w = node
                        .attribute("W")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let h = node
                        .attribute("H")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let cr = node
                        .attribute("Cr")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);

                    if w > 0.0 && h > 0.0 {
                        let hw = w * 0.5;
                        let hh = h * 0.5;
                        let p0 = cur_xform.apply(Point2D::new(-hw, -hh));
                        let p1 = cur_xform.apply(Point2D::new(hw, -hh));
                        let p2 = cur_xform.apply(Point2D::new(hw, hh));
                        let p3 = cur_xform.apply(Point2D::new(-hw, hh));

                        // If axis-aligned without rotation:
                        if cur_xform.b.abs() < 1e-6 && cur_xform.c.abs() < 1e-6 {
                            let min_x = p0.x.min(p1.x).min(p2.x).min(p3.x);
                            let min_y = p0.y.min(p1.y).min(p2.y).min(p3.y);
                            doc.add_shape(Shape::Rect {
                                layer_index: cut_index,
                                x: min_x,
                                y: min_y,
                                width: w * cur_xform.a.abs(),
                                height: h * cur_xform.d.abs(),
                                corner_radius: cr,
                            });
                        } else {
                            // Rotated rect -> Polygon
                            let poly = Polyline::new(vec![p0, p1, p2, p3], true);
                            doc.add_shape(Shape::Path {
                                layer_index: cut_index,
                                polyline: poly,
                            });
                        }
                    }
                }
                "Circle" | "Ellipse" => {
                    let r = node
                        .attribute("R")
                        .and_then(|s| s.parse::<f64>().ok())
                        .or_else(|| {
                            node.attribute("Rx").and_then(|s| s.parse::<f64>().ok())
                        })
                        .or_else(|| {
                            node.attribute("W")
                                .and_then(|s| s.parse::<f64>().ok())
                                .map(|w| w * 0.5)
                        })
                        .unwrap_or(0.0);

                    if r > 0.0 {
                        let center = cur_xform.apply(Point2D::new(0.0, 0.0));
                        let scale = (cur_xform.a.abs() + cur_xform.d.abs()) * 0.5;
                        doc.add_shape(Shape::Circle {
                            layer_index: cut_index,
                            center,
                            radius: r * scale,
                        });
                    }
                }
                "Text" => {
                    // 1. First priority: Check if text has <BackupPath> (which contains exact vector curves of characters)
                    let mut loaded_from_backup = false;
                    if let Some(bp_node) = node
                        .children()
                        .find(|n| n.tag_name().name().eq_ignore_ascii_case("BackupPath"))
                    {
                        if let Some(vl_node) = bp_node.children().find(|n| n.has_tag_name("VertList")) {
                            if let Some(vl_text) = vl_node.text() {
                                let verts = parse_vert_list(vl_text);
                                let prims = if let Some(pl_node) =
                                    bp_node.children().find(|n| n.has_tag_name("PrimList"))
                                {
                                    pl_node
                                        .text()
                                        .map(|s| parse_prim_list_with_verts(s, verts.len()))
                                        .unwrap_or_default()
                                } else if let Some(pid) = bp_node.attribute("PrimID") {
                                    shared_prims.get(pid).cloned().unwrap_or_default()
                                } else {
                                    Vec::new()
                                };

                                // In LightBurn, BackupPath XForm is the pre-computed world transform.
                                let bp_xform = if let Some(xf_node) =
                                    bp_node.children().find(|n| n.has_tag_name("XForm"))
                                {
                                    xf_node.text().map(Transform2D::parse).unwrap_or(cur_xform)
                                } else {
                                    cur_xform
                                };

                                let polylines = convert_path_to_polylines(&verts, &prims, bp_xform);
                                if !polylines.is_empty() {
                                    for poly in polylines {
                                        if !poly.points.is_empty() {
                                            doc.add_shape(Shape::Path {
                                                layer_index: cut_index,
                                                polyline: poly,
                                            });
                                        }
                                    }
                                    loaded_from_backup = true;
                                }
                            }
                        }
                    }

                    // 2. Fallback: If no BackupPath was present (e.g. AirPurifier.lbrn2), synthesize letter polylines using text engine
                    if !loaded_from_backup {
                        if let Some(text_str) = node.attribute("Str") {
                            let font_h = node
                                .attribute("H")
                                .and_then(|s| s.parse::<f64>().ok())
                                .unwrap_or(10.0);
                            let ah = node
                                .attribute("Ah")
                                .and_then(|s| s.parse::<u32>().ok())
                                .unwrap_or(1);
                            let av = node
                                .attribute("Av")
                                .and_then(|s| s.parse::<u32>().ok())
                                .unwrap_or(1);

                            let align = match ah {
                                0 => crate::text::TextAlign::Left,
                                2 => crate::text::TextAlign::Right,
                                _ => crate::text::TextAlign::Center,
                            };

                            let default_font = crate::text::default_system_font();
                            let font_type = if !default_font.is_empty() {
                                crate::text::FontType::TrueType { path: default_font }
                            } else {
                                crate::text::FontType::SingleLine
                            };

                            let options = crate::text::TextOptions {
                                text: text_str.to_string(),
                                font_type: font_type.clone(),
                                font_size_mm: font_h,
                                x: 0.0,
                                y: 0.0,
                                align,
                                letter_spacing: 1.0,
                                line_spacing: 1.2,
                            };

                            let polylines = crate::text::generate_text_polylines(&options)
                                .or_else(|_| {
                                    let fallback_opts = crate::text::TextOptions {
                                        font_type: crate::text::FontType::SingleLine,
                                        ..options
                                    };
                                    crate::text::generate_text_polylines(&fallback_opts)
                                })
                                .unwrap_or_default();

                            let y_offset = match av {
                                0 => 0.0,
                                2 => -font_h,
                                _ => -font_h * 0.5,
                            };

                            for poly in polylines {
                                let world_pts: Vec<Point2D> = poly
                                    .points
                                    .iter()
                                    .map(|p| {
                                        let py = -p.y;
                                        cur_xform.apply(Point2D::new(p.x, py + y_offset))
                                    })
                                    .collect();
                                if !world_pts.is_empty() {
                                    doc.add_shape(Shape::Path {
                                        layer_index: cut_index,
                                        polyline: Polyline::new(world_pts, poly.is_closed),
                                    });
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Legacy JSON fallback parser
    fn parse_json(content: &str, doc_name: &str) -> Result<Document, String> {
        #[derive(Deserialize)]
        struct LegacyLbrn {
            #[serde(rename = "CutSettings", default)]
            cut_settings: Vec<LegacyCutSetting>,
            #[serde(default)]
            shapes: Vec<LegacyShape>,
        }
        #[derive(Deserialize)]
        struct LegacyCutSetting {
            #[serde(default)]
            index: usize,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            speed: Option<f64>,
            #[serde(rename = "minPower", default)]
            min_power: Option<f64>,
            #[serde(rename = "maxPower", default)]
            max_power: Option<f64>,
            #[serde(rename = "numPasses", default)]
            num_passes: Option<u32>,
            #[serde(rename = "airAssist", default)]
            air_assist: Option<bool>,
            #[serde(rename = "type", default)]
            cut_type: Option<String>,
        }
        #[derive(Deserialize)]
        struct LegacyShape {
            #[serde(rename = "Type")]
            shape_type: String,
            #[serde(rename = "CutIndex", default)]
            cut_index: usize,
            #[serde(rename = "Points", default)]
            points: Option<Vec<Point2D>>,
            #[serde(rename = "Closed", default)]
            closed: bool,
            #[serde(rename = "X", default)]
            x: f64,
            #[serde(rename = "Y", default)]
            y: f64,
            #[serde(rename = "Width", default)]
            width: f64,
            #[serde(rename = "Height", default)]
            height: f64,
            #[serde(rename = "Radius", default)]
            radius: f64,
            #[serde(rename = "Text", default)]
            text: Option<String>,
        }

        let lbrn: LegacyLbrn = serde_json::from_str(content)
            .map_err(|e| format!("Failed to parse LightBurn file: {}", e))?;

        let mut doc = Document::new(doc_name, 400.0, 400.0);
        if !lbrn.cut_settings.is_empty() {
            doc.layers.clear();
            for cs in lbrn.cut_settings {
                let layer = doc.get_or_create_layer(cs.index);
                if let Some(n) = cs.name {
                    layer.name = n;
                }
                if let Some(spd) = cs.speed {
                    layer.cut_setting.speed = spd;
                }
                if let Some(pmin) = cs.min_power {
                    layer.cut_setting.power_min = pmin;
                }
                if let Some(pmax) = cs.max_power {
                    layer.cut_setting.power_max = pmax;
                }
                if let Some(np) = cs.num_passes {
                    layer.cut_setting.num_passes = np;
                }
                if let Some(aa) = cs.air_assist {
                    layer.cut_setting.air_assist = aa;
                }
                if let Some(ct) = cs.cut_type {
                    if ct.eq_ignore_ascii_case("Scan") || ct.eq_ignore_ascii_case("Fill") {
                        layer.cut_setting.mode = CutMode::Fill;
                    } else if ct.eq_ignore_ascii_case("Image") {
                        layer.cut_setting.mode = CutMode::Image;
                    } else {
                        layer.cut_setting.mode = CutMode::Line;
                    }
                }
            }
        }
        for s in lbrn.shapes {
            match s.shape_type.as_str() {
                "Path" => {
                    if let Some(pts) = s.points {
                        doc.add_shape(Shape::Path {
                            layer_index: s.cut_index,
                            polyline: Polyline::new(pts, s.closed),
                        });
                    }
                }
                "Rect" => {
                    doc.add_shape(Shape::Rect {
                        layer_index: s.cut_index,
                        x: s.x,
                        y: s.y,
                        width: s.width,
                        height: s.height,
                        corner_radius: s.radius,
                    });
                }
                "Ellipse" | "Circle" => {
                    doc.add_shape(Shape::Circle {
                        layer_index: s.cut_index,
                        center: Point2D::new(s.x + s.width * 0.5, s.y + s.height * 0.5),
                        radius: (s.width * 0.5 + s.height * 0.5) * 0.5,
                    });
                }
                "Text" => {
                    if let Some(txt) = s.text {
                        let font_h = if s.height > 0.0 { s.height } else { 10.0 };
                        let default_font = crate::text::default_system_font();
                        let font_type = if !default_font.is_empty() {
                            crate::text::FontType::TrueType { path: default_font }
                        } else {
                            crate::text::FontType::SingleLine
                        };
                        let options = crate::text::TextOptions {
                            text: txt,
                            font_type: font_type.clone(),
                            font_size_mm: font_h,
                            x: s.x,
                            y: s.y,
                            align: crate::text::TextAlign::Left,
                            letter_spacing: 1.0,
                            line_spacing: 1.2,
                        };
                        if let Ok(polys) = crate::text::generate_text_polylines(&options) {
                            for poly in polys {
                                doc.add_shape(Shape::Path {
                                    layer_index: s.cut_index,
                                    polyline: poly,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let shape_layer_indices: Vec<usize> = doc.shapes.iter().map(|s| s.layer_index()).collect();
        for idx in shape_layer_indices {
            doc.get_or_create_layer(idx);
        }
        if doc.layers.is_empty() {
            doc.get_or_create_layer(0);
        }

        Ok(doc)
    }

    /// Saves a laser_burn Document as standard LightBurn .lbrn2 XML
    pub fn save_to_file<P: AsRef<Path>>(doc: &Document, path: P) -> Result<(), String> {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<LightBurnProject AppVersion=\"1.6.00\" FormatVersion=\"1\" MaterialHeight=\"0\">\n");

        // Write CutSettings
        for (idx, layer) in &doc.layers {
            let cs_type = match layer.cut_setting.mode {
                CutMode::Line => "Cut",
                CutMode::Fill | CutMode::OffsetFill => "Scan",
                CutMode::Image => "Image",
            };
            xml.push_str(&format!("    <CutSetting type=\"{}\">\n", cs_type));
            xml.push_str(&format!("        <index Value=\"{}\"/>\n", idx));
            xml.push_str(&format!("        <name Value=\"{}\"/>\n", layer.name));
            xml.push_str(&format!("        <minPower Value=\"{:.1}\"/>\n", layer.cut_setting.power_min));
            xml.push_str(&format!("        <maxPower Value=\"{:.1}\"/>\n", layer.cut_setting.power_max));
            xml.push_str(&format!("        <speed Value=\"{:.1}\"/>\n", layer.cut_setting.speed));
            xml.push_str(&format!("        <priority Value=\"{}\"/>\n", layer.priority));
            xml.push_str(&format!("        <numPasses Value=\"{}\"/>\n", layer.cut_setting.num_passes));
            xml.push_str(&format!("        <zOffset Value=\"{:.2}\"/>\n", layer.cut_setting.z_offset));
            xml.push_str(&format!("        <enableAir Value=\"{}\"/>\n", if layer.cut_setting.air_assist { 1 } else { 0 }));
            xml.push_str(&format!("        <kerf Value=\"{:.3}\"/>\n", layer.cut_setting.kerf));
            xml.push_str(&format!("        <lineInterval Value=\"{:.3}\"/>\n", layer.cut_setting.line_interval));
            xml.push_str("    </CutSetting>\n");
        }

        // Write Shapes
        for shape in &doc.shapes {
            Self::export_shape_xml(shape, &mut xml, 4);
        }

        xml.push_str("</LightBurnProject>\n");

        let mut file = File::create(path.as_ref())
            .map_err(|e| format!("Failed to create .lbrn2 file: {}", e))?;
        file.write_all(xml.as_bytes())
            .map_err(|e| format!("Failed to write .lbrn2 file: {}", e))?;

        Ok(())
    }

    fn export_shape_xml(shape: &Shape, out: &mut String, indent: usize) {
        let pad = " ".repeat(indent);
        match shape {
            Shape::Path { layer_index, polyline } => {
                out.push_str(&format!("{}<Shape Type=\"Path\" CutIndex=\"{}\">\n", pad, layer_index));
                out.push_str(&format!("{}    <XForm>1 0 0 1 0 0</XForm>\n", pad));

                let mut vl = String::new();
                let mut pl = String::new();

                for (i, p) in polyline.points.iter().enumerate() {
                    vl.push_str(&format!("V{:.3} {:.3}c0x1c1x1", p.x, p.y));
                    if i < polyline.points.len() - 1 {
                        pl.push_str(&format!("L{} {} ", i, i + 1));
                    }
                }
                if polyline.is_closed && polyline.points.len() > 2 {
                    pl.push_str(&format!("L{} 0", polyline.points.len() - 1));
                }

                out.push_str(&format!("{}    <VertList>{}</VertList>\n", pad, vl));
                out.push_str(&format!("{}    <PrimList>{}</PrimList>\n", pad, pl.trim()));
                out.push_str(&format!("{}</Shape>\n", pad));
            }
            Shape::Rect { layer_index, x, y, width, height, corner_radius } => {
                let cx = x + width * 0.5;
                let cy = y + height * 0.5;
                out.push_str(&format!(
                    "{}<Shape Type=\"Rect\" CutIndex=\"{}\" W=\"{:.3}\" H=\"{:.3}\" Cr=\"{:.3}\">\n",
                    pad, layer_index, width, height, corner_radius
                ));
                out.push_str(&format!("{}    <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n", pad, cx, cy));
                out.push_str(&format!("{}</Shape>\n", pad));
            }
            Shape::Circle { layer_index, center, radius } => {
                out.push_str(&format!(
                    "{}<Shape Type=\"Circle\" CutIndex=\"{}\" R=\"{:.3}\">\n",
                    pad, layer_index, radius
                ));
                out.push_str(&format!("{}    <XForm>1 0 0 1 {:.3} {:.3}</XForm>\n", pad, center.x, center.y));
                out.push_str(&format!("{}</Shape>\n", pad));
            }
            Shape::Group { layer_index, shapes, .. } => {
                out.push_str(&format!("{}<Shape Type=\"Group\" CutIndex=\"{}\">\n", pad, layer_index));
                out.push_str(&format!("{}    <XForm>1 0 0 1 0 0</XForm>\n", pad));
                out.push_str(&format!("{}    <Children>\n", pad));
                for child in shapes {
                    Self::export_shape_xml(child, out, indent + 8);
                }
                out.push_str(&format!("{}    </Children>\n", pad));
                out.push_str(&format!("{}</Shape>\n", pad));
            }
            Shape::Image { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vert_list() {
        let sample = "V383.77539 174.42822c0x1c1x382.58997c1y174.38266V384.37646 174.45264c0x386.78476c0y174.54138c1x1";
        let verts = parse_vert_list(sample);
        assert_eq!(verts.len(), 2);
        assert!((verts[0].pt.x - 383.77539).abs() < 1e-4);
        assert!((verts[0].pt.y - 174.42822).abs() < 1e-4);
        assert!((verts[0].c1.x - 382.58997).abs() < 1e-4);
        assert!((verts[0].c1.y - 174.38266).abs() < 1e-4);
        assert!((verts[1].c0.x - 386.78476).abs() < 1e-4);
        assert!((verts[1].c0.y - 174.54138).abs() < 1e-4);
    }

    #[test]
    fn test_parse_prim_list() {
        let sample = "L0 1B1 2B2 3L3 0";
        let prims = parse_prim_list(sample);
        assert_eq!(prims.len(), 4);
        assert!(matches!(prims[0], LbPrimCmd::Line(0, 1)));
        assert!(matches!(prims[1], LbPrimCmd::Bezier(1, 2)));
        assert!(matches!(prims[2], LbPrimCmd::Bezier(2, 3)));
        assert!(matches!(prims[3], LbPrimCmd::Line(3, 0)));
    }

    #[test]
    fn test_transform_2d() {
        let t1 = Transform2D::parse("1 0 0 1 100 50");
        let pt = Point2D::new(10.0, 20.0);
        let transformed = t1.apply(pt);
        assert_eq!(transformed.x, 110.0);
        assert_eq!(transformed.y, 70.0);
    }

    #[test]
    fn test_load_real_lbrn2_file() {
        let path = "/Volumes/Jättitila/Laser/testi.lbrn2";
        if std::path::Path::new(path).exists() {
            let doc = Lbrn2File::load_from_file(path).expect("Should load testi.lbrn2");
            assert_eq!(doc.shapes.len(), 6, "Should contain 6 shapes");
            assert_eq!(doc.layers.len(), 6, "Should contain 6 layers");
            let layer1 = doc.layers.get(&1).expect("Layer 1 should exist");
            assert_eq!(layer1.name, "C01");
            assert_eq!(layer1.cut_setting.mode, CutMode::Fill);
            assert_eq!(layer1.cut_setting.power_max, 60.0);
            assert_eq!(layer1.cut_setting.speed, 16.6667);
        }

        let air_path = "/Volumes/Jättitila/Laser/AirPurifier.lbrn2";
        if std::path::Path::new(air_path).exists() {
            let doc = Lbrn2File::load_from_file(air_path).expect("Should load AirPurifier.lbrn2");
            assert!(doc.shapes.len() > 10, "Should contain shapes");
            assert!(doc.layers.contains_key(&0), "Should contain Layer 0");
            assert!(doc.layers.contains_key(&1), "Should contain Layer 1");
            assert!(doc.layers.contains_key(&3), "Should contain Layer 3");
            assert!(doc.layers.contains_key(&30), "Should contain Layer 30 (Tool T1)");
            assert_eq!(doc.layers.get(&0).unwrap().name, "Outer Cut");
            assert_eq!(doc.layers.get(&1).unwrap().name, "Inner Cut");
            assert_eq!(doc.layers.get(&3).unwrap().name, "Etch");
        }

        let mitta_path = "/Volumes/Jättitila/Laser/40mm_mitta.lbrn2";
        if std::path::Path::new(mitta_path).exists() {
            let doc = Lbrn2File::load_from_file(mitta_path).expect("Should load 40mm_mitta.lbrn2");
            // 1 ruler contour + 6 letter contours (4 outer, 4 inner, 0 outer, 0 inner, m, m)
            assert_eq!(doc.shapes.len(), 7, "Should contain ruler and letters 40mm");
            let text_shapes = &doc.shapes[1..];
            let min_x = text_shapes.iter().map(|s| s.bounds().min_x).fold(f64::INFINITY, f64::min);
            let max_x = text_shapes.iter().map(|s| s.bounds().max_x).fold(f64::NEG_INFINITY, f64::max);
            assert!(min_x < 195.0, "The '4' should be on the left (x < 195.0, got {})", min_x);
            assert!(max_x > 204.0, "The 'm' should be on the right (x > 204.0, got {})", max_x);

            // Verify ruler and text are right-side up (not flipped upside down)
            if let Shape::Path { polyline, .. } = &doc.shapes[0] {
                let left_tick_min_y = polyline.points.iter().filter(|p| p.x < 180.0).map(|p| p.y).fold(f64::INFINITY, f64::min);
                let right_tick_min_y = polyline.points.iter().filter(|p| p.x > 214.0).map(|p| p.y).fold(f64::INFINITY, f64::min);
                assert!(left_tick_min_y < right_tick_min_y, "Ruler left tick should be at the top (left Y {} < right Y {})", left_tick_min_y, right_tick_min_y);
            }
        }
    }

    #[test]
    fn test_load_legacy_lbrn_kerf_test() {
        let path = "/Volumes/Jättitila/Laser/Kerf Test (-0.5 to 0.5).lbrn";
        if std::path::Path::new(path).exists() {
            let doc = Lbrn2File::load_from_file(path).expect("Should load legacy .lbrn Kerf Test");
            assert!(doc.shapes.len() >= 47, "Kerf test should contain letter contours");
            let b = doc.bounds();
            assert!(b.width() > 100.0, "Width should be > 100mm");
            assert!(b.height() > 20.0, "Height should be > 20mm");
        }
    }

    #[test]
    fn test_save_and_reload_xml() {
        let mut doc = Document::new("TestProject", 300.0, 300.0);
        doc.add_shape(Shape::Rect {
            layer_index: 0,
            x: 20.0,
            y: 30.0,
            width: 50.0,
            height: 40.0,
            corner_radius: 0.0,
        });
        doc.add_shape(Shape::Circle {
            layer_index: 1,
            center: Point2D::new(100.0, 100.0),
            radius: 15.0,
        });

        let test_path = "/tmp/laserburn_test_save.lbrn2";
        Lbrn2File::save_to_file(&doc, test_path).expect("Should save .lbrn2 XML");

        let loaded = Lbrn2File::load_from_file(test_path).expect("Should reload .lbrn2 XML");
        assert_eq!(loaded.shapes.len(), 2);
    }

    #[test]
    fn test_xml_cut_settings_and_group_inheritance() {
        let xml_data = r#"<?xml version="1.0" encoding="UTF-8"?>
<LightBurnProject AppVersion="1.4.01" FormatVersion="1">
    <CutSetting type="Cut">
        <index Value="0"/>
        <name Value="C00 Red"/>
        <maxPower Value="85.5"/>
        <minPower Value="25.0"/>
        <speed Value="120.0"/>
        <numPasses Value="2"/>
        <enableAir Value="1"/>
    </CutSetting>
    <CutSetting type="Scan">
        <index Value="2"/>
        <name Value="C02 Blue"/>
        <maxPower Value="45.0"/>
        <speed Value="300.0"/>
    </CutSetting>
    <Shape Type="Group" CutIndex="2">
        <Children>
            <Shape Type="Rect" W="20" H="30">
                <XForm>1 0 0 1 10 10</XForm>
            </Shape>
        </Children>
    </Shape>
    <Shape Type="Rect" CutIndex="0" W="50" H="50">
        <XForm>1 0 0 1 100 100</XForm>
    </Shape>
</LightBurnProject>"#;

        let doc = Lbrn2File::parse_xml(xml_data, "test.lbrn2").expect("Parse XML should succeed");
        assert_eq!(doc.layers.len(), 2);
        
        let layer0 = doc.layers.get(&0).expect("Layer 0 should exist");
        assert_eq!(layer0.name, "C00 Red");
        assert_eq!(layer0.cut_setting.speed, 120.0);
        assert_eq!(layer0.cut_setting.power_max, 85.5);
        assert_eq!(layer0.cut_setting.power_min, 25.0);
        assert_eq!(layer0.cut_setting.num_passes, 2);
        assert!(layer0.cut_setting.air_assist);

        let layer2 = doc.layers.get(&2).expect("Layer 2 should exist");
        assert_eq!(layer2.name, "C02 Blue");
        assert_eq!(layer2.cut_setting.speed, 300.0);
        assert_eq!(layer2.cut_setting.power_max, 45.0);
        assert_eq!(layer2.cut_setting.mode, CutMode::Fill);

        // Shapes: child rect in group should have inherited CutIndex 2
        assert_eq!(doc.shapes.len(), 2);
        assert_eq!(doc.shapes[0].layer_index(), 2);
        assert_eq!(doc.shapes[1].layer_index(), 0);
    }

    #[test]
    fn test_xml_text_loading_backup_and_synthesized() {
        let xml_data = r#"<?xml version="1.0" encoding="UTF-8"?>
<LightBurnProject AppVersion="1.7.01" FormatVersion="1">
    <CutSetting type="Cut">
        <index Value="0"/>
        <name Value="Black"/>
    </CutSetting>
    <Shape Type="Text" CutIndex="0" Font="Arial" Str="OK" H="10.0" Ah="1" Av="1">
        <XForm>1 0 0 1 50 50</XForm>
    </Shape>
</LightBurnProject>"#;

        let doc = Lbrn2File::parse_xml(xml_data, "text_test.lbrn2").expect("Parse XML should succeed");
        // Letters "O" and "K" should produce vector contour shapes, NOT a dummy single box!
        assert!(doc.shapes.len() >= 2, "Text 'OK' should generate at least 2 glyph contours, got {}", doc.shapes.len());
        let b = doc.bounds();
        assert!(b.width() > 5.0 && b.width() < 50.0);
        assert!(b.height() > 5.0 && b.height() < 30.0);
    }
}

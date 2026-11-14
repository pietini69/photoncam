//! Text vectorization and font engine for LaserBurn.
//! Supports both TrueType/OpenType outline fonts and single-line stroke fonts for laser cutting and engraving.

use crate::geometry::{Point2D, Polyline};
use std::borrow::Cow;
use std::path::Path;

// Embedded Google Handwriting Fonts for Wood Engraving & Laser Cutting
pub static FONT_CAVEAT: &[u8] = include_bytes!("../../assets/fonts/Caveat.ttf");
pub static FONT_KALAM: &[u8] = include_bytes!("../../assets/fonts/Kalam.ttf");
pub static FONT_SATISFY: &[u8] = include_bytes!("../../assets/fonts/Satisfy.ttf");
pub static FONT_PATRICK_HAND: &[u8] = include_bytes!("../../assets/fonts/PatrickHand.ttf");
pub static FONT_ALEX_BRUSH: &[u8] = include_bytes!("../../assets/fonts/AlexBrush.ttf");

/// Load font bytes from either an embedded font identifier ("embedded:Caveat", etc.)
/// or a file system path.
pub fn load_font_bytes(path_or_id: &str) -> Result<Cow<'static, [u8]>, String> {
    if path_or_id.is_empty() {
        let def = default_system_font();
        if !def.is_empty() && Path::new(&def).exists() {
            return load_font_bytes(&def);
        }
        return Ok(Cow::Borrowed(FONT_CAVEAT));
    }

    match path_or_id {
        "embedded:Caveat" | "Caveat" => Ok(Cow::Borrowed(FONT_CAVEAT)),
        "embedded:Kalam" | "Kalam" => Ok(Cow::Borrowed(FONT_KALAM)),
        "embedded:Satisfy" | "Satisfy" => Ok(Cow::Borrowed(FONT_SATISFY)),
        "embedded:PatrickHand" | "Patrick Hand" | "PatrickHand" => Ok(Cow::Borrowed(FONT_PATRICK_HAND)),
        "embedded:AlexBrush" | "Alex Brush" | "AlexBrush" => Ok(Cow::Borrowed(FONT_ALEX_BRUSH)),
        path => {
            let data = std::fs::read(path)
                .map_err(|e| format!("Failed to read font file '{}': {}", path, e))?;
            Ok(Cow::Owned(data))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FontType {
    /// TrueType/OpenType outline font for closed cutting contours and hatch filling.
    TrueType {
        path: String,
    },
    /// Single-line stroke font (Hershey Simplex) for rapid laser marking/scoring without double cuts.
    SingleLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub struct TextOptions {
    pub text: String,
    pub font_type: FontType,
    pub font_size_mm: f64,
    pub x: f64,
    pub y: f64,
    pub align: TextAlign,
    pub letter_spacing: f64,
    pub line_spacing: f64,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            text: "PhotonCAM".to_string(),
            font_type: FontType::TrueType {
                path: default_system_font(),
            },
            font_size_mm: 15.0,
            x: 50.0,
            y: 50.0,
            align: TextAlign::Left,
            letter_spacing: 1.0,
            line_spacing: 1.2,
        }
    }
}

/// Find a standard default system font
pub fn default_system_font() -> String {
    let candidates = [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Supplemental/Courier New.ttf",
        "/Library/Fonts/Arial.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];

    for c in &candidates {
        if Path::new(c).exists() {
            return c.to_string();
        }
    }

    "embedded:Caveat".to_string()
}

/// List available system fonts, featuring embedded Google handwriting fonts for wood engraving
pub fn available_system_fonts() -> Vec<(&'static str, &'static str)> {
    let mut fonts = vec![
        // Handwriting / Puukaiverrus Google Fonts (Embedded, cross-platform)
        ("✍ Caveat (Luonnollinen käsiala / Puukaiverrus)", "embedded:Caveat"),
        ("✍ Kalam (Rento käsiala / Ainesosat & Mitat)", "embedded:Kalam"),
        ("✍ Satisfy (Perinteinen kaunokirjoitus / Otsikot)", "embedded:Satisfy"),
        ("✍ Patrick Hand (Selkeä tekstaus / Äidin reseptit)", "embedded:PatrickHand"),
        ("✍ Alex Brush (Koristeellinen kaunokirjoitus)", "embedded:AlexBrush"),
    ];
    let candidates = [
        ("Arial", "/System/Library/Fonts/Supplemental/Arial.ttf"),
        ("Arial Bold", "/System/Library/Fonts/Supplemental/Arial Bold.ttf"),
        ("Courier New", "/System/Library/Fonts/Supplemental/Courier New.ttf"),
        ("Times New Roman", "/System/Library/Fonts/Supplemental/Times New Roman.ttf"),
        ("Georgia", "/System/Library/Fonts/Supplemental/Georgia.ttf"),
        ("Verdana", "/System/Library/Fonts/Supplemental/Verdana.ttf"),
        ("Impact", "/System/Library/Fonts/Supplemental/Impact.ttf"),
        ("Comic Sans MS", "/System/Library/Fonts/Supplemental/Comic Sans MS.ttf"),
        ("Helvetica", "/System/Library/Fonts/Helvetica.ttc"),
    ];

    for (name, path) in candidates {
        if Path::new(path).exists() {
            fonts.push((name, path));
        }
    }
    fonts
}

struct OutlineCollector {
    polylines: Vec<Polyline>,
    current_pts: Vec<Point2D>,
    start_pt: Point2D,
    scale: f64,
    offset_x: f64,
    offset_y: f64,
}

impl OutlineCollector {
    fn new(scale: f64, offset_x: f64, offset_y: f64) -> Self {
        Self {
            polylines: Vec::new(),
            current_pts: Vec::new(),
            start_pt: Point2D::new(0.0, 0.0),
            scale,
            offset_x,
            offset_y,
        }
    }

    fn to_doc_pt(&self, x: f32, y: f32) -> Point2D {
        Point2D::new(
            self.offset_x + (x as f64) * self.scale,
            // Invert Y because TTF has Y+ going upwards, whereas LaserBurn bed has Y+ downwards
            self.offset_y - (y as f64) * self.scale,
        )
    }

    fn finish_subpath(&mut self, is_closed: bool) {
        if self.current_pts.len() >= 2 {
            let pts = std::mem::take(&mut self.current_pts);
            self.polylines.push(Polyline::new(pts, is_closed));
        } else {
            self.current_pts.clear();
        }
    }
}

impl ttf_parser::OutlineBuilder for OutlineCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish_subpath(true);
        let pt = self.to_doc_pt(x, y);
        self.start_pt = pt;
        self.current_pts.push(pt);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let pt = self.to_doc_pt(x, y);
        self.current_pts.push(pt);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = *self.current_pts.last().unwrap_or(&self.start_pt);
        let p1 = self.to_doc_pt(x1, y1);
        let p2 = self.to_doc_pt(x, y);

        // Subdivide quadratic Bézier into line segments
        let steps = 6;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let mt = 1.0 - t;
            let px = mt * mt * p0.x + 2.0 * mt * t * p1.x + t * t * p2.x;
            let py = mt * mt * p0.y + 2.0 * mt * t * p1.y + t * t * p2.y;
            self.current_pts.push(Point2D::new(px, py));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = *self.current_pts.last().unwrap_or(&self.start_pt);
        let p1 = self.to_doc_pt(x1, y1);
        let p2 = self.to_doc_pt(x2, y2);
        let p3 = self.to_doc_pt(x, y);

        // Subdivide cubic Bézier into line segments
        let steps = 8;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let mt = 1.0 - t;
            let px = mt * mt * mt * p0.x
                + 3.0 * mt * mt * t * p1.x
                + 3.0 * mt * t * t * p2.x
                + t * t * t * p3.x;
            let py = mt * mt * mt * p0.y
                + 3.0 * mt * mt * t * p1.y
                + 3.0 * mt * t * t * p2.y
                + t * t * t * p3.y;
            self.current_pts.push(Point2D::new(px, py));
        }
    }

    fn close(&mut self) {
        self.finish_subpath(true);
    }
}

/// Generates vector polylines from text options.
pub fn generate_text_polylines(options: &TextOptions) -> Result<Vec<Polyline>, String> {
    match &options.font_type {
        FontType::TrueType { path } => generate_ttf_polylines(options, path),
        FontType::SingleLine => generate_singleline_polylines(options),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextOnPathOptions {
    pub start_offset_mm: f64,
    pub perpendicular_offset_mm: f64,
    pub align: TextAlign,
    pub reverse_direction: bool,
}

impl Default for TextOnPathOptions {
    fn default() -> Self {
        Self {
            start_offset_mm: 0.0,
            perpendicular_offset_mm: 0.0,
            align: TextAlign::Center,
            reverse_direction: false,
        }
    }
}

/// Generates vector polylines flowing along an arbitrary path (open curve or closed shape).
pub fn generate_text_on_path(
    options: &TextOptions,
    path: &Polyline,
    on_path: &TextOnPathOptions,
) -> Result<Vec<Polyline>, String> {
    if path.points.len() < 2 {
        return Err("Path must have at least 2 points".to_string());
    }

    let mut path_clone = path.clone();
    if on_path.reverse_direction {
        path_clone.reverse();
    }

    let path_len = path_clone.length();
    if path_len <= 1e-4 {
        return Err("Path length is too short".to_string());
    }

    match &options.font_type {
        FontType::SingleLine => generate_singleline_on_path(options, &path_clone, on_path, path_len),
        FontType::TrueType { path: font_path } => {
            generate_ttf_on_path(options, font_path, &path_clone, on_path, path_len)
        }
    }
}

fn generate_singleline_on_path(
    options: &TextOptions,
    path: &Polyline,
    on_path: &TextOnPathOptions,
    path_len: f64,
) -> Result<Vec<Polyline>, String> {
    let scale = options.font_size_mm / 22.0;
    let mut total_w = 0.0;
    for ch in options.text.chars() {
        total_w += get_hershey_char_width(ch) * scale * options.letter_spacing;
    }

    let start_dist = match on_path.align {
        TextAlign::Left => on_path.start_offset_mm,
        TextAlign::Center => on_path.start_offset_mm + (path_len - total_w) * 0.5,
        TextAlign::Right => on_path.start_offset_mm + (path_len - total_w),
    };

    let mut all_polylines = Vec::new();
    let mut cur_x = 0.0;

    for ch in options.text.chars() {
        let (char_w_raw, strokes) = get_hershey_char_strokes(ch);
        let char_w = char_w_raw * scale * options.letter_spacing;

        let char_center_dist = start_dist + cur_x + char_w * 0.5;
        if let Some((pt, tangent)) = path.point_and_tangent_at_distance(char_center_dist) {
            let cos_theta = tangent.x;
            let sin_theta = tangent.y;

            for stroke in strokes {
                let pts: Vec<Point2D> = stroke
                    .into_iter()
                    .map(|(gx, gy)| {
                        let lx = (gx * scale) - (char_w_raw * scale * 0.5);
                        let ly = -(gy * scale) - on_path.perpendicular_offset_mm;
                        let rx = lx * cos_theta - ly * sin_theta;
                        let ry = lx * sin_theta + ly * cos_theta;
                        Point2D::new(pt.x + rx, pt.y + ry)
                    })
                    .collect();

                if pts.len() >= 2 {
                    all_polylines.push(Polyline::new(pts, false));
                }
            }
        }

        cur_x += char_w;
    }

    Ok(all_polylines)
}

fn generate_ttf_on_path(
    options: &TextOptions,
    font_path: &str,
    path: &Polyline,
    on_path: &TextOnPathOptions,
    path_len: f64,
) -> Result<Vec<Polyline>, String> {
    let font_bytes = load_font_bytes(font_path)?;

    let face = ttf_parser::Face::parse(&font_bytes, 0)
        .map_err(|e| format!("Failed to parse font file: {:?}", e))?;

    let units_per_em = face.units_per_em() as f64;
    let ascender = face.ascender() as f64;
    let descender = face.descender() as f64;
    let line_height_units = (ascender - descender).max(units_per_em);
    let scale = options.font_size_mm / line_height_units;

    let mut total_w = 0.0;
    for ch in options.text.chars() {
        if let Some(glyph_id) = face.glyph_index(ch) {
            let adv = face.glyph_hor_advance(glyph_id).unwrap_or(0) as f64;
            total_w += adv * scale * options.letter_spacing;
        } else {
            total_w += (units_per_em * 0.5) * scale * options.letter_spacing;
        }
    }

    let start_dist = match on_path.align {
        TextAlign::Left => on_path.start_offset_mm,
        TextAlign::Center => on_path.start_offset_mm + (path_len - total_w) * 0.5,
        TextAlign::Right => on_path.start_offset_mm + (path_len - total_w),
    };

    let mut all_polylines = Vec::new();
    let mut cur_x = 0.0;

    for ch in options.text.chars() {
        if let Some(glyph_id) = face.glyph_index(ch) {
            let adv_units = face.glyph_hor_advance(glyph_id).unwrap_or(0) as f64;
            let char_w = adv_units * scale * options.letter_spacing;
            let char_center_dist = start_dist + cur_x + char_w * 0.5;

            if let Some((pt, tangent)) = path.point_and_tangent_at_distance(char_center_dist) {
                let cos_theta = tangent.x;
                let sin_theta = tangent.y;

                let mut collector = OutlineCollector::new(scale, 0.0, 0.0);
                face.outline_glyph(glyph_id, &mut collector);
                collector.finish_subpath(true);

                for poly in collector.polylines {
                    let transformed_pts: Vec<Point2D> = poly
                        .points
                        .into_iter()
                        .map(|p| {
                            let lx = p.x - (char_w * 0.5);
                            let ly = p.y - on_path.perpendicular_offset_mm;
                            let rx = lx * cos_theta - ly * sin_theta;
                            let ry = lx * sin_theta + ly * cos_theta;
                            Point2D::new(pt.x + rx, pt.y + ry)
                        })
                        .collect();

                    if transformed_pts.len() >= 2 {
                        all_polylines.push(Polyline::new(transformed_pts, poly.is_closed));
                    }
                }
            }

            cur_x += char_w;
        } else {
            cur_x += (units_per_em * 0.5) * scale * options.letter_spacing;
        }
    }

    Ok(all_polylines)
}

fn generate_ttf_polylines(options: &TextOptions, font_path: &str) -> Result<Vec<Polyline>, String> {
    let font_bytes = load_font_bytes(font_path)?;

    let face = ttf_parser::Face::parse(&font_bytes, 0)
        .map_err(|e| format!("Failed to parse font file: {:?}", e))?;

    let units_per_em = face.units_per_em() as f64;
    let ascender = face.ascender() as f64;
    let descender = face.descender() as f64;
    let line_height_units = (ascender - descender).max(units_per_em);

    let scale = options.font_size_mm / line_height_units;
    let line_height_mm = options.font_size_mm * options.line_spacing;

    let mut all_polylines = Vec::new();
    let lines: Vec<&str> = options.text.lines().collect();

    for (line_idx, line) in lines.iter().enumerate() {
        let line_y = options.y + (line_idx as f64) * line_height_mm + (ascender * scale);

        // Measure line width for alignment
        let mut line_width_units = 0.0;
        for ch in line.chars() {
            if let Some(glyph_id) = face.glyph_index(ch) {
                let adv = face.glyph_hor_advance(glyph_id).unwrap_or(0) as f64;
                line_width_units += adv * options.letter_spacing;
            } else {
                line_width_units += (units_per_em * 0.5) * options.letter_spacing;
            }
        }
        let line_width_mm = line_width_units * scale;

        let start_x = match options.align {
            TextAlign::Left => options.x,
            TextAlign::Center => options.x - line_width_mm * 0.5,
            TextAlign::Right => options.x - line_width_mm,
        };

        let mut cur_x_mm = start_x;
        for ch in line.chars() {
            if let Some(glyph_id) = face.glyph_index(ch) {
                let mut collector = OutlineCollector::new(scale, cur_x_mm, line_y);
                face.outline_glyph(glyph_id, &mut collector);
                collector.finish_subpath(true);
                all_polylines.extend(collector.polylines);

                let adv = face.glyph_hor_advance(glyph_id).unwrap_or(0) as f64;
                cur_x_mm += adv * scale * options.letter_spacing;
            } else {
                cur_x_mm += (units_per_em * 0.5) * scale * options.letter_spacing;
            }
        }
    }

    Ok(all_polylines)
}

/// Generates single-line stroke font polylines using Hershey Simplex vector data
fn generate_singleline_polylines(options: &TextOptions) -> Result<Vec<Polyline>, String> {
    let mut all_polylines = Vec::new();
    let scale = options.font_size_mm / 22.0; // Hershey glyph coordinate grid is roughly 22 units high
    let line_height_mm = options.font_size_mm * options.line_spacing;
    let lines: Vec<&str> = options.text.lines().collect();

    for (line_idx, line) in lines.iter().enumerate() {
        let line_y = options.y + (line_idx as f64) * line_height_mm + (16.0 * scale);

        // Calculate line width
        let mut total_w = 0.0;
        for ch in line.chars() {
            total_w += get_hershey_char_width(ch) * scale * options.letter_spacing;
        }

        let start_x = match options.align {
            TextAlign::Left => options.x,
            TextAlign::Center => options.x - total_w * 0.5,
            TextAlign::Right => options.x - total_w,
        };

        let mut cur_x = start_x;
        for ch in line.chars() {
            let (char_w, strokes) = get_hershey_char_strokes(ch);
            for stroke in strokes {
                let pts: Vec<Point2D> = stroke
                    .into_iter()
                    .map(|(gx, gy)| Point2D::new(cur_x + gx * scale, line_y - gy * scale))
                    .collect();
                if pts.len() >= 2 {
                    all_polylines.push(Polyline::new(pts, false));
                }
            }
            cur_x += char_w * scale * options.letter_spacing;
        }
    }

    Ok(all_polylines)
}

fn get_hershey_char_width(ch: char) -> f64 {
    get_hershey_char_strokes(ch).0
}

/// Vector stroke representation for standard characters (Hershey-style single-line glyphs)
fn get_hershey_char_strokes(ch: char) -> (f64, Vec<Vec<(f64, f64)>>) {
    match ch {
        ' ' => (8.0, vec![]),
        '!' => (6.0, vec![vec![(3.0, 16.0), (3.0, 5.0)], vec![(3.0, 2.0), (3.0, 0.0)]]),
        '"' => (8.0, vec![vec![(2.0, 16.0), (2.0, 10.0)], vec![(6.0, 16.0), (6.0, 10.0)]]),
        '#' => (12.0, vec![
            vec![(4.0, 16.0), (4.0, 0.0)],
            vec![(8.0, 16.0), (8.0, 0.0)],
            vec![(1.0, 11.0), (11.0, 11.0)],
            vec![(1.0, 5.0), (11.0, 5.0)],
        ]),
        '$' => (12.0, vec![
            vec![(6.0, 18.0), (6.0, -2.0)],
            vec![(10.0, 13.0), (8.0, 15.0), (4.0, 15.0), (2.0, 13.0), (2.0, 10.0), (4.0, 8.0), (8.0, 8.0), (10.0, 6.0), (10.0, 3.0), (8.0, 1.0), (4.0, 1.0), (2.0, 3.0)],
        ]),
        '%' => (14.0, vec![
            vec![(12.0, 16.0), (2.0, 0.0)],
            vec![(3.0, 14.0), (5.0, 14.0), (5.0, 11.0), (3.0, 11.0), (3.0, 14.0)],
            vec![(9.0, 5.0), (11.0, 5.0), (11.0, 2.0), (9.0, 2.0), (9.0, 5.0)],
        ]),
        '&' => (14.0, vec![vec![
            (12.0, 0.0), (4.0, 9.0), (4.0, 13.0), (6.0, 15.0), (8.0, 15.0), (9.0, 13.0),
            (8.0, 10.0), (2.0, 3.0), (2.0, 2.0), (4.0, 0.0), (8.0, 0.0), (12.0, 6.0),
        ]]),
        '\'' => (4.0, vec![vec![(2.0, 16.0), (2.0, 11.0)]]),
        '(' => (6.0, vec![vec![(5.0, 18.0), (2.0, 13.0), (1.0, 8.0), (2.0, 3.0), (5.0, -2.0)]]),
        ')' => (6.0, vec![vec![(1.0, 18.0), (4.0, 13.0), (5.0, 8.0), (4.0, 3.0), (1.0, -2.0)]]),
        '*' => (10.0, vec![
            vec![(5.0, 13.0), (5.0, 3.0)],
            vec![(1.0, 11.0), (9.0, 5.0)],
            vec![(1.0, 5.0), (9.0, 11.0)],
        ]),
        '+' => (12.0, vec![vec![(6.0, 13.0), (6.0, 3.0)], vec![(1.0, 8.0), (11.0, 8.0)]]),
        ',' => (5.0, vec![vec![(3.0, 2.0), (3.0, 0.0), (1.0, -3.0)]]),
        '-' => (10.0, vec![vec![(1.0, 8.0), (9.0, 8.0)]]),
        '.' => (5.0, vec![vec![(2.5, 2.0), (2.5, 0.0)]]),
        '/' => (10.0, vec![vec![(1.0, 0.0), (9.0, 16.0)]]),
        // Numbers
        '0' => (12.0, vec![
            vec![(6.0, 16.0), (2.0, 13.0), (2.0, 3.0), (6.0, 0.0), (10.0, 3.0), (10.0, 13.0), (6.0, 16.0)],
            vec![(2.0, 3.0), (10.0, 13.0)],
        ]),
        '1' => (10.0, vec![vec![(2.0, 12.0), (5.0, 16.0), (5.0, 0.0)], vec![(2.0, 0.0), (8.0, 0.0)]]),
        '2' => (12.0, vec![vec![(2.0, 13.0), (4.0, 16.0), (8.0, 16.0), (10.0, 13.0), (10.0, 10.0), (2.0, 0.0), (10.0, 0.0)]]),
        '3' => (12.0, vec![vec![(2.0, 15.0), (10.0, 15.0), (6.0, 9.0), (9.0, 9.0), (10.0, 6.0), (10.0, 3.0), (7.0, 0.0), (2.0, 0.0)]]),
        '4' => (12.0, vec![vec![(9.0, 0.0), (9.0, 16.0), (1.0, 5.0), (11.0, 5.0)]]),
        '5' => (12.0, vec![vec![(10.0, 16.0), (2.0, 16.0), (2.0, 9.0), (7.0, 9.0), (10.0, 7.0), (10.0, 3.0), (8.0, 0.0), (2.0, 0.0)]]),
        '6' => (12.0, vec![vec![(9.0, 14.0), (6.0, 16.0), (2.0, 11.0), (2.0, 4.0), (6.0, 0.0), (9.0, 2.0), (10.0, 5.0), (8.0, 8.0), (2.0, 8.0)]]),
        '7' => (12.0, vec![vec![(2.0, 16.0), (10.0, 16.0), (4.0, 0.0)]]),
        '8' => (12.0, vec![
            vec![(6.0, 16.0), (2.0, 13.0), (3.0, 9.0), (6.0, 8.0), (9.0, 9.0), (10.0, 13.0), (6.0, 16.0)],
            vec![(6.0, 8.0), (2.0, 6.0), (2.0, 2.0), (6.0, 0.0), (10.0, 2.0), (10.0, 6.0), (6.0, 8.0)],
        ]),
        '9' => (12.0, vec![vec![(10.0, 8.0), (4.0, 8.0), (2.0, 11.0), (3.0, 14.0), (6.0, 16.0), (10.0, 12.0), (10.0, 5.0), (6.0, 0.0), (3.0, 2.0)]]),
        ':' => (5.0, vec![vec![(2.5, 11.0), (2.5, 9.0)], vec![(2.5, 3.0), (2.5, 1.0)]]),
        ';' => (5.0, vec![vec![(2.5, 11.0), (2.5, 9.0)], vec![(2.5, 3.0), (2.5, 1.0), (1.0, -2.0)]]),
        '<' => (10.0, vec![vec![(9.0, 14.0), (1.0, 8.0), (9.0, 2.0)]]),
        '=' => (10.0, vec![vec![(1.0, 10.0), (9.0, 10.0)], vec![(1.0, 6.0), (9.0, 6.0)]]),
        '>' => (10.0, vec![vec![(1.0, 14.0), (9.0, 8.0), (1.0, 2.0)]]),
        '?' => (10.0, vec![
            vec![(2.0, 13.0), (4.0, 16.0), (7.0, 16.0), (9.0, 14.0), (8.0, 11.0), (5.0, 8.0), (5.0, 5.0)],
            vec![(5.0, 2.0), (5.0, 0.0)],
        ]),
        '@' => (16.0, vec![
            vec![(14.0, 5.0), (11.0, 2.0), (7.0, 2.0), (4.0, 5.0), (3.0, 9.0), (4.0, 14.0), (8.0, 17.0), (13.0, 14.0), (13.0, 7.0), (10.0, 6.0), (8.0, 8.0), (8.0, 11.0), (10.0, 11.0), (10.0, 6.0)],
        ]),
        // Uppercase Letters
        'A' => (13.0, vec![
            vec![(1.0, 0.0), (6.5, 16.0), (12.0, 0.0)],
            vec![(3.5, 6.0), (9.5, 6.0)],
        ]),
        'B' => (13.0, vec![
            vec![(2.0, 0.0), (2.0, 16.0), (8.0, 16.0), (11.0, 14.0), (11.0, 10.0), (8.0, 8.0), (2.0, 8.0)],
            vec![(8.0, 8.0), (11.0, 6.0), (11.0, 2.0), (8.0, 0.0), (2.0, 0.0)],
        ]),
        'C' => (13.0, vec![vec![(11.0, 13.0), (8.0, 16.0), (4.0, 16.0), (2.0, 12.0), (2.0, 4.0), (4.0, 0.0), (8.0, 0.0), (11.0, 3.0)]]),
        'D' => (13.0, vec![vec![(2.0, 0.0), (2.0, 16.0), (7.0, 16.0), (11.0, 12.0), (11.0, 4.0), (7.0, 0.0), (2.0, 0.0)]]),
        'E' => (12.0, vec![
            vec![(10.0, 16.0), (2.0, 16.0), (2.0, 0.0), (10.0, 0.0)],
            vec![(2.0, 8.0), (8.0, 8.0)],
        ]),
        'F' => (12.0, vec![
            vec![(10.0, 16.0), (2.0, 16.0), (2.0, 0.0)],
            vec![(2.0, 8.0), (8.0, 8.0)],
        ]),
        'G' => (14.0, vec![
            vec![(12.0, 13.0), (9.0, 16.0), (4.0, 16.0), (2.0, 12.0), (2.0, 4.0), (4.0, 0.0), (9.0, 0.0), (12.0, 3.0), (12.0, 8.0), (7.0, 8.0)],
        ]),
        'H' => (13.0, vec![
            vec![(2.0, 16.0), (2.0, 0.0)],
            vec![(11.0, 16.0), (11.0, 0.0)],
            vec![(2.0, 8.0), (11.0, 8.0)],
        ]),
        'I' => (6.0, vec![
            vec![(1.0, 16.0), (5.0, 16.0)],
            vec![(3.0, 16.0), (3.0, 0.0)],
            vec![(1.0, 0.0), (5.0, 0.0)],
        ]),
        'J' => (10.0, vec![
            vec![(8.0, 16.0), (8.0, 4.0), (6.0, 0.0), (3.0, 0.0), (1.0, 3.0), (1.0, 6.0)],
            vec![(5.0, 16.0), (10.0, 16.0)],
        ]),
        'K' => (13.0, vec![
            vec![(2.0, 16.0), (2.0, 0.0)],
            vec![(11.0, 16.0), (2.0, 7.0)],
            vec![(5.0, 9.5), (11.0, 0.0)],
        ]),
        'L' => (11.0, vec![vec![(2.0, 16.0), (2.0, 0.0), (9.0, 0.0)]]),
        'M' => (15.0, vec![vec![(2.0, 0.0), (2.0, 16.0), (7.5, 6.0), (13.0, 16.0), (13.0, 0.0)]]),
        'N' => (13.0, vec![vec![(2.0, 0.0), (2.0, 16.0), (11.0, 0.0), (11.0, 16.0)]]),
        'O' => (14.0, vec![vec![(7.0, 16.0), (3.0, 13.0), (2.0, 8.0), (3.0, 3.0), (7.0, 0.0), (11.0, 3.0), (12.0, 8.0), (11.0, 13.0), (7.0, 16.0)]]),
        'P' => (13.0, vec![
            vec![(2.0, 0.0), (2.0, 16.0), (8.0, 16.0), (11.0, 13.5), (11.0, 10.5), (8.0, 8.0), (2.0, 8.0)],
        ]),
        'Q' => (14.0, vec![
            vec![(7.0, 16.0), (3.0, 13.0), (2.0, 8.0), (3.0, 3.0), (7.0, 0.0), (11.0, 3.0), (12.0, 8.0), (11.0, 13.0), (7.0, 16.0)],
            vec![(8.0, 4.0), (12.0, -2.0)],
        ]),
        'R' => (13.0, vec![
            vec![(2.0, 0.0), (2.0, 16.0), (8.0, 16.0), (11.0, 13.5), (11.0, 10.5), (8.0, 8.0), (2.0, 8.0)],
            vec![(6.0, 8.0), (11.0, 0.0)],
        ]),
        'S' => (12.0, vec![vec![
            (10.0, 14.0), (8.0, 16.0), (4.0, 16.0), (2.0, 14.0), (2.0, 11.0),
            (4.0, 9.0), (8.0, 8.0), (10.0, 6.0), (10.0, 2.0), (8.0, 0.0), (3.0, 0.0), (1.0, 2.0),
        ]]),
        'T' => (12.0, vec![
            vec![(1.0, 16.0), (11.0, 16.0)],
            vec![(6.0, 16.0), (6.0, 0.0)],
        ]),
        'U' => (13.0, vec![vec![(2.0, 16.0), (2.0, 4.0), (5.0, 0.0), (8.0, 0.0), (11.0, 4.0), (11.0, 16.0)]]),
        'V' => (13.0, vec![vec![(1.0, 16.0), (6.5, 0.0), (12.0, 16.0)]]),
        'W' => (17.0, vec![vec![(1.0, 16.0), (5.0, 0.0), (8.5, 10.0), (12.0, 0.0), (16.0, 16.0)]]),
        'X' => (13.0, vec![vec![(2.0, 16.0), (11.0, 0.0)], vec![(11.0, 16.0), (2.0, 0.0)]]),
        'Y' => (13.0, vec![
            vec![(1.0, 16.0), (6.5, 8.0), (12.0, 16.0)],
            vec![(6.5, 8.0), (6.5, 0.0)],
        ]),
        'Z' => (12.0, vec![vec![(2.0, 16.0), (10.0, 16.0), (2.0, 0.0), (10.0, 0.0)]]),
        'Ä' => {
            let (w, mut s) = get_hershey_char_strokes('A');
            s.push(vec![(4.0, 19.0), (5.0, 19.0)]);
            s.push(vec![(8.0, 19.0), (9.0, 19.0)]);
            (w, s)
        }
        'Ö' => {
            let (w, mut s) = get_hershey_char_strokes('O');
            s.push(vec![(4.5, 19.0), (5.5, 19.0)]);
            s.push(vec![(8.5, 19.0), (9.5, 19.0)]);
            (w, s)
        }
        'Å' => {
            let (w, mut s) = get_hershey_char_strokes('A');
            s.push(vec![(6.5, 20.0), (5.5, 19.0), (6.5, 18.0), (7.5, 19.0), (6.5, 20.0)]);
            (w, s)
        }
        // Lowercase Letters
        'a' => (11.0, vec![
            vec![(9.0, 10.0), (9.0, 0.0)],
            vec![(9.0, 8.0), (6.0, 10.0), (3.0, 10.0), (1.0, 7.0), (1.0, 4.0), (3.0, 0.0), (7.0, 0.0), (9.0, 2.0)],
        ]),
        'b' => (11.0, vec![
            vec![(2.0, 16.0), (2.0, 0.0)],
            vec![(2.0, 8.0), (5.0, 10.0), (8.0, 10.0), (10.0, 7.0), (10.0, 3.0), (8.0, 0.0), (2.0, 0.0)],
        ]),
        'c' => (10.0, vec![vec![(9.0, 8.0), (7.0, 10.0), (3.0, 10.0), (1.0, 7.0), (1.0, 3.0), (3.0, 0.0), (7.0, 0.0), (9.0, 2.0)]]),
        'd' => (11.0, vec![
            vec![(9.0, 16.0), (9.0, 0.0)],
            vec![(9.0, 8.0), (6.0, 10.0), (3.0, 10.0), (1.0, 7.0), (1.0, 3.0), (3.0, 0.0), (6.0, 0.0), (9.0, 2.0)],
        ]),
        'e' => (10.0, vec![vec![
            (1.0, 5.0), (9.0, 5.0), (9.0, 7.0), (7.0, 10.0), (3.0, 10.0), (1.0, 7.0), (1.0, 3.0), (3.0, 0.0), (7.0, 0.0), (9.0, 2.0),
        ]]),
        'f' => (7.0, vec![
            vec![(6.0, 16.0), (4.0, 16.0), (2.0, 14.0), (2.0, 0.0)],
            vec![(0.0, 10.0), (5.0, 10.0)],
        ]),
        'g' => (11.0, vec![
            vec![(9.0, 10.0), (9.0, -3.0), (7.0, -6.0), (3.0, -6.0), (1.0, -4.0)],
            vec![(9.0, 8.0), (6.0, 10.0), (3.0, 10.0), (1.0, 7.0), (1.0, 3.0), (3.0, 0.0), (6.0, 0.0), (9.0, 2.0)],
        ]),
        'h' => (11.0, vec![
            vec![(2.0, 16.0), (2.0, 0.0)],
            vec![(2.0, 7.0), (5.0, 10.0), (8.0, 10.0), (9.0, 8.0), (9.0, 0.0)],
        ]),
        'i' => (4.0, vec![
            vec![(2.0, 10.0), (2.0, 0.0)],
            vec![(2.0, 14.0), (2.0, 13.0)],
        ]),
        'j' => (6.0, vec![
            vec![(4.0, 10.0), (4.0, -4.0), (2.0, -6.0), (0.0, -5.0)],
            vec![(4.0, 14.0), (4.0, 13.0)],
        ]),
        'k' => (10.0, vec![
            vec![(2.0, 16.0), (2.0, 0.0)],
            vec![(8.0, 10.0), (2.0, 4.0)],
            vec![(4.5, 6.0), (9.0, 0.0)],
        ]),
        'l' => (4.0, vec![vec![(2.0, 16.0), (2.0, 1.0), (3.0, 0.0)]]),
        'm' => (16.0, vec![
            vec![(2.0, 10.0), (2.0, 0.0)],
            vec![(2.0, 7.0), (5.0, 10.0), (7.0, 10.0), (8.0, 7.0), (8.0, 0.0)],
            vec![(8.0, 7.0), (11.0, 10.0), (13.0, 10.0), (14.0, 7.0), (14.0, 0.0)],
        ]),
        'n' => (11.0, vec![
            vec![(2.0, 10.0), (2.0, 0.0)],
            vec![(2.0, 7.0), (5.0, 10.0), (8.0, 10.0), (9.0, 8.0), (9.0, 0.0)],
        ]),
        'o' => (11.0, vec![vec![(5.5, 10.0), (2.0, 8.0), (1.0, 5.0), (2.0, 2.0), (5.5, 0.0), (8.5, 2.0), (9.5, 5.0), (8.5, 8.0), (5.5, 10.0)]]),
        'p' => (11.0, vec![
            vec![(2.0, 10.0), (2.0, -5.0)],
            vec![(2.0, 8.0), (5.0, 10.0), (8.0, 10.0), (10.0, 7.0), (10.0, 3.0), (8.0, 0.0), (2.0, 0.0)],
        ]),
        'q' => (11.0, vec![
            vec![(9.0, 10.0), (9.0, -5.0)],
            vec![(9.0, 8.0), (6.0, 10.0), (3.0, 10.0), (1.0, 7.0), (1.0, 3.0), (3.0, 0.0), (6.0, 0.0), (9.0, 2.0)],
        ]),
        'r' => (8.0, vec![
            vec![(2.0, 10.0), (2.0, 0.0)],
            vec![(2.0, 7.0), (4.0, 10.0), (7.0, 10.0)],
        ]),
        's' => (9.0, vec![vec![
            (8.0, 9.0), (6.0, 10.0), (3.0, 10.0), (1.0, 8.0), (3.0, 6.0), (6.0, 5.0), (8.0, 3.0), (6.0, 0.0), (2.0, 0.0), (1.0, 1.0),
        ]]),
        't' => (7.0, vec![
            vec![(3.0, 14.0), (3.0, 2.0), (5.0, 0.0)],
            vec![(1.0, 10.0), (6.0, 10.0)],
        ]),
        'u' => (11.0, vec![
            vec![(2.0, 10.0), (2.0, 3.0), (5.0, 0.0), (8.0, 0.0), (9.0, 3.0)],
            vec![(9.0, 10.0), (9.0, 0.0)],
        ]),
        'v' => (10.0, vec![vec![(1.0, 10.0), (5.0, 0.0), (9.0, 10.0)]]),
        'w' => (14.0, vec![vec![(1.0, 10.0), (4.0, 0.0), (7.0, 7.0), (10.0, 0.0), (13.0, 10.0)]]),
        'x' => (10.0, vec![vec![(1.0, 10.0), (9.0, 0.0)], vec![(9.0, 10.0), (1.0, 0.0)]]),
        'y' => (10.0, vec![
            vec![(1.0, 10.0), (5.0, 0.0)],
            vec![(9.0, 10.0), (3.0, -5.0), (1.0, -4.0)],
        ]),
        'z' => (9.0, vec![vec![(1.0, 10.0), (8.0, 10.0), (1.0, 0.0), (8.0, 0.0)]]),
        'ä' => {
            let (w, mut s) = get_hershey_char_strokes('a');
            s.push(vec![(3.0, 13.0), (4.0, 13.0)]);
            s.push(vec![(7.0, 13.0), (8.0, 13.0)]);
            (w, s)
        }
        'ö' => {
            let (w, mut s) = get_hershey_char_strokes('o');
            s.push(vec![(3.5, 13.0), (4.5, 13.0)]);
            s.push(vec![(6.5, 13.0), (7.5, 13.0)]);
            (w, s)
        }
        'å' => {
            let (w, mut s) = get_hershey_char_strokes('a');
            s.push(vec![(5.0, 14.5), (4.0, 13.5), (5.0, 12.5), (6.0, 13.5), (5.0, 14.5)]);
            (w, s)
        }
        '|' => (6.0, vec![vec![(3.0, -2.0), (3.0, 17.0)]]),
        '_' => (10.0, vec![vec![(1.0, 0.0), (9.0, 0.0)]]),
        '[' => (7.0, vec![vec![(5.0, 17.0), (2.0, 17.0), (2.0, -2.0), (5.0, -2.0)]]),
        ']' => (7.0, vec![vec![(2.0, 17.0), (5.0, 17.0), (5.0, -2.0), (2.0, -2.0)]]),
        _ => (10.0, vec![vec![(1.0, 0.0), (1.0, 14.0), (9.0, 14.0), (9.0, 0.0), (1.0, 0.0)]]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hershey_single_line() {
        let opt = TextOptions {
            text: "LASER 123".to_string(),
            font_type: FontType::SingleLine,
            font_size_mm: 10.0,
            x: 10.0,
            y: 20.0,
            align: TextAlign::Left,
            letter_spacing: 1.0,
            line_spacing: 1.2,
        };

        let polylines = generate_text_polylines(&opt).expect("Single line generation should succeed");
        assert!(!polylines.is_empty(), "Should generate polylines for text");
    }

    #[test]
    fn test_ttf_vectorization() {
        let default_font = default_system_font();
        if !default_font.is_empty() {
            let opt = TextOptions {
                text: "LightBurn".to_string(),
                font_type: FontType::TrueType { path: default_font },
                font_size_mm: 15.0,
                x: 0.0,
                y: 0.0,
                align: TextAlign::Center,
                letter_spacing: 1.0,
                line_spacing: 1.2,
            };

            let polylines = generate_text_polylines(&opt).expect("TTF vectorization should succeed");
            assert!(!polylines.is_empty(), "Should generate closed polylines for glyphs");
            for p in &polylines {
                assert!(p.is_closed, "TrueType contours must be closed");
            }
        }
    }

    #[test]
    fn test_text_on_path_singleline() {
        let opt = TextOptions {
            text: "CIRCULAR".to_string(),
            font_type: FontType::SingleLine,
            font_size_mm: 10.0,
            x: 0.0,
            y: 0.0,
            align: TextAlign::Center,
            letter_spacing: 1.0,
            line_spacing: 1.2,
        };

        // Create a semicircular arc path of radius 50mm
        let mut arc_pts = Vec::new();
        let steps = 30;
        for i in 0..=steps {
            let a = (i as f64 / steps as f64) * std::f64::consts::PI;
            arc_pts.push(Point2D::new(50.0 + 50.0 * a.cos(), 50.0 + 50.0 * a.sin()));
        }
        let arc_path = Polyline::new(arc_pts, false);

        let on_path = TextOnPathOptions {
            start_offset_mm: 0.0,
            perpendicular_offset_mm: 2.0,
            align: TextAlign::Center,
            reverse_direction: false,
        };

        let polylines = generate_text_on_path(&opt, &arc_path, &on_path).expect("Text on path must succeed");
        assert!(!polylines.is_empty(), "Must generate polylines along arc");
    }

    #[test]
    fn test_embedded_google_handwriting_fonts() {
        let recipe_text = "Mummon mustikkapiirakka:\n2 dl sokeria, 3 munaa, 100g voita.\nÄidin herkkuresepti & öljyä!";
        let embedded_fonts = [
            ("Caveat", "embedded:Caveat"),
            ("Kalam", "embedded:Kalam"),
            ("Satisfy", "embedded:Satisfy"),
            ("PatrickHand", "embedded:PatrickHand"),
            ("AlexBrush", "embedded:AlexBrush"),
        ];

        for (name, font_id) in embedded_fonts {
            let opt = TextOptions {
                text: recipe_text.to_string(),
                font_type: FontType::TrueType { path: font_id.to_string() },
                font_size_mm: 14.0,
                x: 10.0,
                y: 10.0,
                align: TextAlign::Left,
                letter_spacing: 1.15,
                line_spacing: 1.3,
            };

            let polylines = generate_text_polylines(&opt)
                .unwrap_or_else(|e| panic!("Font '{}' failed vectorization: {}", name, e));
            assert!(!polylines.is_empty(), "Font '{}' produced 0 polylines", name);
            for p in &polylines {
                assert!(p.is_closed, "Font '{}' contours must be closed loops for laser cutting/engraving", name);
            }
        }
    }
}

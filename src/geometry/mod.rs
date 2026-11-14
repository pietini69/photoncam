use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

impl Point2D {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance_to(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn distance_squared_to(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }

    pub fn lerp(&self, other: &Self, t: f64) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vector2D {
    pub dx: f64,
    pub dy: f64,
}

impl Vector2D {
    pub fn new(dx: f64, dy: f64) -> Self {
        Self { dx, dy }
    }

    pub fn length(&self) -> f64 {
        (self.dx * self.dx + self.dy * self.dy).sqrt()
    }

    pub fn normalized(&self) -> Self {
        let len = self.length();
        if len > 1e-12 {
            Self {
                dx: self.dx / len,
                dy: self.dy / len,
            }
        } else {
            Self { dx: 0.0, dy: 0.0 }
        }
    }

    pub fn perpendicular(&self) -> Self {
        Self {
            dx: -self.dy,
            dy: self.dx,
        }
    }

    pub fn dot(&self, other: &Self) -> f64 {
        self.dx * other.dx + self.dy * other.dy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl BoundingBox {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }

    pub fn from_points(points: &[Point2D]) -> Self {
        let mut bb = Self::empty();
        for p in points {
            bb.expand_to_include(p);
        }
        bb
    }

    pub fn expand_to_include(&mut self, p: &Point2D) {
        if p.x < self.min_x {
            self.min_x = p.x;
        }
        if p.x > self.max_x {
            self.max_x = p.x;
        }
        if p.y < self.min_y {
            self.min_y = p.y;
        }
        if p.y > self.max_y {
            self.max_y = p.y;
        }
    }

    pub fn union(&self, other: &Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    pub fn width(&self) -> f64 {
        if self.max_x >= self.min_x {
            self.max_x - self.min_x
        } else {
            0.0
        }
    }

    pub fn height(&self) -> f64 {
        if self.max_y >= self.min_y {
            self.max_y - self.min_y
        } else {
            0.0
        }
    }

    pub fn center(&self) -> Point2D {
        Point2D::new(
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        )
    }

    pub fn contains_point(&self, p: &Point2D) -> bool {
        p.x >= self.min_x && p.x <= self.max_x && p.y >= self.min_y && p.y <= self.max_y
    }

    /// Returns true if this bounding box intersects with the other bounding box
    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.min_x <= other.max_x && self.max_x >= other.min_x
            && self.min_y <= other.max_y && self.max_y >= other.min_y
    }

    /// Returns true if this bounding box completely contains the other bounding box
    pub fn contains_bounds(&self, other: &BoundingBox) -> bool {
        self.min_x <= other.min_x + 1e-6
            && self.max_x >= other.max_x - 1e-6
            && self.min_y <= other.min_y + 1e-6
            && self.max_y >= other.max_y - 1e-6
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polyline {
    pub points: Vec<Point2D>,
    pub is_closed: bool,
}

impl Polyline {
    pub fn new(points: Vec<Point2D>, is_closed: bool) -> Self {
        Self { points, is_closed }
    }

    pub fn bounds(&self) -> BoundingBox {
        BoundingBox::from_points(&self.points)
    }

    pub fn length(&self) -> f64 {
        if self.points.len() < 2 {
            return 0.0;
        }
        let mut total = 0.0;
        for i in 0..self.points.len() - 1 {
            total += self.points[i].distance_to(&self.points[i + 1]);
        }
        if self.is_closed && self.points.len() > 2 {
            total += self.points.last().unwrap().distance_to(&self.points[0]);
        }
        total
    }

    /// Returns the position and unit tangent vector at a specified distance along the polyline.
    pub fn point_and_tangent_at_distance(&self, mut distance: f64) -> Option<(Point2D, Point2D)> {
        if self.points.len() < 2 {
            return None;
        }

        let total_len = self.length();
        if total_len <= 1e-6 {
            return None;
        }

        if self.is_closed {
            distance = distance.rem_euclid(total_len);
        } else {
            distance = distance.clamp(0.0, total_len);
        }

        let n = self.points.len();
        let num_segs = if self.is_closed { n } else { n - 1 };

        let mut accumulated = 0.0;
        for i in 0..num_segs {
            let p1 = self.points[i];
            let p2 = if i + 1 < n { self.points[i + 1] } else { self.points[0] };
            let seg_len = p1.distance_to(&p2);

            if seg_len <= 1e-9 {
                continue;
            }

            if accumulated + seg_len >= distance || i == num_segs - 1 {
                let seg_dist = (distance - accumulated).clamp(0.0, seg_len);
                let t = seg_dist / seg_len;
                let pt = Point2D::new(p1.x + (p2.x - p1.x) * t, p1.y + (p2.y - p1.y) * t);
                let tangent = Point2D::new((p2.x - p1.x) / seg_len, (p2.y - p1.y) / seg_len);
                return Some((pt, tangent));
            }

            accumulated += seg_len;
        }

        let p1 = self.points[n - 2];
        let p2 = self.points[n - 1];
        let seg_len = p1.distance_to(&p2).max(1e-9);
        let tangent = Point2D::new((p2.x - p1.x) / seg_len, (p2.y - p1.y) / seg_len);
        Some((p2, tangent))
    }

    pub fn reverse(&mut self) {
        self.points.reverse();
    }

    /// For closed polylines, rotates the vertex order so that it begins at the vertex closest to `target`.
    /// This drastically minimizes rapid non-cutting transit time for laser jobs.
    pub fn rotate_start_to_closest_vertex(&mut self, target: Point2D) {
        if !self.is_closed || self.points.len() < 3 {
            return;
        }
        let mut best_idx = 0;
        let mut min_dist_sq = f64::INFINITY;
        for (i, p) in self.points.iter().enumerate() {
            let d = p.distance_squared_to(&target);
            if d < min_dist_sq {
                min_dist_sq = d;
                best_idx = i;
            }
        }
        if best_idx > 0 {
            self.points.rotate_left(best_idx);
        }
    }

    pub fn contains_point(&self, p: &Point2D) -> bool {
        if !self.is_closed || self.points.len() < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = self.points.len() - 1;
        for i in 0..self.points.len() {
            let pi = self.points[i];
            let pj = self.points[j];
            if ((pi.y > p.y) != (pj.y > p.y))
                && (p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x)
            {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    pub fn signed_area(&self) -> f64 {
        if self.points.len() < 3 {
            return 0.0;
        }
        let mut area = 0.0;
        let n = self.points.len();
        for i in 0..n {
            let j = (i + 1) % n;
            area += self.points[i].x * self.points[j].y;
            area -= self.points[j].x * self.points[i].y;
        }
        area * 0.5
    }

    pub fn is_clockwise(&self) -> bool {
        self.signed_area() < 0.0
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        for p in &mut self.points {
            p.x += dx;
            p.y += dy;
        }
    }

    pub fn scale(&mut self, origin: Point2D, sx: f64, sy: f64) {
        for p in &mut self.points {
            p.x = origin.x + (p.x - origin.x) * sx;
            p.y = origin.y + (p.y - origin.y) * sy;
        }
    }

    pub fn rotate(&mut self, origin: Point2D, angle_rad: f64) {
        let (sin_a, cos_a) = angle_rad.sin_cos();
        for p in &mut self.points {
            let dx = p.x - origin.x;
            let dy = p.y - origin.y;
            p.x = origin.x + dx * cos_a - dy * sin_a;
            p.y = origin.y + dx * sin_a + dy * cos_a;
        }
    }

    pub fn distance_to_point(&self, p: &Point2D) -> f64 {
        if self.points.is_empty() {
            return f64::INFINITY;
        }
        if self.points.len() == 1 {
            return p.distance_to(&self.points[0]);
        }
        let mut min_d = f64::INFINITY;
        let n = self.points.len();
        let count = if self.is_closed { n } else { n - 1 };
        for i in 0..count {
            let a = self.points[i];
            let b = self.points[(i + 1) % n];
            let d = point_to_segment_distance(p, &a, &b);
            if d < min_d {
                min_d = d;
            }
        }
        min_d
    }

    /// Offsets the polyline by a given distance.
    /// Positive distance expands outward for closed shapes (or shifts right for open paths).
    /// Negative distance shrinks inward for closed shapes.
    pub fn offset(&self, distance: f64) -> Polyline {
        if self.points.len() < 2 || distance.abs() < 1e-7 {
            return self.clone();
        }

        // Deduplicate consecutive identical or sub-micron points
        let mut pts: Vec<Point2D> = Vec::with_capacity(self.points.len());
        for &p in &self.points {
            if pts.last().map_or(true, |last: &Point2D| last.distance_to(&p) > 1e-3) {
                pts.push(p);
            }
        }
        if self.is_closed && pts.len() > 1 {
            if pts.first().unwrap().distance_to(pts.last().unwrap()) < 1e-3 {
                pts.pop();
            }
        }
        if pts.len() < 2 {
            return self.clone();
        }

        let n = pts.len();
        let is_ccw = if self.is_closed && n >= 3 {
            // Shoelace area
            let mut area = 0.0;
            for i in 0..n {
                let j = (i + 1) % n;
                area += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
            }
            area >= 0.0
        } else {
            true
        };

        let edge_count = if self.is_closed { n } else { n - 1 };
        let mut normals: Vec<Vector2D> = Vec::with_capacity(edge_count);

        for i in 0..edge_count {
            let p1 = pts[i];
            let p2 = pts[(i + 1) % n];
            let dx = p2.x - p1.x;
            let dy = p2.y - p1.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-9 {
                normals.push(Vector2D::new(0.0, 0.0));
            } else {
                let tx = dx / len;
                let ty = dy / len;
                if is_ccw {
                    normals.push(Vector2D::new(ty, -tx));
                } else {
                    normals.push(Vector2D::new(-ty, tx));
                }
            }
        }

        let miter_limit = 2.0 * distance.abs();
        let mut offset_pts: Vec<Point2D> = Vec::with_capacity(n);

        for i in 0..n {
            let (n1, n2) = if self.is_closed {
                let prev_idx = if i == 0 { n - 1 } else { i - 1 };
                (normals[prev_idx], normals[i])
            } else if i == 0 {
                (normals[0], normals[0])
            } else if i == n - 1 {
                (normals[n - 2], normals[n - 2])
            } else {
                (normals[i - 1], normals[i])
            };

            let dot = n1.dx * n2.dx + n1.dy * n2.dy;
            let denom = 1.0 + dot;
            let (vx, vy) = if denom < 0.15 {
                // Sharp hairpin corner: use average normal capped at distance
                let bx = n1.dx + n2.dx;
                let by = n1.dy + n2.dy;
                let blen = (bx * bx + by * by).sqrt();
                if blen > 1e-6 {
                    (distance * bx / blen, distance * by / blen)
                } else {
                    (distance * n2.dx, distance * n2.dy)
                }
            } else {
                let raw_vx = distance * (n1.dx + n2.dx) / denom;
                let raw_vy = distance * (n1.dy + n2.dy) / denom;
                let v_len = (raw_vx * raw_vx + raw_vy * raw_vy).sqrt();
                if v_len > miter_limit && miter_limit > 0.0 {
                    let scale = miter_limit / v_len;
                    (raw_vx * scale, raw_vy * scale)
                } else {
                    (raw_vx, raw_vy)
                }
            };

            offset_pts.push(Point2D::new(pts[i].x + vx, pts[i].y + vy));
        }

        Polyline::new(offset_pts, self.is_closed)
    }
}

pub fn point_to_segment_distance_sq(p: &Point2D, a: &Point2D, b: &Point2D) -> f64 {
    let l2 = a.distance_squared_to(b);
    if l2 < 1e-12 {
        return p.distance_squared_to(a);
    }
    let t = (((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / l2).clamp(0.0, 1.0);
    let proj_x = a.x + t * (b.x - a.x);
    let proj_y = a.y + t * (b.y - a.y);
    let dx = p.x - proj_x;
    let dy = p.y - proj_y;
    dx * dx + dy * dy
}

pub fn point_to_segment_distance(p: &Point2D, a: &Point2D, b: &Point2D) -> f64 {
    point_to_segment_distance_sq(p, a, b).sqrt()
}

/// Computes clean, smooth, merged, self-intersection-free offset contours around a set of polylines
/// (such as TrueType text, Hershey single-line text, welded shapes, or complex SVG paths)
/// using a high-resolution Signed Distance Field (SDF) and Marching Squares iso-contouring.
pub fn offset_polylines_sdf(polylines: &[Polyline], distance: f64) -> Vec<Polyline> {
    if polylines.is_empty() || distance.abs() < 1e-5 {
        return polylines.to_vec();
    }

    let mut bb = BoundingBox::empty();
    let mut has_closed = false;
    let mut segments: Vec<(Point2D, Point2D, bool)> = Vec::new();

    for poly in polylines {
        if poly.points.len() < 2 {
            continue;
        }
        bb = bb.union(&poly.bounds());
        if poly.is_closed && poly.points.len() >= 3 {
            has_closed = true;
        }
        let n = poly.points.len();
        let edge_count = if poly.is_closed { n } else { n - 1 };
        for i in 0..edge_count {
            let a = poly.points[i];
            let b = poly.points[(i + 1) % n];
            if a.distance_squared_to(&b) > 1e-10 {
                segments.push((a, b, poly.is_closed && n >= 3));
            }
        }
    }

    if segments.is_empty() || bb.width() <= 0.0 && bb.height() <= 0.0 {
        return Vec::new();
    }
    // Inward offset on purely open strokes disappears
    if !has_closed && distance < 0.0 {
        return Vec::new();
    }

    let abs_d = distance.abs();
    let pad = abs_d + 2.5;
    let min_x = bb.min_x - pad;
    let min_y = bb.min_y - pad;
    let max_x = bb.max_x + pad;
    let max_y = bb.max_y + pad;

    let span_x = (max_x - min_x).max(1.0);
    let span_y = (max_y - min_y).max(1.0);

    // Choose sub-millimeter grid step balanced for crisp corners and instant interactive response
    let mut cell = (span_x.max(span_y) / 300.0)
        .clamp(0.08, 0.45)
        .min((abs_d * 0.22).clamp(0.08, 0.45));
    let est_cells = (span_x / cell) * (span_y / cell);
    if est_cells > 200_000.0 {
        cell = ((span_x * span_y) / 200_000.0).sqrt();
    }

    let gw = ((span_x / cell).ceil() as usize + 2).max(4);
    let gh = ((span_y / cell).ceil() as usize + 2).max(4);
    let total_pts = gw * gh;

    // 1. Inside/outside classification via fast scanline even-odd rule on closed contours
    let mut inside = vec![false; total_pts];
    if has_closed {
        let mut x_ints = Vec::with_capacity(32);
        for iy in 0..gh {
            let y = min_y + (iy as f64) * cell + 1.3e-6;
            x_ints.clear();
            for &(a, b, is_closed_seg) in &segments {
                if !is_closed_seg {
                    continue;
                }
                if (a.y > y) != (b.y > y) {
                    let x = a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y);
                    x_ints.push(x);
                }
            }
            if x_ints.len() >= 2 {
                x_ints.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let row_off = iy * gw;
                for pair in x_ints.chunks_exact(2) {
                    let x0 = pair[0];
                    let x1 = pair[1];
                    let ix_start = (((x0 - min_x) / cell).ceil().max(0.0) as usize).min(gw);
                    let ix_end = (((x1 - min_x) / cell).floor() as isize).min((gw as isize) - 1);
                    if ix_end >= ix_start as isize {
                        for ix in ix_start..=(ix_end as usize) {
                            inside[row_off + ix] = true;
                        }
                    }
                }
            }
        }
    }

    // 2. Narrow-band Euclidean distance splatting around all segments
    let band_r = abs_d + 2.5 * cell;
    let max_d2 = (band_r + cell) * (band_r + cell);
    let mut min_dist_sq = vec![max_d2; total_pts];

    for &(a, b, _) in &segments {
        let seg_min_x = a.x.min(b.x) - band_r;
        let seg_max_x = a.x.max(b.x) + band_r;
        let seg_min_y = a.y.min(b.y) - band_r;
        let seg_max_y = a.y.max(b.y) + band_r;

        let ix0 = (((seg_min_x - min_x) / cell).floor().max(0.0) as usize).min(gw - 1);
        let ix1 = (((seg_max_x - min_x) / cell).ceil().max(0.0) as usize).min(gw - 1);
        let iy0 = (((seg_min_y - min_y) / cell).floor().max(0.0) as usize).min(gh - 1);
        let iy1 = (((seg_max_y - min_y) / cell).ceil().max(0.0) as usize).min(gh - 1);

        for iy in iy0..=iy1 {
            let gy = min_y + (iy as f64) * cell;
            let row_off = iy * gw;
            for ix in ix0..=ix1 {
                let gx = min_x + (ix as f64) * cell;
                let d2 = point_to_segment_distance_sq(&Point2D::new(gx, gy), &a, &b);
                if d2 < min_dist_sq[row_off + ix] {
                    min_dist_sq[row_off + ix] = d2;
                }
            }
        }
    }

    // 3. Build shifted signed distance field where field <= 0.0 is inside the offset region
    let mut field = vec![0.0f64; total_pts];
    for idx in 0..total_pts {
        let d = min_dist_sq[idx].sqrt();
        let sdf = if inside[idx] { -d } else { d };
        field[idx] = sdf - distance;
    }

    // 4. Marching Squares directed edge extraction (CCW around inside region field <= 0)
    // Unique Edge IDs:
    // Horizontal edge (ix, iy) -> between (ix, iy) and (ix+1, iy): id = (iy * gw + ix) * 2
    // Vertical edge   (ix, iy) -> between (ix, iy) and (ix, iy+1): id = (iy * gw + ix) * 2 + 1
    let mut edge_pos: std::collections::HashMap<usize, Point2D> = std::collections::HashMap::new();
    let mut next_edge: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();

    let interp = |v1: f64, v2: f64| -> f64 {
        let denom = v1 - v2;
        if denom.abs() < 1e-12 {
            0.5
        } else {
            (v1 / denom).clamp(0.0, 1.0)
        }
    };

    for iy in 0..(gh - 1) {
        let y0 = min_y + (iy as f64) * cell;
        let y1 = y0 + cell;
        for ix in 0..(gw - 1) {
            let x0 = min_x + (ix as f64) * cell;
            let x1 = x0 + cell;

            let v00 = field[iy * gw + ix];
            let v10 = field[iy * gw + ix + 1];
            let v11 = field[(iy + 1) * gw + ix + 1];
            let v01 = field[(iy + 1) * gw + ix];

            let mut mask = 0u8;
            if v00 <= 0.0 { mask |= 1; } // TL
            if v10 <= 0.0 { mask |= 2; } // TR
            if v11 <= 0.0 { mask |= 4; } // BR
            if v01 <= 0.0 { mask |= 8; } // BL

            if mask == 0 || mask == 15 {
                continue;
            }

            // Edge IDs for Top, Right, Bottom, Left of this cell
            let e_top = (iy * gw + ix) * 2;
            let e_right = (iy * gw + (ix + 1)) * 2 + 1;
            let e_bottom = ((iy + 1) * gw + ix) * 2;
            let e_left = (iy * gw + ix) * 2 + 1;

            // Lazily compute interpolated edge coordinates
            if (mask & 1 != 0) != (mask & 2 != 0) {
                edge_pos.entry(e_top).or_insert_with(|| Point2D::new(x0 + interp(v00, v10) * cell, y0));
            }
            if (mask & 2 != 0) != (mask & 4 != 0) {
                edge_pos.entry(e_right).or_insert_with(|| Point2D::new(x1, y0 + interp(v10, v11) * cell));
            }
            if (mask & 8 != 0) != (mask & 4 != 0) {
                edge_pos.entry(e_bottom).or_insert_with(|| Point2D::new(x0 + interp(v01, v11) * cell, y1));
            }
            if (mask & 1 != 0) != (mask & 8 != 0) {
                edge_pos.entry(e_left).or_insert_with(|| Point2D::new(x0, y0 + interp(v00, v01) * cell));
            }

            // Connect directed segments keeping inside (<= 0) on the left
            match mask {
                1 => { next_edge.insert(e_top, e_left); }
                2 => { next_edge.insert(e_right, e_top); }
                3 => { next_edge.insert(e_right, e_left); }
                4 => { next_edge.insert(e_bottom, e_right); }
                5 => {
                    let center = 0.25 * (v00 + v10 + v11 + v01);
                    if center <= 0.0 {
                        next_edge.insert(e_top, e_right);
                        next_edge.insert(e_bottom, e_left);
                    } else {
                        next_edge.insert(e_top, e_left);
                        next_edge.insert(e_bottom, e_right);
                    }
                }
                6 => { next_edge.insert(e_bottom, e_top); }
                7 => { next_edge.insert(e_bottom, e_left); }
                8 => { next_edge.insert(e_left, e_bottom); }
                9 => { next_edge.insert(e_top, e_bottom); }
                10 => {
                    let center = 0.25 * (v00 + v10 + v11 + v01);
                    if center <= 0.0 {
                        next_edge.insert(e_right, e_top);
                        next_edge.insert(e_left, e_bottom);
                    } else {
                        next_edge.insert(e_right, e_bottom);
                        next_edge.insert(e_left, e_top);
                    }
                }
                11 => { next_edge.insert(e_right, e_bottom); }
                12 => { next_edge.insert(e_left, e_right); }
                13 => { next_edge.insert(e_top, e_right); }
                14 => { next_edge.insert(e_left, e_top); }
                _ => {}
            }
        }
    }

    // 5. Trace closed loops from directed edges
    let mut visited: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut start_keys: Vec<usize> = next_edge.keys().copied().collect();
    start_keys.sort_unstable();

    let mut result_polylines = Vec::new();
    for start_e in start_keys {
        if visited.contains(&start_e) {
            continue;
        }
        let mut loop_pts = Vec::new();
        let mut curr = start_e;
        while !visited.contains(&curr) {
            visited.insert(curr);
            if let Some(&pt) = edge_pos.get(&curr) {
                loop_pts.push(pt);
            }
            if let Some(&nxt) = next_edge.get(&curr) {
                curr = nxt;
            } else {
                break;
            }
        }

        if loop_pts.len() >= 4 {
            // 2 passes of gentle Laplacian curve fairing to remove marching-squares grid staircase
            let mut smoothed = loop_pts;
            for _ in 0..2 {
                let m = smoothed.len();
                let mut next_pass = Vec::with_capacity(m);
                for i in 0..m {
                    let prev = smoothed[(i + m - 1) % m];
                    let cur = smoothed[i];
                    let nxt = smoothed[(i + 1) % m];
                    next_pass.push(Point2D::new(
                        0.22 * prev.x + 0.56 * cur.x + 0.22 * nxt.x,
                        0.22 * prev.y + 0.56 * cur.y + 0.22 * nxt.y,
                    ));
                }
                smoothed = next_pass;
            }

            // Simplify nearly-collinear vertices
            let tol = (cell * 0.16).clamp(0.015, 0.06);
            let simplified = simplify_closed_polyline(&smoothed, tol);
            if simplified.len() >= 3 {
                let poly = Polyline::new(simplified, true);
                if poly.length() > cell * 3.0 {
                    result_polylines.push(poly);
                }
            }
        }
    }

    result_polylines
}

fn simplify_closed_polyline(pts: &[Point2D], tol: f64) -> Vec<Point2D> {
    if pts.len() <= 4 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(pts.len());
    out.push(pts[0]);
    for i in 1..pts.len() {
        let prev = *out.last().unwrap();
        let curr = pts[i];
        let next = pts[(i + 1) % pts.len()];
        let d = point_to_segment_distance(&curr, &prev, &next);
        if d > tol || prev.distance_to(&curr) > tol * 12.0 {
            out.push(curr);
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Subtract,
    Intersect,
}

/// Performs an exact 2D Boolean operation (`Union`, `Subtract` A - B, or `Intersect` A ∩ B)
/// on two sets of closed polylines using exact segment-segment intersection subdivision,
/// even-odd interior classification, and directed boundary chaining.
pub fn boolean_op_polylines(
    polys_a: &[Polyline],
    polys_b: &[Polyline],
    op: BooleanOp,
) -> Vec<Polyline> {
    let clean_set = |polys: &[Polyline], dx_eps: f64, dy_eps: f64| -> Vec<Polyline> {
        let mut out = Vec::new();
        for p in polys {
            if p.points.len() < 3 {
                continue;
            }
            let mut pts: Vec<Point2D> = Vec::with_capacity(p.points.len());
            for &pt in &p.points {
                let shifted = Point2D::new(pt.x + dx_eps, pt.y + dy_eps);
                if pts.last().map_or(true, |last: &Point2D| last.distance_to(&shifted) > 1e-6) {
                    pts.push(shifted);
                }
            }
            if pts.len() > 2 && pts.first().unwrap().distance_to(pts.last().unwrap()) <= 1e-6 {
                pts.pop();
            }
            if pts.len() >= 3 {
                out.push(Polyline::new(pts, true));
            }
        }
        // Normalize winding so outer contours have signed_area > 0 and holes have signed_area < 0
        let n = out.len();
        let mut depths = vec![0usize; n];
        for i in 0..n {
            let p0 = out[i].points[0];
            let p1 = out[i].points[1];
            let test_pt = Point2D::new(0.5 * (p0.x + p1.x), 0.5 * (p0.y + p1.y));
            let mut d = 0usize;
            for j in 0..n {
                if i != j && out[j].contains_point(&test_pt) {
                    d += 1;
                }
            }
            depths[i] = d;
        }
        for i in 0..n {
            let area = out[i].signed_area();
            let want_positive = depths[i] % 2 == 0;
            if (want_positive && area < 0.0) || (!want_positive && area > 0.0) {
                out[i].reverse();
            }
        }
        out
    };

    let clean_a = clean_set(polys_a, 0.0, 0.0);
    // Tiny sub-micron perturbation on B avoids exact vertex-on-edge and collinear edge degeneracies
    let clean_b = clean_set(polys_b, 1.37e-5, 1.91e-5);

    if clean_a.is_empty() {
        return match op {
            BooleanOp::Union => clean_set(polys_b, 0.0, 0.0),
            BooleanOp::Subtract | BooleanOp::Intersect => Vec::new(),
        };
    }
    if clean_b.is_empty() {
        return match op {
            BooleanOp::Union | BooleanOp::Subtract => clean_a,
            BooleanOp::Intersect => Vec::new(),
        };
    }

    let mut edges_a: Vec<(Point2D, Point2D)> = Vec::new();
    for poly in &clean_a {
        let m = poly.points.len();
        for i in 0..m {
            edges_a.push((poly.points[i], poly.points[(i + 1) % m]));
        }
    }

    let mut edges_b: Vec<(Point2D, Point2D)> = Vec::new();
    for poly in &clean_b {
        let m = poly.points.len();
        for i in 0..m {
            edges_b.push((poly.points[i], poly.points[(i + 1) % m]));
        }
    }

    let mut splits_a: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; edges_a.len()];
    let mut splits_b: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; edges_b.len()];

    for (ia, &(a0, a1)) in edges_a.iter().enumerate() {
        let dax = a1.x - a0.x;
        let day = a1.y - a0.y;
        let a_min_x = a0.x.min(a1.x) - 1e-7;
        let a_max_x = a0.x.max(a1.x) + 1e-7;
        let a_min_y = a0.y.min(a1.y) - 1e-7;
        let a_max_y = a0.y.max(a1.y) + 1e-7;

        for (ib, &(b0, b1)) in edges_b.iter().enumerate() {
            if b0.x.max(b1.x) < a_min_x
                || b0.x.min(b1.x) > a_max_x
                || b0.y.max(b1.y) < a_min_y
                || b0.y.min(b1.y) > a_max_y
            {
                continue;
            }
            let dbx = b1.x - b0.x;
            let dby = b1.y - b0.y;
            let denom = dax * dby - day * dbx;
            if denom.abs() < 1e-12 {
                continue;
            }
            let dx = b0.x - a0.x;
            let dy = b0.y - a0.y;
            let ta = (dx * dby - dy * dbx) / denom;
            let tb = (dx * day - dy * dax) / denom;
            if ta >= -1e-7 && ta <= 1.0 + 1e-7 && tb >= -1e-7 && tb <= 1.0 + 1e-7 {
                splits_a[ia].push(ta.clamp(0.0, 1.0));
                splits_b[ib].push(tb.clamp(0.0, 1.0));
            }
        }
    }

    let in_region = |polys: &[Polyline], pt: &Point2D| -> bool {
        let mut count = 0usize;
        for c in polys {
            if c.contains_point(pt) {
                count += 1;
            }
        }
        count % 2 == 1
    };

    let mut kept_segments: Vec<(Point2D, Point2D)> = Vec::new();

    for (ia, &(a0, a1)) in edges_a.iter().enumerate() {
        let ts = &mut splits_a[ia];
        ts.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        for w in ts.windows(2) {
            let t0 = w[0];
            let t1 = w[1];
            if t1 - t0 < 1e-6 {
                continue;
            }
            let p0 = a0.lerp(&a1, t0);
            let p1 = a0.lerp(&a1, t1);
            if p0.distance_to(&p1) < 1e-6 {
                continue;
            }
            let mid = Point2D::new(0.5 * (p0.x + p1.x), 0.5 * (p0.y + p1.y));
            let inside_b = in_region(&clean_b, &mid);
            let keep = match op {
                BooleanOp::Union | BooleanOp::Subtract => !inside_b,
                BooleanOp::Intersect => inside_b,
            };
            if keep {
                kept_segments.push((p0, p1));
            }
        }
    }

    for (ib, &(b0, b1)) in edges_b.iter().enumerate() {
        let ts = &mut splits_b[ib];
        ts.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        for w in ts.windows(2) {
            let t0 = w[0];
            let t1 = w[1];
            if t1 - t0 < 1e-6 {
                continue;
            }
            let p0 = b0.lerp(&b1, t0);
            let p1 = b0.lerp(&b1, t1);
            if p0.distance_to(&p1) < 1e-6 {
                continue;
            }
            let mid = Point2D::new(0.5 * (p0.x + p1.x), 0.5 * (p0.y + p1.y));
            let inside_a = in_region(&clean_a, &mid);
            match op {
                BooleanOp::Union => {
                    if !inside_a {
                        kept_segments.push((p0, p1));
                    }
                }
                BooleanOp::Subtract => {
                    if inside_a {
                        kept_segments.push((p1, p0));
                    }
                }
                BooleanOp::Intersect => {
                    if inside_a {
                        kept_segments.push((p0, p1));
                    }
                }
            }
        }
    }

    // Chain directed segments into closed loops
    let mut used = vec![false; kept_segments.len()];
    let mut result = Vec::new();
    let connect_tol = 2e-3;

    for start_idx in 0..kept_segments.len() {
        if used[start_idx] {
            continue;
        }
        used[start_idx] = true;
        let mut pts = vec![kept_segments[start_idx].0, kept_segments[start_idx].1];

        loop {
            let curr_end = *pts.last().unwrap();
            if pts.len() >= 3 && curr_end.distance_to(&pts[0]) <= connect_tol {
                pts.pop(); // closed back to pts[0]
                break;
            }
            let mut best_next = None;
            let mut best_dist = connect_tol;
            for (j, &(s0, _)) in kept_segments.iter().enumerate() {
                if !used[j] {
                    let d = curr_end.distance_to(&s0);
                    if d <= best_dist {
                        best_dist = d;
                        best_next = Some(j);
                    }
                }
            }
            if let Some(nxt_idx) = best_next {
                used[nxt_idx] = true;
                pts.push(kept_segments[nxt_idx].1);
            } else {
                break;
            }
        }

        if pts.len() >= 3 {
            // Merge collinear vertices along straight segments
            let mut simplified = Vec::with_capacity(pts.len());
            let m = pts.len();
            for i in 0..m {
                let prev = pts[(i + m - 1) % m];
                let curr = pts[i];
                let next = pts[(i + 1) % m];
                if point_to_segment_distance(&curr, &prev, &next) > 1e-4 {
                    simplified.push(curr);
                }
            }
            if simplified.len() >= 3 {
                let poly = Polyline::new(simplified, true);
                if poly.length() > 0.05 {
                    result.push(poly);
                }
            }
        }
    }

    result
}

/// Welds / unions a collection of closed polylines (such as cursive text or overlapping shapes),
/// preserving all inner holes (counters) and outer contours without filling in letter holes.
pub fn weld_polylines(polys: &[Polyline]) -> Vec<Polyline> {
    if polys.len() <= 1 {
        return polys.to_vec();
    }

    let mut closed_polys = Vec::new();
    let mut open_polys = Vec::new();

    for p in polys {
        if p.is_closed && p.points.len() >= 3 {
            closed_polys.push(p.clone());
        } else {
            open_polys.push(p.clone());
        }
    }

    if closed_polys.is_empty() {
        return polys.to_vec();
    }

    // 1. Classify closed polylines into outers and holes by nesting depth
    let n = closed_polys.len();
    let mut depths = vec![0usize; n];
    for i in 0..n {
        let p0 = closed_polys[i].points[0];
        let p1 = closed_polys[i].points[1];
        let test_pt = Point2D::new(0.5 * (p0.x + p1.x), 0.5 * (p0.y + p1.y));
        let mut d = 0usize;
        for j in 0..n {
            if i != j && closed_polys[j].contains_point(&test_pt) {
                d += 1;
            }
        }
        depths[i] = d;
    }

    let mut outers: Vec<Polyline> = Vec::new();
    let mut holes: Vec<Polyline> = Vec::new();

    for i in 0..n {
        if depths[i] % 2 == 0 {
            outers.push(closed_polys[i].clone());
        } else {
            holes.push(closed_polys[i].clone());
        }
    }

    // Optimization: Check if any outer contours overlap with each other
    let any_outers_overlap = outers.len() > 1 && {
        let bboxes: Vec<_> = outers.iter().map(|p| p.bounds()).collect();
        let mut overlap = false;
        'outer_check: for i in 0..outers.len() {
            for j in (i + 1)..outers.len() {
                if bboxes[i].intersects(&bboxes[j]) {
                    overlap = true;
                    break 'outer_check;
                }
            }
        }
        overlap
    };

    // If no outer contours overlap (such as normal printed text), nothing needs welding!
    // Returning the original polylines untouched preserves perfect original glyph geometry.
    if !any_outers_overlap {
        return polys.to_vec();
    }

    // 2. Union overlapping outers together
    let mut unioned_outers: Vec<Polyline> = Vec::new();
    for o in outers {
        if unioned_outers.is_empty() {
            unioned_outers = vec![o];
        } else {
            unioned_outers = boolean_op_polylines(&unioned_outers, &[o], BooleanOp::Union);
        }
    }

    // 3. If there are no holes, return unioned outers + open polys
    if holes.is_empty() {
        unioned_outers.extend(open_polys);
        return unioned_outers;
    }

    // 4. Union holes together if multiple holes overlap
    let mut unioned_holes: Vec<Polyline> = Vec::new();
    for h in holes {
        if unioned_holes.is_empty() {
            unioned_holes = vec![h];
        } else {
            unioned_holes = boolean_op_polylines(&unioned_holes, &[h], BooleanOp::Union);
        }
    }

    // 5. Subtract the unioned holes from the unioned outers
    let mut result = boolean_op_polylines(&unioned_outers, &unioned_holes, BooleanOp::Subtract);
    result.extend(open_polys);
    result
}

/// Evaluates a Cubic Bezier curve at parameter t in [0.0, 1.0]
pub fn eval_cubic_bezier(p0: Point2D, p1: Point2D, p2: Point2D, p3: Point2D, t: f64) -> Point2D {
    let u = 1.0 - t;
    let tt = t * t;
    let uu = u * u;
    let uuu = uu * u;
    let ttt = tt * t;

    Point2D::new(
        uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x,
        uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y,
    )
}

/// Recursively flattens a cubic bezier curve into points within chordal deviation tolerance
pub fn tessellate_cubic_bezier(
    p0: Point2D,
    p1: Point2D,
    p2: Point2D,
    p3: Point2D,
    tolerance: f64,
    points: &mut Vec<Point2D>,
) {
    let chord = p3.distance_to(&p0);
    let d1 = p1.distance_to(&p0.lerp(&p3, 1.0 / 3.0));
    let d2 = p2.distance_to(&p0.lerp(&p3, 2.0 / 3.0));

    if (d1 + d2) <= tolerance || chord < 1e-4 {
        points.push(p3);
    } else {
        // De Casteljau subdivision
        let p01 = p0.lerp(&p1, 0.5);
        let p12 = p1.lerp(&p2, 0.5);
        let p23 = p2.lerp(&p3, 0.5);

        let p012 = p01.lerp(&p12, 0.5);
        let p123 = p12.lerp(&p23, 0.5);

        let p0123 = p012.lerp(&p123, 0.5);

        tessellate_cubic_bezier(p0, p01, p012, p0123, tolerance, points);
        tessellate_cubic_bezier(p0123, p123, p23, p3, tolerance, points);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polyline_contains_point() {
        let pts = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(10.0, 0.0),
            Point2D::new(10.0, 10.0),
            Point2D::new(0.0, 10.0),
        ];
        let poly = Polyline::new(pts, true);
        assert!(poly.contains_point(&Point2D::new(5.0, 5.0)));
        assert!(!poly.contains_point(&Point2D::new(15.0, 5.0)));
        assert!(!poly.contains_point(&Point2D::new(-5.0, 5.0)));
    }

    #[test]
    fn test_polyline_offset_expand_and_shrink() {
        // CCW Square 10x10 from (0,0) to (10,10)
        let pts = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(10.0, 0.0),
            Point2D::new(10.0, 10.0),
            Point2D::new(0.0, 10.0),
        ];
        let poly = Polyline::new(pts, true);

        // Expand outward by 0.1 mm (+d)
        let expanded = poly.offset(0.1);
        let exp_bounds = expanded.bounds();
        assert!((exp_bounds.min_x - (-0.1)).abs() < 1e-4);
        assert!((exp_bounds.min_y - (-0.1)).abs() < 1e-4);
        assert!((exp_bounds.max_x - 10.1).abs() < 1e-4);
        assert!((exp_bounds.max_y - 10.1).abs() < 1e-4);
        assert!((exp_bounds.width() - 10.2).abs() < 1e-4);
        assert!((exp_bounds.height() - 10.2).abs() < 1e-4);

        // Shrink inward by 0.1 mm (-d)
        let shrunk = poly.offset(-0.1);
        let sh_bounds = shrunk.bounds();
        assert!((sh_bounds.min_x - 0.1).abs() < 1e-4);
        assert!((sh_bounds.min_y - 0.1).abs() < 1e-4);
        assert!((sh_bounds.max_x - 9.9).abs() < 1e-4);
        assert!((sh_bounds.max_y - 9.9).abs() < 1e-4);
        assert!((sh_bounds.width() - 9.8).abs() < 1e-4);
        assert!((sh_bounds.height() - 9.8).abs() < 1e-4);

        // CW Square 10x10
        let pts_cw = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(0.0, 10.0),
            Point2D::new(10.0, 10.0),
            Point2D::new(10.0, 0.0),
        ];
        let poly_cw = Polyline::new(pts_cw, true);
        let expanded_cw = poly_cw.offset(0.1);
        let exp_cw_bounds = expanded_cw.bounds();
        assert!((exp_cw_bounds.width() - 10.2).abs() < 1e-4);
        assert!((exp_cw_bounds.height() - 10.2).abs() < 1e-4);
    }

    #[test]
    fn test_boolean_union_subtract_intersect() {
        let sq_a = Polyline::new(
            vec![
                Point2D::new(0.0, 0.0),
                Point2D::new(10.0, 0.0),
                Point2D::new(10.0, 10.0),
                Point2D::new(0.0, 10.0),
            ],
            true,
        );
        let sq_b = Polyline::new(
            vec![
                Point2D::new(5.0, 5.0),
                Point2D::new(15.0, 5.0),
                Point2D::new(15.0, 15.0),
                Point2D::new(5.0, 15.0),
            ],
            true,
        );

        let u = boolean_op_polylines(std::slice::from_ref(&sq_a), std::slice::from_ref(&sq_b), BooleanOp::Union);
        assert_eq!(u.len(), 1);
        let ub = u[0].bounds();
        assert!((ub.width() - 15.0).abs() < 1e-2);
        assert!((ub.height() - 15.0).abs() < 1e-2);

        let sub = boolean_op_polylines(std::slice::from_ref(&sq_a), std::slice::from_ref(&sq_b), BooleanOp::Subtract);
        assert_eq!(sub.len(), 1);
        let sb = sub[0].bounds();
        assert!((sb.width() - 10.0).abs() < 1e-2);
        assert!((sb.height() - 10.0).abs() < 1e-2);
        assert!((sub[0].signed_area().abs() - 75.0).abs() < 0.1);

        let inter = boolean_op_polylines(std::slice::from_ref(&sq_a), std::slice::from_ref(&sq_b), BooleanOp::Intersect);
        assert_eq!(inter.len(), 1);
        let ib = inter[0].bounds();
        assert!((ib.width() - 5.0).abs() < 1e-2);
        assert!((ib.height() - 5.0).abs() < 1e-2);
        assert!((inter[0].signed_area().abs() - 25.0).abs() < 0.1);
    }

    #[test]
    fn test_weld_polylines_with_holes_and_non_overlapping() {
        // Outer square 20x20 with inner hole 10x10 (like an 'O')
        let outer_o = Polyline::new(
            vec![
                Point2D::new(0.0, 0.0),
                Point2D::new(20.0, 0.0),
                Point2D::new(20.0, 20.0),
                Point2D::new(0.0, 20.0),
            ],
            true,
        );
        let hole_o = Polyline::new(
            vec![
                Point2D::new(5.0, 5.0),
                Point2D::new(5.0, 15.0),
                Point2D::new(15.0, 15.0),
                Point2D::new(15.0, 5.0),
            ],
            true,
        );

        // A second non-overlapping shape at x=30 (like another letter)
        let outer_2 = Polyline::new(
            vec![
                Point2D::new(30.0, 0.0),
                Point2D::new(40.0, 0.0),
                Point2D::new(40.0, 20.0),
                Point2D::new(30.0, 20.0),
            ],
            true,
        );

        let polys = vec![outer_o, hole_o, outer_2];
        let welded = weld_polylines(&polys);

        // Because outer_o and outer_2 do not overlap, all 3 polylines (including the hole!) must be preserved
        assert_eq!(welded.len(), 3, "Non-overlapping shapes must preserve all contours and holes");

        // Now test overlapping cursive shapes with holes:
        // Overlap a rectangle across the outer_o at x=15..25 (touches outer_o but doesn't touch the hole at 5..15)
        let overlap_rect = Polyline::new(
            vec![
                Point2D::new(15.0, 8.0),
                Point2D::new(30.0, 8.0),
                Point2D::new(30.0, 12.0),
                Point2D::new(15.0, 12.0),
            ],
            true,
        );

        let polys_overlap = vec![
            Polyline::new(vec![Point2D::new(0.0, 0.0), Point2D::new(20.0, 0.0), Point2D::new(20.0, 20.0), Point2D::new(0.0, 20.0)], true),
            Polyline::new(vec![Point2D::new(5.0, 5.0), Point2D::new(5.0, 15.0), Point2D::new(15.0, 15.0), Point2D::new(15.0, 5.0)], true),
            overlap_rect,
        ];
        let welded_overlap = weld_polylines(&polys_overlap);

        // Must produce 1 unioned outer contour and 1 preserved inner hole = 2 polylines!
        assert_eq!(welded_overlap.len(), 2, "Overlapping weld must produce 1 unioned outer and 1 hole");
        let has_hole = welded_overlap.iter().any(|p| p.signed_area() < 0.0 || (p.signed_area().abs() - 100.0).abs() < 1.0);
        assert!(has_hole, "Inner hole must be preserved after weld");
    }
}

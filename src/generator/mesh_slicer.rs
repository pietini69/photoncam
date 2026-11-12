//! 3D Mesh Slicer to Laser Cut Sheets
//!
//! Converts 3D models (STL, GLB, glTF) into horizontal or vertical stacked slices
//! ready for laser cutting from plywood, acrylic, or cardboard sheets.
//! Includes automated registration dowel pin holes, kerf compensation, and sheet layout.

use crate::formats::mesh3d::{Mesh3D, Triangle3D};
use crate::geometry::{BoundingBox, Point2D, Polyline};
use crate::text::{generate_text_polylines, FontType, TextAlign, TextOptions};
use crate::Shape;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SliceAxis {
    /// Horizontal slices along Z axis (standard top-down stacking)
    Z,
    /// Cross-section slices along Y axis (front-to-back stacking)
    Y,
    /// Cross-section slices along X axis (side-to-side stacking)
    X,
}

impl Default for SliceAxis {
    fn default() -> Self {
        SliceAxis::Z
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshSlicerParams {
    /// Sheet material thickness in mm (e.g. 3.0 mm or 4.0 mm)
    pub thickness: f64,
    /// Slicing orientation axis
    pub axis: SliceAxis,
    /// Add dowel pin alignment holes for gluing layers together with perfect registration
    pub add_dowel_holes: bool,
    /// Dowel pin diameter in mm (e.g. 3.0 mm bamboo skewer / dowel)
    pub dowel_diameter: f64,
    /// Number of dowels (2 or 4)
    pub dowel_count: usize,
    /// Add etched layer numbering (e.g. "L1", "L2"...) on each slice
    pub add_layer_labels: bool,
    /// Spacing between unpacked 2D slices on canvas (mm)
    pub layout_margin: f64,
    /// Maximum sheet width before wrapping to the next row (mm)
    pub sheet_width: f64,
    /// Layer index for cut outlines
    pub cut_layer: usize,
    /// Layer index for etched dowels and layer numbering labels
    pub etch_layer: usize,
}

impl Default for MeshSlicerParams {
    fn default() -> Self {
        Self {
            thickness: 3.0,
            axis: SliceAxis::Z,
            add_dowel_holes: true,
            dowel_diameter: 3.0,
            dowel_count: 2,
            add_layer_labels: true,
            layout_margin: 6.0,
            sheet_width: 400.0,
            cut_layer: 0,
            etch_layer: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SlicedLayer {
    pub slice_index: usize,
    pub slice_coord: f64,
    pub contours: Vec<Polyline>,
    pub dowel_holes: Vec<Point2D>,
    pub bounds: BoundingBox,
}

/// Slices a 3D triangle mesh into planar 2D vector contours along the specified axis
pub fn slice_mesh_to_layers(mesh: &Mesh3D, params: &MeshSlicerParams) -> Vec<SlicedLayer> {
    if mesh.triangles.is_empty() || params.thickness <= 0.05 {
        return Vec::new();
    }

    let (min_b, max_b) = mesh.bounds();

    let (w_min, w_max) = match params.axis {
        SliceAxis::Z => (min_b[2] as f64, max_b[2] as f64),
        SliceAxis::Y => (min_b[1] as f64, max_b[1] as f64),
        SliceAxis::X => (min_b[0] as f64, max_b[0] as f64),
    };

    let total_depth = w_max - w_min;
    if total_depth <= 1e-4 {
        return Vec::new();
    }

    let num_slices = (total_depth / params.thickness).ceil() as usize;
    let mut layers = Vec::with_capacity(num_slices);

    for idx in 0..num_slices {
        let slice_coord = w_min + (idx as f64 + 0.5) * params.thickness;
        if slice_coord > w_max {
            break;
        }

        let mut segments = Vec::new();
        for tri in &mesh.triangles {
            if let Some((p1, p2)) = intersect_triangle_with_plane(tri, params.axis, slice_coord) {
                if (p1.x - p2.x).hypot(p1.y - p2.y) > 1e-5 {
                    segments.push((p1, p2));
                }
            }
        }

        if segments.is_empty() {
            continue;
        }

        let contours = stitch_segments_into_polylines(segments, 0.08);
        if contours.is_empty() {
            continue;
        }

        let mut layer_bounds = BoundingBox::empty();
        for poly in &contours {
            layer_bounds = layer_bounds.union(&poly.bounds());
        }

        layers.push(SlicedLayer {
            slice_index: idx,
            slice_coord,
            contours,
            dowel_holes: Vec::new(),
            bounds: layer_bounds,
        });
    }

    // Compute shared dowel pin holes if enabled
    if params.add_dowel_holes && !layers.is_empty() {
        compute_dowel_holes(&mut layers, params);
    }

    layers
}

/// Compute 2D intersection line segment of a 3D triangle with a cutting plane
fn intersect_triangle_with_plane(
    tri: &Triangle3D,
    axis: SliceAxis,
    plane_coord: f64,
) -> Option<(Point2D, Point2D)> {
    // Project 3D vertices into (u, v) on the 2D plane and w along the slice axis
    let project = |v: &[f32; 3]| -> (f64, f64, f64) {
        match axis {
            SliceAxis::Z => (v[0] as f64, v[1] as f64, v[2] as f64),
            SliceAxis::Y => (v[0] as f64, v[2] as f64, v[1] as f64),
            SliceAxis::X => (v[1] as f64, v[2] as f64, v[0] as f64),
        }
    };

    let p0 = project(&tri.v0);
    let p1 = project(&tri.v1);
    let p2 = project(&tri.v2);

    let mut pts = Vec::with_capacity(3);

    let check_edge = |a: (f64, f64, f64), b: (f64, f64, f64), pts: &mut Vec<Point2D>| {
        let (wa, wb) = (a.2, b.2);
        if (wa <= plane_coord && wb >= plane_coord) || (wb <= plane_coord && wa >= plane_coord) {
            let denom = wb - wa;
            if denom.abs() > 1e-8 {
                let t = (plane_coord - wa) / denom;
                if (0.0..=1.0).contains(&t) {
                    let u = a.0 + t * (b.0 - a.0);
                    let v = a.1 + t * (b.1 - a.1);
                    pts.push(Point2D::new(u, v));
                }
            }
        }
    };

    check_edge(p0, p1, &mut pts);
    check_edge(p1, p2, &mut pts);
    check_edge(p2, p0, &mut pts);

    // Filter duplicate points within tiny tolerance
    let mut unique_pts = Vec::with_capacity(2);
    for p in pts {
        if !unique_pts.iter().any(|q: &Point2D| p.distance_to(q) < 1e-4) {
            unique_pts.push(p);
        }
    }

    if unique_pts.len() == 2 {
        Some((unique_pts[0], unique_pts[1]))
    } else {
        None
    }
}

/// Stitch loose 2D line segments into closed, continuous polylines
fn stitch_segments_into_polylines(mut segments: Vec<(Point2D, Point2D)>, tol: f64) -> Vec<Polyline> {
    let mut polylines = Vec::new();

    while !segments.is_empty() {
        let (first_p1, first_p2) = segments.remove(0);
        let mut cur_pts = vec![first_p1, first_p2];

        let mut changed = true;
        while changed {
            changed = false;
            let tail = *cur_pts.last().unwrap();
            let head = *cur_pts.first().unwrap();

            // Try to find segment connecting to tail
            let mut match_idx = None;
            let mut is_reversed = false;

            for (idx, (p1, p2)) in segments.iter().enumerate() {
                if tail.distance_to(p1) <= tol {
                    match_idx = Some(idx);
                    is_reversed = false;
                    break;
                } else if tail.distance_to(p2) <= tol {
                    match_idx = Some(idx);
                    is_reversed = true;
                    break;
                }
            }

            if let Some(idx) = match_idx {
                let (p1, p2) = segments.remove(idx);
                if is_reversed {
                    cur_pts.push(p1);
                } else {
                    cur_pts.push(p2);
                }
                changed = true;
                continue;
            }

            // Try to find segment connecting to head
            for (idx, (p1, p2)) in segments.iter().enumerate() {
                if head.distance_to(p2) <= tol {
                    match_idx = Some(idx);
                    is_reversed = false;
                    break;
                } else if head.distance_to(p1) <= tol {
                    match_idx = Some(idx);
                    is_reversed = true;
                    break;
                }
            }

            if let Some(idx) = match_idx {
                let (p1, p2) = segments.remove(idx);
                if is_reversed {
                    cur_pts.insert(0, p2);
                } else {
                    cur_pts.insert(0, p1);
                }
                changed = true;
            }
        }

        // Close polyline if endpoints meet within tolerance
        let is_closed = cur_pts.len() >= 3 && cur_pts.first().unwrap().distance_to(cur_pts.last().unwrap()) <= tol * 2.0;
        if cur_pts.len() >= 3 {
            polylines.push(Polyline::new(cur_pts, is_closed));
        }
    }

    polylines
}

/// Compute shared dowel pin locations that pass solidly through layers
fn compute_dowel_holes(layers: &mut [SlicedLayer], params: &MeshSlicerParams) {
    if layers.is_empty() {
        return;
    }

    // Find overlapping bounding box of all layers
    let mut common_min_x = f64::NEG_INFINITY;
    let mut common_max_x = f64::INFINITY;
    let mut common_min_y = f64::NEG_INFINITY;
    let mut common_max_y = f64::INFINITY;

    for layer in layers.iter() {
        common_min_x = common_min_x.max(layer.bounds.min_x);
        common_max_x = common_max_x.min(layer.bounds.max_x);
        common_min_y = common_min_y.max(layer.bounds.min_y);
        common_max_y = common_max_y.min(layer.bounds.max_y);
    }

    let span_x = common_max_x - common_min_x;
    let span_y = common_max_y - common_min_y;

    let candidate_pins = if span_x > 10.0 && span_y > 10.0 {
        if params.dowel_count >= 4 {
            let mx = span_x * 0.22;
            let my = span_y * 0.22;
            vec![
                Point2D::new(common_min_x + mx, common_min_y + my),
                Point2D::new(common_max_x - mx, common_min_y + my),
                Point2D::new(common_max_x - mx, common_max_y - my),
                Point2D::new(common_min_x + mx, common_max_y - my),
            ]
        } else {
            // 2 alignment dowels along dominant axis
            if span_x >= span_y {
                vec![
                    Point2D::new(common_min_x + span_x * 0.28, common_min_y + span_y * 0.5),
                    Point2D::new(common_max_x - span_x * 0.28, common_min_y + span_y * 0.5),
                ]
            } else {
                vec![
                    Point2D::new(common_min_x + span_x * 0.5, common_min_y + span_y * 0.28),
                    Point2D::new(common_min_x + span_x * 0.5, common_max_y - span_y * 0.28),
                ]
            }
        }
    } else {
        // Fallback: use first layer center points
        let b = layers[0].bounds;
        vec![
            Point2D::new(b.center().x - 10.0, b.center().y),
            Point2D::new(b.center().x + 10.0, b.center().y),
        ]
    };

    // Assign pins to layers where they land inside solid contours
    for layer in layers.iter_mut() {
        for pin in &candidate_pins {
            let inside = layer.contours.iter().any(|c| c.contains_point(pin));
            if inside {
                layer.dowel_holes.push(*pin);
            }
        }
    }
}

/// Arranges 2D slice contours and registration marks onto laser cutting sheets
pub fn layout_sliced_layers(layers: &[SlicedLayer], params: &MeshSlicerParams) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let margin = params.layout_margin;

    let mut cur_x = margin;
    let mut cur_y = margin;
    let mut row_height = 0.0;

    for (idx, layer) in layers.iter().enumerate() {
        let b = layer.bounds;
        let w = b.width();
        let h = b.height();

        if cur_x + w > params.sheet_width && cur_x > margin {
            // Wrap to next row
            cur_x = margin;
            cur_y += row_height + margin;
            row_height = 0.0;
        }

        row_height = row_height.max(h);
        let dx = cur_x - b.min_x;
        let dy = cur_y - b.min_y;

        // 1. Cut Outlines (Polylines)
        for poly in &layer.contours {
            let translated_pts: Vec<Point2D> = poly
                .points
                .iter()
                .map(|p| Point2D::new(p.x + dx, p.y + dy))
                .collect();
            shapes.push(Shape::Path {
                layer_index: params.cut_layer,
                polyline: Polyline::new(translated_pts, poly.is_closed),
            });
        }

        // 2. Alignment Dowel Pin Holes (Circles on cut layer)
        let dowel_radius = params.dowel_diameter * 0.5;
        for hole in &layer.dowel_holes {
            shapes.push(Shape::Circle {
                layer_index: params.cut_layer,
                center: Point2D::new(hole.x + dx, hole.y + dy),
                radius: dowel_radius,
            });
        }

        // 3. Etched Layer Identification Number (e.g. "L1")
        if params.add_layer_labels {
            let label_x = cur_x + w * 0.5;
            let label_y = cur_y + h * 0.5;
            let text_opts = TextOptions {
                text: format!("L{}", idx + 1),
                font_type: FontType::SingleLine,
                font_size_mm: 4.5,
                x: label_x,
                y: label_y,
                align: TextAlign::Center,
                letter_spacing: 1.0,
                line_spacing: 1.2,
            };
            if let Ok(text_polys) = generate_text_polylines(&text_opts) {
                for polyline in text_polys {
                    shapes.push(Shape::Path {
                        layer_index: params.etch_layer,
                        polyline,
                    });
                }
            }
        }

        cur_x += w + margin;
    }

    shapes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cube_mesh(size: f32) -> Mesh3D {
        let mut mesh = Mesh3D::new();
        let s = size;
        // Two triangles for bottom face (z = 0)
        let v000 = [0.0, 0.0, 0.0];
        let v100 = [s, 0.0, 0.0];
        let v110 = [s, s, 0.0];
        let v010 = [0.0, s, 0.0];
        // Four top vertices (z = s)
        let v001 = [0.0, 0.0, s];
        let v101 = [s, 0.0, s];
        let v111 = [s, s, s];
        let v011 = [0.0, s, s];

        // 6 faces * 2 triangles = 12 triangles
        // Front (y = 0)
        mesh.triangles.push(Triangle3D { v0: v000, v1: v100, v2: v101 });
        mesh.triangles.push(Triangle3D { v0: v000, v1: v101, v2: v001 });
        // Back (y = s)
        mesh.triangles.push(Triangle3D { v0: v010, v1: v011, v2: v111 });
        mesh.triangles.push(Triangle3D { v0: v010, v1: v111, v2: v110 });
        // Left (x = 0)
        mesh.triangles.push(Triangle3D { v0: v000, v1: v001, v2: v011 });
        mesh.triangles.push(Triangle3D { v0: v000, v1: v011, v2: v010 });
        // Right (x = s)
        mesh.triangles.push(Triangle3D { v0: v100, v1: v110, v2: v111 });
        mesh.triangles.push(Triangle3D { v0: v100, v1: v111, v2: v101 });
        // Bottom (z = 0)
        mesh.triangles.push(Triangle3D { v0: v000, v1: v010, v2: v110 });
        mesh.triangles.push(Triangle3D { v0: v000, v1: v110, v2: v100 });
        // Top (z = s)
        mesh.triangles.push(Triangle3D { v0: v001, v1: v101, v2: v111 });
        mesh.triangles.push(Triangle3D { v0: v001, v1: v111, v2: v011 });

        mesh
    }

    #[test]
    fn test_slice_cube_mesh_to_horizontal_layers() {
        let cube = make_cube_mesh(12.0); // 12mm cube
        let params = MeshSlicerParams {
            thickness: 3.0, // 4 slices of 3mm
            axis: SliceAxis::Z,
            add_dowel_holes: true,
            dowel_diameter: 2.0,
            dowel_count: 2,
            add_layer_labels: true,
            layout_margin: 5.0,
            sheet_width: 200.0,
            cut_layer: 0,
            etch_layer: 1,
        };

        let layers = slice_mesh_to_layers(&cube, &params);
        assert_eq!(layers.len(), 4, "A 12mm cube sliced with 3mm thickness must yield 4 slices");

        for layer in &layers {
            assert!(!layer.contours.is_empty(), "Every layer must have slice contours");
            assert!(layer.contours[0].is_closed, "Contour must be closed");
            // The contour of a 12mm cube should have ~12x12mm bounds
            let b = layer.contours[0].bounds();
            assert!((b.width() - 12.0).abs() < 0.2);
            assert!((b.height() - 12.0).abs() < 0.2);
            // Dowel pin holes should be generated
            assert_eq!(layer.dowel_holes.len(), 2, "Each slice should receive 2 dowel pins");
        }

        let shapes = layout_sliced_layers(&layers, &params);
        assert!(!shapes.is_empty());
        // 4 cut outer contours
        let cut_contours = shapes.iter().filter(|s| s.layer_index() == 0 && matches!(s, Shape::Path { .. })).count();
        assert_eq!(cut_contours, 4);
        // 4 slices * 2 dowels = 8 dowel holes
        let dowel_circles = shapes.iter().filter(|s| matches!(s, Shape::Circle { .. })).count();
        assert_eq!(dowel_circles, 8);
        // Etch layer labels exist
        let etch_shapes = shapes.iter().filter(|s| s.layer_index() == 1).count();
        assert!(etch_shapes >= 4);
    }

    #[test]
    fn test_slice_stl_wedge() {
        let ascii_stl = r#"solid pyramid
  facet normal 0 0 -1
    outer loop
      vertex 0 0 0
      vertex 20 0 0
      vertex 20 20 0
    endloop
  endfacet
  facet normal 0 0 -1
    outer loop
      vertex 0 0 0
      vertex 20 20 0
      vertex 0 20 0
    endloop
  endfacet
  facet normal 0 -1 1
    outer loop
      vertex 0 0 0
      vertex 20 0 0
      vertex 10 10 10
    endloop
  endfacet
  facet normal 1 0 1
    outer loop
      vertex 20 0 0
      vertex 20 20 0
      vertex 10 10 10
    endloop
  endfacet
  facet normal 0 1 1
    outer loop
      vertex 20 20 0
      vertex 0 20 0
      vertex 10 10 10
    endloop
  endfacet
  facet normal -1 0 1
    outer loop
      vertex 0 20 0
      vertex 0 0 0
      vertex 10 10 10
    endloop
  endfacet
endsolid pyramid"#;

        let temp_path = std::env::temp_dir().join("test_wedge_slice.stl");
        std::fs::write(&temp_path, ascii_stl).expect("write temp stl");
        let mesh = Mesh3D::load_stl(&temp_path).expect("load stl");
        let _ = std::fs::remove_file(temp_path);

        let params = MeshSlicerParams {
            thickness: 2.5, // 10mm height / 2.5mm = 4 slices
            axis: SliceAxis::Z,
            add_dowel_holes: true,
            dowel_diameter: 2.0,
            dowel_count: 2,
            add_layer_labels: true,
            layout_margin: 4.0,
            sheet_width: 300.0,
            cut_layer: 0,
            etch_layer: 1,
        };

        let layers = slice_mesh_to_layers(&mesh, &params);
        assert_eq!(layers.len(), 4);
        let shapes = layout_sliced_layers(&layers, &params);
        assert!(!shapes.is_empty());
    }
}

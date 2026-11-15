use crate::document::Document;
use crate::geometry::{Point2D, Polyline};

#[derive(Debug, Clone)]
pub struct OptimizedJob {
    pub paths: Vec<OptimizedPath>,
}

#[derive(Debug, Clone)]
pub struct OptimizedPath {
    pub layer_index: usize,
    pub polyline: Polyline,
    pub is_inner_cut: bool,
    pub powers: Option<Vec<f64>>,
    pub z_level: Option<f64>,
}

pub struct CutOptimizer {
    pub inner_cuts_first: bool,
    pub minimize_travel: bool,
    pub allow_reversal: bool,
    pub lead_in_length: f64,
    pub tab_width: f64,
    pub tab_count: usize,
    pub default_kerf: f64, // Laser beam kerf compensation in mm (0.0 = disabled)
}

impl Default for CutOptimizer {
    fn default() -> Self {
        Self {
            inner_cuts_first: true,
            minimize_travel: true,
            allow_reversal: true,
            lead_in_length: 0.0,
            tab_width: 0.0,
            tab_count: 0,
            default_kerf: 0.0,
        }
    }
}

impl CutOptimizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Optimizes all shapes in a document into an ordered sequence of laser toolpaths
    pub fn optimize(&self, doc: &Document) -> OptimizedJob {
        // Step 1: Collect all paths grouped by layer priority
        let mut layer_groups: std::collections::BTreeMap<i32, Vec<OptimizedPath>> =
            std::collections::BTreeMap::new();

        for shape in &doc.shapes {
            let layer_idx = shape.layer_index();

            // Skip Tool layers (T01, T02) and any layer with output disabled
            if let Some(layer) = doc.layers.get(&layer_idx) {
                if !layer.output || layer.is_tool() {
                    continue;
                }
            } else if layer_idx >= 30 {
                continue;
            }

            let priority = doc
                .layers
                .get(&layer_idx)
                .map(|l| l.priority)
                .unwrap_or(layer_idx as i32);

            let layer_cut_setting = doc.layers.get(&layer_idx).map(|l| &l.cut_setting);
            let paths: Vec<OptimizedPath> = if let Some(cs) = layer_cut_setting {
                if matches!(shape, crate::document::Shape::Image { .. }) || cs.mode == crate::document::CutMode::Image {
                    if cs.dither_mode == crate::document::DitherMode::Sliced3D {
                        shape.to_3d_relief_paths(cs).into_iter().map(|(poly, powers, z)| {
                            OptimizedPath {
                                layer_index: layer_idx,
                                polyline: poly,
                                is_inner_cut: false,
                                powers,
                                z_level: z,
                            }
                        }).collect()
                    } else {
                        shape.to_raster_paths(cs).into_iter().map(|(poly, powers)| {
                            OptimizedPath {
                                layer_index: layer_idx,
                                polyline: poly,
                                is_inner_cut: false,
                                powers,
                                z_level: None,
                            }
                        }).collect()
                    }
                } else if cs.mode == crate::document::CutMode::Fill {
                    let interval = if cs.line_interval > 0.005 { cs.line_interval } else { 0.08 };
                    let polys = shape.to_polylines();
                    let hatch_polys = crate::hatch::generate_hatch(&polys, cs.scan_angle, interval, cs.cross_hatch);
                    hatch_polys.into_iter().map(|p| OptimizedPath {
                        layer_index: layer_idx,
                        polyline: p,
                        is_inner_cut: false,
                        powers: None,
                        z_level: None,
                    }).collect()
                } else {
                    shape.to_polylines().into_iter().map(|p| OptimizedPath {
                        layer_index: layer_idx,
                        polyline: p,
                        is_inner_cut: false,
                        powers: None,
                        z_level: None,
                    }).collect()
                }
            } else {
                shape.to_polylines().into_iter().map(|p| OptimizedPath {
                    layer_index: layer_idx,
                    polyline: p,
                    is_inner_cut: false,
                    powers: None,
                    z_level: None,
                }).collect()
            };
            for p in paths {
                layer_groups.entry(priority).or_default().push(p);
            }
        }

        let mut final_paths: Vec<OptimizedPath> = Vec::new();
        let mut current_laser_pos = Point2D::ZERO;

        // Step 2: Process each layer by priority order
        for (_prio, items) in layer_groups {
            if items.is_empty() {
                continue;
            }

            // Image raster layers and Hatch Fill layers are already ordered scanlines
            let is_raster_layer = items
                .first()
                .and_then(|item| doc.layers.get(&item.layer_index))
                .map_or(false, |l| {
                    l.cut_setting.mode == crate::document::CutMode::Image
                        || l.cut_setting.mode == crate::document::CutMode::Fill
                });

            if is_raster_layer {
                final_paths.extend(items);
                continue;
            }

            // Step 2a: Classify inner vs outer contours for vector cuts
            let mut classified: Vec<OptimizedPath> = Vec::with_capacity(items.len());
            for i in 0..items.len() {
                let is_inner = if self.inner_cuts_first && items[i].polyline.is_closed {
                    // Check if items[i] is contained inside any other closed polygon in the layer
                    let mut contained = false;
                    if let Some(test_pt) = items[i].polyline.points.first() {
                        for (j, item) in items.iter().enumerate() {
                            if i != j && item.polyline.is_closed && item.polyline.contains_point(test_pt) {
                                contained = true;
                                break;
                            }
                        }
                    }
                    contained
                } else {
                    false
                };

                let layer_opt = doc.layers.get(&items[i].layer_index);
                let is_vector_line = layer_opt.map_or(true, |l| l.cut_setting.mode == crate::document::CutMode::Line);
                let layer_kerf = layer_opt.map(|l| l.cut_setting.kerf).unwrap_or(0.0);
                let eff_kerf = if layer_kerf != 0.0 { layer_kerf } else { self.default_kerf };

                let poly = if is_vector_line && eff_kerf.abs() > 1e-5 && items[i].polyline.is_closed {
                    // Outer cut: laser burns on cutline, so expand shape outward (+eff_kerf / 2.0)
                    // Inner cut (hole/slot): laser burns on cutline, so shrink hole inward (-eff_kerf / 2.0)
                    let offset_dist = if is_inner { -eff_kerf * 0.5 } else { eff_kerf * 0.5 };
                    items[i].polyline.offset(offset_dist)
                } else {
                    items[i].polyline.clone()
                };

                classified.push(OptimizedPath {
                    layer_index: items[i].layer_index,
                    polyline: poly,
                    is_inner_cut: is_inner,
                    powers: items[i].powers.clone(),
                    z_level: items[i].z_level,
                });
            }

            // Step 2b: Sort so inner cuts come before outer cuts
            if self.inner_cuts_first {
                classified.sort_by(|a, b| {
                    match (a.is_inner_cut, b.is_inner_cut) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    }
                });
            }

            // Step 2c: Nearest-Neighbor TSP optimization for rapid travel (vector layers only)
            if self.minimize_travel && classified.len() > 1 {
                let mut remaining = classified;
                let mut reordered: Vec<OptimizedPath> = Vec::with_capacity(remaining.len());

                while !remaining.is_empty() {
                    let mut best_idx = 0;
                    let mut best_dist_sq = f64::INFINITY;
                    let mut should_reverse = false;

                    // When inner_cuts_first is enabled, only consider inner cuts until all inner cuts are cut
                    let has_inner = self.inner_cuts_first && remaining.iter().any(|p| p.is_inner_cut);

                    for (idx, item) in remaining.iter().enumerate() {
                        if has_inner && !item.is_inner_cut {
                            continue;
                        }

                        let pts = &item.polyline.points;
                        if pts.is_empty() {
                            continue;
                        }

                        let start_dist = current_laser_pos.distance_squared_to(&pts[0]);
                        if start_dist < best_dist_sq {
                            best_dist_sq = start_dist;
                            best_idx = idx;
                            should_reverse = false;
                        }

                        // Open paths can be reversed to save rapid travel
                        if self.allow_reversal && !item.polyline.is_closed {
                            let end_dist = current_laser_pos.distance_squared_to(pts.last().unwrap());
                            if end_dist < best_dist_sq {
                                best_dist_sq = end_dist;
                                best_idx = idx;
                                should_reverse = true;
                            }
                        }
                    }

                    let mut chosen = remaining.remove(best_idx);
                    if should_reverse {
                        chosen.polyline.reverse();
                        if let Some(ref mut p) = chosen.powers {
                            p.reverse();
                        }
                    }

                    if let Some(last_pt) = chosen.polyline.points.last() {
                        current_laser_pos = *last_pt;
                    }
                    reordered.push(chosen);
                }
                classified = reordered;
            }

            // Step 2d: Insert Tabs / Bridges if configured on layer or optimizer
            let mut tabbed_paths = Vec::new();
            for path in classified {
                let layer_tabs = doc.layers.get(&path.layer_index).map(|l| (l.cut_setting.tabs_enabled, l.cut_setting.tab_width, l.cut_setting.tab_count));
                let (tabs_active, t_width, t_count) = match layer_tabs {
                    Some((true, w, c)) if w > 0.0 && c > 0 => (true, w, c),
                    _ => (self.tab_width > 0.0 && self.tab_count > 0, self.tab_width, self.tab_count),
                };

                if tabs_active && path.polyline.is_closed && path.polyline.length() > t_width * (t_count as f64 + 1.0) {
                    let segments = self.insert_tabs(&path.polyline, t_width, t_count);
                    for seg in segments {
                        tabbed_paths.push(OptimizedPath {
                            layer_index: path.layer_index,
                            polyline: seg,
                            is_inner_cut: path.is_inner_cut,
                            powers: None,
                            z_level: path.z_level,
                        });
                    }
                } else {
                    tabbed_paths.push(path);
                }
            }
            classified = tabbed_paths;

            final_paths.extend(classified);
        }

        OptimizedJob { paths: final_paths }
    }

    /// Splits a closed polyline by injecting tab gaps of specified width
    fn insert_tabs(&self, poly: &Polyline, tab_width: f64, tab_count: usize) -> Vec<Polyline> {
        let total_len = poly.length();
        if total_len <= tab_width * 2.0 || tab_count == 0 {
            return vec![poly.clone()];
        }

        let interval = total_len / tab_count as f64;
        let mut segments = Vec::new();
        let mut current_segment = Vec::new();

        let mut accumulated_dist = 0.0;
        let mut next_tab_start = interval - (tab_width * 0.5);

        for i in 0..poly.points.len() {
            let p1 = poly.points[i];
            let p2 = poly.points[(i + 1) % poly.points.len()];
            let seg_len = p1.distance_to(&p2);

            current_segment.push(p1);

            if accumulated_dist + seg_len >= next_tab_start {
                // Tab starts on this line segment
                let t_start = ((next_tab_start - accumulated_dist) / seg_len).clamp(0.0, 1.0);
                let tab_pt1 = p1.lerp(&p2, t_start);
                current_segment.push(tab_pt1);
                segments.push(Polyline::new(current_segment, false));

                // Skip the tab gap
                let tab_end_dist = next_tab_start + tab_width;
                let t_end = ((tab_end_dist - accumulated_dist) / seg_len).clamp(0.0, 1.0);
                let tab_pt2 = p1.lerp(&p2, t_end);

                current_segment = vec![tab_pt2];
                next_tab_start += interval;
            }

            accumulated_dist += seg_len;
        }

        if !current_segment.is_empty() {
            segments.push(Polyline::new(current_segment, false));
        }

        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Shape};

    #[test]
    fn test_kerf_compensation_outer_and_inner() {
        let mut doc = Document::new("test", 100.0, 100.0);

        // Outer box: 20x20 from (0,0) to (20,20)
        let outer_pts = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(20.0, 0.0),
            Point2D::new(20.0, 20.0),
            Point2D::new(0.0, 20.0),
        ];
        doc.shapes.push(Shape::Path {
            polyline: Polyline::new(outer_pts, true),
            layer_index: 0,
        });

        // Inner hole: 10x10 from (5,5) to (15,15)
        let inner_pts = vec![
            Point2D::new(5.0, 5.0),
            Point2D::new(15.0, 5.0),
            Point2D::new(15.0, 15.0),
            Point2D::new(5.0, 15.0),
        ];
        doc.shapes.push(Shape::Path {
            polyline: Polyline::new(inner_pts, true),
            layer_index: 0,
        });

        let mut opt = CutOptimizer::new();
        opt.inner_cuts_first = true;
        opt.default_kerf = 0.20; // 0.20 mm beam diameter -> 0.10 mm offset

        let job = opt.optimize(&doc);
        assert_eq!(job.paths.len(), 2);

        // Path 0 must be inner cut
        assert!(job.paths[0].is_inner_cut);
        let inner_bounds = job.paths[0].polyline.bounds();
        // Inner hole was 10x10, shrunk by 0.1mm on each side -> 9.80 mm
        assert!((inner_bounds.width() - 9.80).abs() < 1e-4);
        assert!((inner_bounds.height() - 9.80).abs() < 1e-4);
        assert!((inner_bounds.min_x - 5.10).abs() < 1e-4);
        assert!((inner_bounds.min_y - 5.10).abs() < 1e-4);

        // Path 1 must be outer cut
        assert!(!job.paths[1].is_inner_cut);
        let outer_bounds = job.paths[1].polyline.bounds();
        // Outer box was 20x20, expanded by 0.1mm on each side -> 20.20 mm
        assert!((outer_bounds.width() - 20.20).abs() < 1e-4);
        assert!((outer_bounds.height() - 20.20).abs() < 1e-4);
        assert!((outer_bounds.min_x - (-0.10)).abs() < 1e-4);
        assert!((outer_bounds.min_y - (-0.10)).abs() < 1e-4);
    }

    #[test]
    fn test_tool_layers_and_non_output_layers_are_skipped() {
        let mut doc = Document::new("test", 100.0, 100.0);

        // Shape on Layer 0 (normal cut)
        doc.shapes.push(Shape::Rect {
            layer_index: 0,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            corner_radius: 0.0,
        });

        // Shape on Layer 30 (T01 Tool layer, guide frame)
        doc.shapes.push(Shape::Rect {
            layer_index: 30,
            x: -5.0,
            y: -5.0,
            width: 20.0,
            height: 20.0,
            corner_radius: 0.0,
        });

        // Shape on Layer 1 with output disabled
        if let Some(l1) = doc.layers.get_mut(&1) {
            l1.output = false;
        }
        doc.shapes.push(Shape::Rect {
            layer_index: 1,
            x: 2.0,
            y: 2.0,
            width: 5.0,
            height: 5.0,
            corner_radius: 0.0,
        });

        let opt = CutOptimizer::new();
        let job = opt.optimize(&doc);

        // Only Layer 0 should be cut, Layer 30 (Tool) and Layer 1 (Output=false) must be omitted!
        assert_eq!(job.paths.len(), 1);
        assert_eq!(job.paths[0].layer_index, 0);
    }

    #[test]
    fn test_grayscale_power_analog_raster_and_gcode() {
        let mut doc = Document::new("grayscale_test", 100.0, 100.0);
        let layer = doc.get_or_create_layer(1);
        layer.cut_setting.mode = crate::document::CutMode::Image;
        layer.cut_setting.dither_mode = crate::document::DitherMode::GrayscalePower;
        layer.cut_setting.power_min = 10.0;
        layer.cut_setting.power_max = 90.0;
        layer.cut_setting.line_interval = 0.5; // 0.5 mm spacing
        layer.cut_setting.overscan = 2.0;

        // Create an image 10x4 pixels:
        // Left: White (255) -> 0% power
        // Mid: Gray (128) -> ~50% between min(10) and max(90) = ~50%
        // Right: Black (0) -> max(90)% power
        let w = 10usize;
        let h = 4usize;
        let mut pixels = Vec::with_capacity(w * h * 4);
        for _y in 0..h {
            for x in 0..w {
                let v = if x < 2 {
                    255u8 // White background
                } else if x < 6 {
                    128u8 // Mid gray
                } else {
                    0u8   // Full black
                };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }

        doc.shapes.push(crate::document::Shape::Image {
            layer_index: 1,
            x: 10.0,
            y: 10.0,
            width: 10.0,
            height: 2.0,
            image_path: None,
            raw_pixels: Some(pixels),
            raw_width: w as u32,
            raw_height: h as u32,
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
            sharpen: 0.0,
        });

        let opt = CutOptimizer::new();
        let job = opt.optimize(&doc);

        assert!(!job.paths.is_empty(), "Should generate toolpaths for image");
        let first_path = &job.paths[0];
        assert!(first_path.powers.is_some(), "Path should have variable powers");

        let powers = first_path.powers.as_ref().unwrap();
        // Check that powers contain modulated values
        assert!(powers.iter().any(|&p| p > 10.0 && p < 60.0), "Should contain mid-gray power");
        assert!(powers.iter().any(|&p| p >= 85.0), "Should contain high power for black");

        // Verify G-code generation
        let gen = crate::formats::GCodeGenerator::new(crate::formats::GCodeConfig::default());
        let gcode = gen.generate(&doc, &job);

        assert!(gcode.contains("G1 "), "G-code should have G1 feed moves");
        assert!(gcode.contains("S"), "G-code should have modulated S power parameters");
        assert!(gcode.contains("M4") || gcode.contains("M3"), "G-code should enable laser mode");
    }

    #[test]
    fn test_layer_tabs_insertion() {
        let mut doc = Document::new("tabs_test", 100.0, 100.0);
        let mut layer = crate::document::Layer::new(0, "Cut with Tabs", "#FF0000");
        layer.cut_setting.mode = crate::document::CutMode::Line;
        layer.cut_setting.tabs_enabled = true;
        layer.cut_setting.tab_count = 4;
        layer.cut_setting.tab_width = 1.0;
        doc.layers.insert(0, layer);

        // Add a 40x40 square (perimeter = 160 mm)
        doc.shapes.push(crate::document::Shape::Rect {
            layer_index: 0,
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 40.0,
            corner_radius: 0.0,
        });

        let opt = CutOptimizer::new();
        let job = opt.optimize(&doc);

        // With 4 tabs, the closed square should be split into multiple open segments
        assert!(job.paths.len() >= 4, "Should split rectangle into at least 4 segments, got {}", job.paths.len());
        for path in &job.paths {
            assert!(!path.polyline.is_closed, "Segments created by tabs should be open");
        }
    }

    #[test]
    fn test_3d_sliced_relief_optimization_and_gcode() {
        let mut doc = Document::new("relief_gcode_test", 100.0, 100.0);
        let mut layer = crate::document::Layer::new(0, "3D Relief Layer", "#000000");
        layer.cut_setting.mode = crate::document::CutMode::Image;
        layer.cut_setting.dither_mode = crate::document::DitherMode::Sliced3D;
        layer.cut_setting.relief_slices = 3;
        layer.cut_setting.relief_material_thickness = 3.0;
        layer.cut_setting.relief_depth_mm = 1.5;
        layer.cut_setting.relief_z_step = 0.5;
        layer.cut_setting.relief_clearance_z = 2.0;
        layer.cut_setting.relief_alternate_angle = true;
        layer.cut_setting.line_interval = 1.0;
        doc.layers.insert(0, layer);

        let w = 6;
        let h = 6;
        let mut pixels = Vec::new();
        for y in 0..h {
            for _x in 0..w {
                let v = if y < 2 { 255 } else if y < 4 { 128 } else { 0 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }

        doc.shapes.push(crate::document::Shape::Image {
            layer_index: 0,
            x: 10.0,
            y: 10.0,
            width: 10.0,
            height: 10.0,
            image_path: None,
            raw_pixels: Some(pixels),
            raw_width: w as u32,
            raw_height: h as u32,
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
            sharpen: 0.0,
        });

        let opt = CutOptimizer::new();
        let job = opt.optimize(&doc);
        assert!(!job.paths.is_empty(), "Should generate optimized 3D relief paths");

        let gen = crate::formats::GCodeGenerator::new(crate::formats::GCodeConfig::default());
        let gcode = gen.generate(&doc, &job);

        // Verify Z-axis collision prevention and depth commands
        assert!(gcode.contains("G0 Z2.000"), "G-code should retract to clearance Z to prevent collisions");
        assert!(gcode.contains("(3D Relief Z"), "G-code should annotate 3D relief passes");
        assert!(gcode.contains("G0 Z-"), "G-code should plunge to negative Z depths for slices");
    }
}

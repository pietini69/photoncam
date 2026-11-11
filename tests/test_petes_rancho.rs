use photoncam::formats::lbrn2::Lbrn2File;
use photoncam::document::Shape;

#[test]
fn test_petes_rancho_render_and_orientation() {
    let path = "/tmp/petes_rancho/PetesRancho.lbrn2";
    if !std::path::Path::new(path).exists() {
        return;
    }
    let doc = Lbrn2File::load_from_file(path).expect("Failed to load PetesRancho.lbrn2");
    assert_eq!(doc.shapes.len(), 28, "Should load all 28 shapes");

    // Shape 2 and 3 form the letter 'P' in "PETE'S RANCHO"
    if let Shape::Path { polyline, .. } = &doc.shapes[3] {
        let min_y = polyline.points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_y = polyline.points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        
        // Loop of 'P' should be at the top (around Y = 20..38) and stem at bottom (around Y = 50..60)
        let top_points_count = polyline.points.iter().filter(|p| p.y < 35.0).count();
        let bottom_points_count = polyline.points.iter().filter(|p| p.y > 45.0).count();
        assert!(top_points_count > 0, "Top curve of 'P' must be near Y < 35.0");
        assert!(bottom_points_count > 0, "Stem of 'P' must extend to Y > 45.0");
        assert!(min_y < 25.0, "Top of 'P' should be near Y ~ 20.0, got {}", min_y);
        assert!(max_y > 55.0, "Bottom stem of 'P' should be near Y ~ 60.0, got {}", max_y);
    } else {
        panic!("Shape 3 should be a Path");
    }

    // Outer border shapes (shape 26 and 27)
    let b = doc.bounds();
    assert!(b.min_x > 0.0 && b.max_x < 420.0, "Bounds X should fit in ~400mm bed");
    assert!(b.min_y > 0.0 && b.max_y < 80.0, "Bounds Y should fit in ~75mm height");
}

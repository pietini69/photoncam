use photoncam::formats::lbrn2::Lbrn2File;

#[test]
fn test_all_lbrn2_files_orientation() {
    let files = [
        "/tmp/petes_rancho/PetesRancho.lbrn2",
        "/Volumes/Jättitila/Laser/40mm_mitta.lbrn2",
        "/Volumes/Jättitila/Laser/testi.lbrn2",
        "/Volumes/Jättitila/Laser/AirPurifier.lbrn2",
    ];

    for path in files {
        if !std::path::Path::new(path).exists() {
            println!("Skipping non-existent: {}", path);
            continue;
        }
        let doc = Lbrn2File::load_from_file(path).expect(&format!("Failed to load {}", path));
        println!("Loaded {}: {} shapes, bounds: {:?}", path, doc.shapes.len(), doc.bounds());
    }
}

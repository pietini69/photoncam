use eframe::egui;
use crate::generator::{
    generate_box_parts, generate_living_hinge, generate_qr_shapes, generate_variable_text_shapes,
    layout_box_shapes, living_hinge_to_shapes, BoxLidType, HingePattern, QrEcLevel, QrStyle,
    VariableTextMode,
};
use super::LaserBurnApp;
use crate::Shape;

impl LaserBurnApp {
    /// Renders all modal generator windows (Box Maker, Living Hinge, QR Code, Variable Text)
    pub fn show_generators_dialog_windows(&mut self, ctx: &egui::Context) {
        self.show_box_maker_dialog(ctx);
        self.show_living_hinge_dialog(ctx);
        self.show_qr_code_dialog(ctx);
        self.show_variable_text_dialog(ctx);
    }

    // -------------------------------------------------------------
    // 1. BOX & ENCLOSURE MAKER DIALOG
    // -------------------------------------------------------------
    fn show_box_maker_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_box_maker_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let mut is_open = true;
        let mut do_generate = false;

        let (def_pos, def_size) = self.window_pos_size("box_maker_dialog_window", [100.0, 100.0], [460.0, 520.0]);
        let resp = egui::Window::new(tr("Finger Joint Box Designer"))
            .id(egui::Id::new("box_maker_dialog_window"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading(tr("📦 Box & Enclosure Generator"));
                ui.label(
                    egui::RichText::new(tr("Generates finger joint plywood and acrylic boxes with automatic kerf compensation."))
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                egui::Grid::new("box_maker_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        // Outer Dimensions
                        ui.label(egui::RichText::new(tr("Outer Dimensions (W × D × H):")).strong());
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.box_maker_params.width).speed(1.0).range(15.0..=2000.0).suffix(" mm"))
                                .on_hover_text(tr("Width X (mm)"));
                            ui.label("×");
                            ui.add(egui::DragValue::new(&mut self.box_maker_params.depth).speed(1.0).range(15.0..=2000.0).suffix(" mm"))
                                .on_hover_text(tr("Depth Y (mm)"));
                            ui.label("×");
                            ui.add(egui::DragValue::new(&mut self.box_maker_params.height).speed(1.0).range(10.0..=2000.0).suffix(" mm"))
                                .on_hover_text(tr("Height Z (mm)"));
                        });
                        ui.end_row();

                        // Material Thickness
                        ui.label(egui::RichText::new(tr("Material Thickness (T):")).strong());
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.box_maker_params.thickness).speed(0.1).range(0.5..=30.0).suffix(" mm"));
                            for &std_t in &[3.0, 4.0, 6.0] {
                                if ui.small_button(format!("{}mm", std_t)).clicked() {
                                    self.box_maker_params.thickness = std_t;
                                }
                            }
                        });
                        ui.end_row();

                        // Finger Tab Width
                        ui.label(tr("Finger / Tab Width:"));
                        ui.add(egui::DragValue::new(&mut self.box_maker_params.tab_size).speed(0.5).range(3.0..=50.0).suffix(" mm"))
                            .on_hover_text(tr("Target tab finger width (mm)"));
                        ui.end_row();

                        // Kerf Compensation
                        ui.label(tr("Kerf Compensation:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.box_maker_params.kerf).speed(0.01).range(-0.5..=0.5).suffix(" mm"))
                                .on_hover_text(tr("Tab expansion / slot reduction for a snug fit without glue"));
                            if ui.small_button("0.08mm").clicked() {
                                self.box_maker_params.kerf = 0.08;
                            }
                            if ui.small_button("0.12mm").clicked() {
                                self.box_maker_params.kerf = 0.12;
                            }
                        });
                        ui.end_row();

                        // Lid Type
                        ui.label(tr("Lid Type:"));
                        egui::ComboBox::from_id_salt("box_lid_type_combo")
                            .selected_text(match self.box_maker_params.lid_type {
                                BoxLidType::Open => tr("Open box (no lid)"),
                                BoxLidType::FingerJointLid => tr("Finger joint lid (closed box)"),
                                BoxLidType::SlideInLid => tr("Slide-in lid"),
                                BoxLidType::FlatLid => tr("Flat lid"),
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.box_maker_params.lid_type, BoxLidType::Open, tr("Open box (no lid)"));
                                ui.selectable_value(&mut self.box_maker_params.lid_type, BoxLidType::FingerJointLid, tr("Finger joint lid (closed box)"));
                                ui.selectable_value(&mut self.box_maker_params.lid_type, BoxLidType::SlideInLid, tr("Slide-in lid"));
                                ui.selectable_value(&mut self.box_maker_params.lid_type, BoxLidType::FlatLid, tr("Flat lid"));
                            });
                        ui.end_row();

                        // Dividers
                        ui.label(tr("Dividers (partitions):"));
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            ui.add(egui::DragValue::new(&mut self.box_maker_params.dividers_x).range(0..=10));
                            ui.label("Y:");
                            ui.add(egui::DragValue::new(&mut self.box_maker_params.dividers_y).range(0..=10));
                        });
                        ui.end_row();

                        // Parts Layout Spacing
                        ui.label(tr("Parts Layout Spacing:"));
                        ui.add(egui::DragValue::new(&mut self.box_maker_params.layout_margin).speed(0.5).range(1.0..=50.0).suffix(" mm"));
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                // Summary
                let parts = generate_box_parts(&self.box_maker_params);
                ui.label(egui::RichText::new(format!("{}: {} {}", tr("Total parts to generate:"), parts.len(), tr("pcs"))).strong().color(egui::Color32::from_rgb(0, 220, 130)));
                for (p_idx, p) in parts.iter().enumerate() {
                    ui.label(format!("  {}. {} ({:.0} × {:.0} mm)", p_idx + 1, p.name, p.width, p.height));
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let btn = egui::Button::new(egui::RichText::new(tr("Generate Box on Canvas")).strong().color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(34, 139, 34));
                    if ui.add_sized([180.0, 32.0], btn).clicked() {
                        do_generate = true;
                    }

                    if ui.button(tr("Close")).clicked() {
                        self.show_box_maker_window = false;
                    }
                });
            });

        if do_generate {
            self.push_undo();
            let parts = generate_box_parts(&self.box_maker_params);
            let shapes = layout_box_shapes(&parts, 20.0, 20.0, self.box_maker_params.layout_margin, self.selected_layer);

            let start_idx = self.doc.shapes.len();
            let count = shapes.len();
            for s in shapes {
                self.doc.add_shape(s);
            }
            self.selected_shape_indices = (start_idx..start_idx + count).collect();
            self.status_msg = format!("{} ({} {})", tr("Box created on canvas"), count, tr("parts"));
            self.show_box_maker_window = false;
        }

        if !is_open {
            self.show_box_maker_window = false;
        }

        if let Some(r) = resp {
            self.record_window_rect("box_maker_dialog_window", r.response.rect);
        }
    }

    // -------------------------------------------------------------
    // 2. LIVING HINGE DIALOG
    // -------------------------------------------------------------
    fn show_living_hinge_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_living_hinge_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let mut is_open = true;
        let mut do_generate = false;

        let (def_pos, def_size) = self.window_pos_size("living_hinge_dialog_window", [120.0, 120.0], [440.0, 480.0]);
        let resp = egui::Window::new(tr("Living Hinge Generator"))
            .id(egui::Id::new("living_hinge_dialog_window"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading(tr("🌀 Living Hinge Laser Patterns"));
                ui.label(
                    egui::RichText::new(tr("Transforms rigid plywood and acrylic into flexible bends using patterned laser cuts."))
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                // Option to adapt to selected rectangle on canvas
                let has_rect_selection = self.selected_shape_indices.iter().find_map(|&idx| {
                    if idx < self.doc.shapes.len() {
                        if let Shape::Rect { x, y, width, height, .. } = self.doc.shapes[idx] {
                            return Some((x, y, width, height));
                        }
                    }
                    None
                });

                if let Some((rx, ry, rw, rh)) = has_rect_selection {
                    if ui.button(format!("{} ({:.1} × {:.1} mm)", tr("Use dimensions of selected rectangle"), rw, rh)).clicked() {
                        self.living_hinge_params.x = rx;
                        self.living_hinge_params.y = ry;
                        self.living_hinge_params.width = rw;
                        self.living_hinge_params.height = rh;
                    }
                    ui.add_space(4.0);
                }

                egui::Grid::new("living_hinge_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        // Bend Area Dimensions
                        ui.label(tr("Bend Area Dimensions (W × H):"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.living_hinge_params.width).speed(1.0).range(5.0..=1000.0).suffix(" mm"))
                                .on_hover_text(tr("Bend width (direction of bend)"));
                            ui.label("×");
                            ui.add(egui::DragValue::new(&mut self.living_hinge_params.height).speed(1.0).range(5.0..=1000.0).suffix(" mm"))
                                .on_hover_text(tr("Hinge length"));
                        });
                        ui.end_row();

                        // Canvas Position
                        ui.label(tr("Canvas Position (X, Y):"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.living_hinge_params.x).speed(1.0).suffix(" mm"));
                            ui.add(egui::DragValue::new(&mut self.living_hinge_params.y).speed(1.0).suffix(" mm"));
                        });
                        ui.end_row();

                        // Hinge Pattern
                        ui.label(tr("Hinge Cut Pattern:"));
                        egui::ComboBox::from_id_salt("hinge_pattern_combo")
                            .selected_text(match self.living_hinge_params.pattern {
                                HingePattern::StraightLattice => tr("Straight alternating lattice"),
                                HingePattern::WavyLattice => tr("Wavy smooth lattice"),
                                HingePattern::CrossLattice => tr("Cross lattice"),
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.living_hinge_params.pattern, HingePattern::StraightLattice, tr("Straight alternating lattice"));
                                ui.selectable_value(&mut self.living_hinge_params.pattern, HingePattern::WavyLattice, tr("Wavy smooth lattice"));
                                ui.selectable_value(&mut self.living_hinge_params.pattern, HingePattern::CrossLattice, tr("Cross lattice"));
                            });
                        ui.end_row();

                        // Line Spacing (Pitch)
                        ui.label(tr("Cut Pitch (Line Spacing):"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.living_hinge_params.pitch).speed(0.1).range(1.0..=15.0).suffix(" mm"))
                                .on_hover_text("Viiltojen vaakaväli (3mm vanerille suositus 2.5 - 3.5 mm)");
                            if ui.small_button("2.5mm").clicked() { self.living_hinge_params.pitch = 2.5; }
                            if ui.small_button("3.0mm").clicked() { self.living_hinge_params.pitch = 3.0; }
                        });
                        ui.end_row();

                        // Cut Length
                        ui.label(tr("Cut Line Segment Length:"));
                        ui.add(egui::DragValue::new(&mut self.living_hinge_params.cut_length).speed(0.5).range(5.0..=100.0).suffix(" mm"))
                            .on_hover_text("Yksittäisen laserleikkauksen pituus (tyypillisesti 15 - 25 mm)");
                        ui.end_row();

                        // Bridge Gap Width
                        ui.label(tr("Bridge / Gap Width:"));
                        ui.add(egui::DragValue::new(&mut self.living_hinge_params.gap).speed(0.1).range(0.5..=15.0).suffix(" mm"))
                            .on_hover_text("Leikkaamaton väli viiltojen välissä, joka pitää puun koossa (tyypillisesti 1.5 - 2.5 mm)");
                        ui.end_row();

                        // Border Margin
                        ui.label(tr("Border Margins:"));
                        ui.add(egui::DragValue::new(&mut self.living_hinge_params.border).speed(0.5).range(0.0..=50.0).suffix(" mm"))
                            .on_hover_text("Eheä leikkaamaton alue ylä- ja alareunassa");
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                let preview_lines = generate_living_hinge(&self.living_hinge_params);
                ui.label(egui::RichText::new(format!("{}: {} {}", tr("Total cut lines to generate:"), preview_lines.len(), tr("pcs"))).strong().color(egui::Color32::from_rgb(0, 220, 130)));

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let btn = egui::Button::new(egui::RichText::new(tr("Generate Living Hinge")).strong().color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(34, 139, 34));
                    if ui.add_sized([180.0, 32.0], btn).clicked() {
                        do_generate = true;
                    }

                    if ui.button(tr("Close")).clicked() {
                        self.show_living_hinge_window = false;
                    }
                });
            });

        if do_generate {
            self.push_undo();
            let cuts = generate_living_hinge(&self.living_hinge_params);
            let count = cuts.len();
            let shapes = living_hinge_to_shapes(cuts, self.selected_layer);

            let start_idx = self.doc.shapes.len();
            for s in shapes {
                self.doc.add_shape(s);
            }
            self.selected_shape_indices = (start_idx..start_idx + count).collect();
            self.status_msg = format!("{} ({} {})", tr("Living hinge created"), count, tr("cuts"));
            self.show_living_hinge_window = false;
        }

        if !is_open {
            self.show_living_hinge_window = false;
        }

        if let Some(r) = resp {
            self.record_window_rect("living_hinge_dialog_window", r.response.rect);
        }
    }

    // -------------------------------------------------------------
    // 3. VECTOR QR CODE GENERATOR DIALOG
    // -------------------------------------------------------------
    fn show_qr_code_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_qr_code_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let mut is_open = true;
        let mut do_generate = false;

        let (def_pos, def_size) = self.window_pos_size("qr_code_dialog_window", [140.0, 140.0], [420.0, 440.0]);
        let resp = egui::Window::new(tr("Vector QR Code Generator"))
            .id(egui::Id::new("qr_code_dialog_window"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading(tr("📱 Vector QR Code"));
                ui.label(
                    egui::RichText::new(tr("Generate sharp laser-cuttable or engravable QR codes."))
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                egui::Grid::new("qr_code_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        // Content
                        ui.label(egui::RichText::new(tr("Text or URL content:")).strong());
                        ui.add(egui::TextEdit::singleline(&mut self.qr_code_params.content).desired_width(240.0));
                        ui.end_row();

                        // Size
                        ui.label(tr("QR Code Size:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.qr_code_params.size_mm).speed(1.0).range(10.0..=500.0).suffix(" mm"));
                            for &std_sz in &[30.0, 40.0, 50.0, 80.0] {
                                if ui.small_button(format!("{:.0}", std_sz)).clicked() {
                                    self.qr_code_params.size_mm = std_sz;
                                }
                            }
                        });
                        ui.end_row();

                        // Position
                        ui.label(tr("Canvas Position (X, Y):"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.qr_code_params.x).speed(1.0).suffix(" mm"));
                            ui.add(egui::DragValue::new(&mut self.qr_code_params.y).speed(1.0).suffix(" mm"));
                        });
                        ui.end_row();

                        // Style
                        ui.label(tr("Style:"));
                        egui::ComboBox::from_id_salt("qr_style_combo")
                            .selected_text(match self.qr_code_params.style {
                                QrStyle::SharpSquares => tr("Sharp Squares"),
                                QrStyle::RoundedSquares => tr("Rounded Squares"),
                                QrStyle::Dots => tr("Dots / Circles"),
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.qr_code_params.style, QrStyle::SharpSquares, tr("Sharp Squares"));
                                ui.selectable_value(&mut self.qr_code_params.style, QrStyle::RoundedSquares, tr("Rounded Squares"));
                                ui.selectable_value(&mut self.qr_code_params.style, QrStyle::Dots, tr("Dots / Circles"));
                            });
                        ui.end_row();

                        // Error Correction Level
                        ui.label(tr("Error Correction Level:"));
                        egui::ComboBox::from_id_salt("qr_ecc_combo")
                            .selected_text(match self.qr_code_params.ec_level {
                                QrEcLevel::Low => tr("Low (~7%)"),
                                QrEcLevel::Medium => tr("Medium (~15%)"),
                                QrEcLevel::Quartile => tr("Quartile (~25%)"),
                                QrEcLevel::High => tr("High (~30%)"),
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.qr_code_params.ec_level, QrEcLevel::Low, tr("Low (~7%)"));
                                ui.selectable_value(&mut self.qr_code_params.ec_level, QrEcLevel::Medium, tr("Medium (~15%)"));
                                ui.selectable_value(&mut self.qr_code_params.ec_level, QrEcLevel::Quartile, tr("Quartile (~25%)"));
                                ui.selectable_value(&mut self.qr_code_params.ec_level, QrEcLevel::High, tr("High (~30%)"));
                            });
                        ui.end_row();

                        // Margin (Quiet zone)
                        ui.label(tr("Quiet Zone Margin:"));
                        ui.add(egui::DragValue::new(&mut self.qr_code_params.quiet_zone).range(0..=8).suffix(tr(" modules")));
                        ui.end_row();

                        // Invert
                        ui.label(tr("Invert:"));
                        ui.checkbox(&mut self.qr_code_params.invert, tr("Create background plaque (invert)"));
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    let btn = egui::Button::new(egui::RichText::new(tr("Generate QR Code")).strong().color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(34, 139, 34));
                    if ui.add_sized([180.0, 32.0], btn).clicked() {
                        do_generate = true;
                    }

                    if ui.button(tr("Close")).clicked() {
                        self.show_qr_code_window = false;
                    }
                });
            });

        if do_generate {
            match generate_qr_shapes(&self.qr_code_params, self.selected_layer) {
                Ok(shapes) => {
                    self.push_undo();
                    let start_idx = self.doc.shapes.len();
                    let count = shapes.len();
                    for s in shapes {
                        self.doc.add_shape(s);
                    }
                    self.selected_shape_indices = (start_idx..start_idx + count).collect();
                    self.status_msg = format!("{} ({:.0}x{:.0} mm)", tr("QR Code created on canvas"), self.qr_code_params.size_mm, self.qr_code_params.size_mm);
                    self.show_qr_code_window = false;
                }
                Err(err) => {
                    self.status_msg = format!("{}: {}", tr("QR Code error"), err);
                }
            }
        }

        if !is_open {
            self.show_qr_code_window = false;
        }

        if let Some(r) = resp {
            self.record_window_rect("qr_code_dialog_window", r.response.rect);
        }
    }

    // -------------------------------------------------------------
    // 4. VARIABLE TEXT & BATCH PLAQUES DIALOG
    // -------------------------------------------------------------
    fn show_variable_text_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_variable_text_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let mut is_open = true;
        let mut do_generate = false;

        let (def_pos, def_size) = self.window_pos_size("variable_text_dialog_window", [160.0, 160.0], [500.0, 560.0]);
        let resp = egui::Window::new(tr("Batch Variable Text & Serial Numbers"))
            .id(egui::Id::new("variable_text_dialog_window"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading(tr("🏷️ Batch Serial Numbers & Name Badges"));
                ui.label(
                    egui::RichText::new(tr("Batch-generate sequential serial numbers, asset tags, or badges from CSV lists."))
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                // Mode Selection
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.variable_text_params.mode, VariableTextMode::Sequential, tr("Sequential Numbers"));
                    ui.selectable_value(&mut self.variable_text_params.mode, VariableTextMode::CustomList, tr("Custom Text List"));
                    ui.selectable_value(&mut self.variable_text_params.mode, VariableTextMode::CsvTemplate, tr("CSV Template"));
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                match self.variable_text_params.mode {
                    VariableTextMode::Sequential => {
                        egui::Grid::new("var_seq_grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label(tr("Prefix:"));
                                ui.text_edit_singleline(&mut self.variable_text_params.prefix);
                                ui.end_row();

                                ui.label(tr("Number Range (Start / End / Step):"));
                                ui.horizontal(|ui| {
                                    ui.add(egui::DragValue::new(&mut self.variable_text_params.start_num));
                                    ui.label("–");
                                    ui.add(egui::DragValue::new(&mut self.variable_text_params.end_num));
                                    ui.label(tr("Step:"));
                                    ui.add(egui::DragValue::new(&mut self.variable_text_params.step).range(1..=100));
                                });
                                ui.end_row();

                                ui.label(tr("Digit Padding:"));
                                ui.horizontal(|ui| {
                                    ui.add(egui::DragValue::new(&mut self.variable_text_params.padding).range(1..=8).suffix(tr(" digits")));
                                    ui.label(tr("Suffix:"));
                                    ui.text_edit_singleline(&mut self.variable_text_params.suffix);
                                });
                                ui.end_row();
                            });
                    }

                    VariableTextMode::CustomList => {
                        ui.label(egui::RichText::new(tr("Enter names or text (one per line):")).strong());
                        ui.add(
                            egui::TextEdit::multiline(&mut self.variable_text_params.raw_data)
                                .desired_rows(6)
                                .desired_width(ui.available_width()),
                        );
                    }

                    VariableTextMode::CsvTemplate => {
                        ui.label(tr("CSV rows (comma or semicolon delimited):"));
                        ui.add(
                            egui::TextEdit::multiline(&mut self.variable_text_params.raw_data)
                                .desired_rows(5)
                                .desired_width(ui.available_width()),
                        );
                        ui.horizontal(|ui| {
                            ui.label(tr("Template:"));
                            ui.text_edit_singleline(&mut self.variable_text_params.csv_template);
                            ui.label(egui::RichText::new(tr("(Use {1}, {2}...)")).small().weak());
                        });
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // Typography & Layout
                egui::Grid::new("var_layout_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(tr("Font Height:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.variable_text_params.font_size_mm).speed(0.5).range(2.0..=50.0).suffix(" mm"));
                            ui.checkbox(&mut self.variable_text_params.use_single_line_font, tr("Single-line stroke font"));
                        });
                        ui.end_row();

                        ui.label(tr("Columns / Spacing:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.variable_text_params.columns).range(1..=20).suffix(tr(" cols")));
                            ui.label(tr("Spacing:"));
                            ui.add(egui::DragValue::new(&mut self.variable_text_params.spacing_x).speed(0.5).range(1.0..=50.0).suffix(" mm"));
                        });
                        ui.end_row();

                        // Plaque badge
                        ui.label(tr("Badge Plaque (Outline):"));
                        ui.checkbox(&mut self.variable_text_params.add_badge, tr("Create cuttable badge border around text"));
                        ui.end_row();

                        if self.variable_text_params.add_badge {
                            ui.label(tr("Badge Dimensions (W × H):"));
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut self.variable_text_params.badge_width).speed(1.0).range(10.0..=300.0).suffix(" mm"));
                                ui.label("×");
                                ui.add(egui::DragValue::new(&mut self.variable_text_params.badge_height).speed(1.0).range(10.0..=300.0).suffix(" mm"));
                                ui.label(tr("Corner Radius:"));
                                ui.add(egui::DragValue::new(&mut self.variable_text_params.badge_corner_radius).speed(0.5).range(0.0..=20.0).suffix(" mm"));
                            });
                            ui.end_row();

                            ui.label(tr("Mounting Hole:"));
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.variable_text_params.add_mounting_hole, tr("Add left mounting / lanyard hole"));
                                if self.variable_text_params.add_mounting_hole {
                                    ui.add(egui::DragValue::new(&mut self.variable_text_params.hole_diameter).speed(0.1).range(1.0..=15.0).suffix(" mm"));
                                }
                            });
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                let preview_strings = crate::generator::resolve_variable_strings(&self.variable_text_params);
                ui.label(egui::RichText::new(format!("{}: {} {}", tr("Total badges to generate:"), preview_strings.len(), tr("pcs"))).strong().color(egui::Color32::from_rgb(0, 220, 130)));

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let btn = egui::Button::new(egui::RichText::new(tr("Generate Batch Shapes")).strong().color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(34, 139, 34));
                    if ui.add_sized([200.0, 32.0], btn).clicked() {
                        do_generate = true;
                    }

                    if ui.button(tr("Close")).clicked() {
                        self.show_variable_text_window = false;
                    }
                });
            });

        if do_generate {
            self.push_undo();
            let cut_layer = if self.doc.layers.len() > 1 { 1 } else { 0 };
            let shapes = generate_variable_text_shapes(&self.variable_text_params, self.selected_layer, cut_layer);
            let count = shapes.len();

            let start_idx = self.doc.shapes.len();
            for s in shapes {
                self.doc.add_shape(s);
            }
            self.selected_shape_indices = (start_idx..start_idx + count).collect();
            self.status_msg = format!("{} ({} {})", tr("Batch badges generated on canvas"), count, tr("items"));
            self.show_variable_text_window = false;
        }

        if !is_open {
            self.show_variable_text_window = false;
        }

        if let Some(r) = resp {
            self.record_window_rect("variable_text_dialog_window", r.response.rect);
        }
    }
}

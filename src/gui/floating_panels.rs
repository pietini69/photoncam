use eframe::egui;
use super::{AlignMode, DistributeMode, LaserBurnApp, LengthUnit};
use crate::Shape;
use crate::document::{CutMode, DitherMode, Document};
use crate::geometry::Point2D;

impl LaserBurnApp {
    /// Renders the unified floating "Properties & Layers" inspector window (Transform + Cut Settings)
    pub fn render_properties_window(&mut self, ctx: &egui::Context) {
        if !self.show_properties_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let mut is_open = true;
        let screen_rect = ctx.screen_rect();
        let (def_pos, def_size) = self.window_pos_size("floating_properties_window", [screen_rect.width() - 390.0, 50.0], [370.0, 680.0]);

        let resp = egui::Window::new(tr("Properties & Layers"))
            .id(egui::Id::new("floating_properties_window"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .min_width(320.0)
            .min_height(200.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // =============================================================
                        // 1. TRANSFORM & SIZE INSPECTOR
                        // =============================================================
                        egui::CollapsingHeader::new(egui::RichText::new(tr("Transform & Size")).strong().size(13.0))
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (lbl, other) = match self.length_unit {
                                        LengthUnit::Millimeters => ("mm", LengthUnit::Inches),
                                        LengthUnit::Inches => ("in", LengthUnit::Millimeters),
                                    };
                                    if ui.small_button(format!("Yksikkö: {}", lbl))
                                        .on_hover_text(format!("Click to toggle unit between mm and inches (currently {})", self.length_unit.label()))
                                        .clicked()
                                    {
                                        self.length_unit = other;
                                    }
                                });
                                ui.separator();

                                if let Some(bounds) = self.selection_bounds() {
                                    let is_inches = self.length_unit == LengthUnit::Inches;
                                    let count = self.selected_shape_indices.len();
                                    let any_locked = self.selected_shape_indices.iter().any(|&idx| self.is_shape_locked(idx));
                                    ui.horizontal(|ui| {
                                        let label = if count == 1 {
                                            let idx = self.selected_shape_indices[0];
                                            let is_locked = self.is_shape_locked(idx);
                                            let locked_str = if is_locked { format!(" [{}]", tr("Locked")) } else { "".to_string() };
                                            format!("{} #{}{} {}", tr("Shape"), idx + 1, locked_str, tr("Selected"))
                                        } else {
                                            let locked_str = if any_locked { format!(" [{}]", tr("Locked")) } else { "".to_string() };
                                            format!("{} {}{}", count, tr("Shapes Selected"), locked_str)
                                        };
                                        ui.label(egui::RichText::new(label).strong().color(egui::Color32::from_rgb(255, 140, 0)));
                                        if ui.button(tr("Deselect")).clicked() {
                                            self.selected_shape_indices.clear();
                                        }
                                    });

                                    // Position controls (supports both mm and inches)
                                    let mut cur_x_disp = self.length_unit.to_display(bounds.min_x);
                                    let mut cur_y_disp = self.length_unit.to_display(bounds.min_y);
                                    let step_pos = if is_inches { 0.05 } else { 1.0 };
                                    let suf = self.length_unit.suffix();

                                    ui.horizontal(|ui| {
                                        ui.label(if is_inches { tr("X (in):") } else { tr("X (mm):") });
                                        if ui.add(egui::DragValue::new(&mut cur_x_disp).speed(step_pos).range(-200.0..=5000.0).suffix(suf)).changed() {
                                            let new_x = self.length_unit.from_display(cur_x_disp);
                                            let dx = new_x - bounds.min_x;
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.is_shape_locked(idx) {
                                                    self.doc.shapes[idx].translate(dx, 0.0);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                        }
                                        ui.label(if is_inches { tr("Y (in):") } else { tr("Y (mm):") });
                                        if ui.add(egui::DragValue::new(&mut cur_y_disp).speed(step_pos).range(-200.0..=5000.0).suffix(suf)).changed() {
                                            let new_y = self.length_unit.from_display(cur_y_disp);
                                            let dy = new_y - bounds.min_y;
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.is_shape_locked(idx) {
                                                    self.doc.shapes[idx].translate(0.0, dy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                        }
                                    });

                                    // Size controls (supports both mm and inches)
                                    let mut cur_w_disp = self.length_unit.to_display(bounds.width());
                                    let mut cur_h_disp = self.length_unit.to_display(bounds.height());
                                    let step_sz = if is_inches { 0.025 } else { 0.5 };

                                    ui.horizontal(|ui| {
                                        ui.label(if is_inches { tr("Width (in):") } else { tr("Width (mm):") });
                                        if ui.add(egui::DragValue::new(&mut cur_w_disp).speed(step_sz).range(0.01..=5000.0).suffix(suf)).changed() {
                                            let new_w = self.length_unit.from_display(cur_w_disp);
                                            let new_h = if self.lock_aspect_ratio && bounds.width() > 1e-4 {
                                                new_w * (bounds.height() / bounds.width())
                                            } else {
                                                bounds.height()
                                            };
                                            let sx = new_w / bounds.width().max(1e-4);
                                            let sy = new_h / bounds.height().max(1e-4);
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.is_shape_locked(idx) {
                                                    self.doc.shapes[idx].scale(Point2D::new(bounds.min_x, bounds.min_y), sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                        }

                                        ui.label(if is_inches { tr("Height (in):") } else { tr("Height (mm):") });
                                        if ui.add(egui::DragValue::new(&mut cur_h_disp).speed(step_sz).range(0.01..=5000.0).suffix(suf)).changed() {
                                            let new_h = self.length_unit.from_display(cur_h_disp);
                                            let new_w = if self.lock_aspect_ratio && bounds.height() > 1e-4 {
                                                new_h * (bounds.width() / bounds.height())
                                            } else {
                                                bounds.width()
                                            };
                                            let sx = new_w / bounds.width().max(1e-4);
                                            let sy = new_h / bounds.height().max(1e-4);
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.is_shape_locked(idx) {
                                                    self.doc.shapes[idx].scale(Point2D::new(bounds.min_x, bounds.min_y), sx, sy);
                                                }
                                            }
                                            self.doc_version = self.doc_version.wrapping_add(1);
                                        }
                                    });

                                    ui.checkbox(&mut self.lock_aspect_ratio, tr("Lock Aspect Ratio"));

                                    // Align & Distribute Quick Toolbar
                                    ui.add_space(4.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(tr("Align:"));
                                        if ui.button(tr("Bed (P)")).on_hover_text(tr("Center to Bed (P)")).clicked() {
                                            self.align_selection(AlignMode::CenterToBed);
                                        }
                                        if ui.button("←").on_hover_text(tr("Align Left")).clicked() {
                                            self.align_selection(AlignMode::Left);
                                        }
                                        if ui.button("↔").on_hover_text(tr("Align Center Horizontal")).clicked() {
                                            self.align_selection(AlignMode::CenterH);
                                        }
                                        if ui.button("→").on_hover_text(tr("Align Right")).clicked() {
                                            self.align_selection(AlignMode::Right);
                                        }
                                        if ui.button("↑").on_hover_text(tr("Align Top")).clicked() {
                                            self.align_selection(AlignMode::Top);
                                        }
                                        if ui.button("↕").on_hover_text(tr("Align Center Vertical")).clicked() {
                                            self.align_selection(AlignMode::CenterV);
                                        }
                                        if ui.button("↓").on_hover_text(tr("Align Bottom")).clicked() {
                                            self.align_selection(AlignMode::Bottom);
                                        }
                                        if ui.button("|←→|").on_hover_text(tr("Distribute Horizontally")).clicked() {
                                            self.distribute_selection(DistributeMode::Horizontal);
                                        }
                                        if ui.button("|↑↓|").on_hover_text(tr("Distribute Vertically")).clicked() {
                                            self.distribute_selection(DistributeMode::Vertical);
                                        }
                                        if ui.button(tr("Nest")).on_hover_text(tr("Auto-Nest (Material Packing)")).clicked() {
                                            self.show_nesting_window = true;
                                        }
                                    });
                                    ui.add_space(4.0);

                                    // Nudge Buttons (also mention keyboard arrow keys!)
                                    ui.horizontal(|ui| {
                                        ui.label(tr("Nudge (5mm):"));
                                        if ui.button("X-").clicked() {
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].translate(-5.0, 0.0);
                                                }
                                            }
                                        }
                                        if ui.button("X+").clicked() {
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].translate(5.0, 0.0);
                                                }
                                            }
                                        }
                                        if ui.button("Y-").clicked() {
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].translate(0.0, -5.0);
                                                }
                                            }
                                        }
                                        if ui.button("Y+").clicked() {
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].translate(0.0, 5.0);
                                                }
                                            }
                                        }
                                    });

                                    ui.label(egui::RichText::new(tr("Tip: You can nudge directly with arrow keys (Shift = 10mm, Alt = 0.1mm)")).italics().size(11.0).color(egui::Color32::from_rgb(100, 160, 255)));

                                    // Quick Scaling and Rotation
                                    ui.horizontal(|ui| {
                                        ui.label(tr("Scale:"));
                                        let c = bounds.center();
                                        if ui.button("+10%").clicked() {
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(c, 1.10, 1.10);
                                                }
                                            }
                                        }
                                        if ui.button("-10%").clicked() {
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(c, 0.90, 0.90);
                                                }
                                            }
                                        }
                                        if ui.button("2x").clicked() {
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(c, 2.0, 2.0);
                                                }
                                            }
                                        }
                                        if ui.button("0.5x").clicked() {
                                            for &idx in &self.selected_shape_indices {
                                                if idx < self.doc.shapes.len() && !self.doc.shapes[idx].is_locked() {
                                                    self.doc.shapes[idx].scale(c, 0.5, 0.5);
                                                }
                                            }
                                        }
                                    });

                                    // Rotation controls
                                    ui.horizontal(|ui| {
                                        ui.label(tr("Rotate (fine):"));
                                        if ui.button("-1°").on_hover_text("Rotate -1° counter-clockwise (Keys: Q, Comma ,, or Option+8)").clicked() {
                                            self.rotate_selected(-1.0);
                                        }
                                        if ui.button("+1°").on_hover_text("Rotate +1° clockwise (Keys: E, Period ., or Option+9)").clicked() {
                                            self.rotate_selected(1.0);
                                        }
                                        if ui.button("-5°").on_hover_text("Rotate -5° counter-clockwise (Shift+Q, Shift+Comma)").clicked() {
                                            self.rotate_selected(-5.0);
                                        }
                                        if ui.button("+5°").on_hover_text("Rotate +5° clockwise (Shift+E, Shift+Period)").clicked() {
                                            self.rotate_selected(5.0);
                                        }
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label(tr("Rotate (coarse):"));
                                        if ui.button("-15°").on_hover_text("Rotate -15° counter-clockwise").clicked() {
                                            self.rotate_selected(-15.0);
                                        }
                                        if ui.button("+15°").on_hover_text("Rotate +15° clockwise").clicked() {
                                            self.rotate_selected(15.0);
                                        }
                                        if ui.button("-45°").on_hover_text("Rotate -45° counter-clockwise").clicked() {
                                            self.rotate_selected(-45.0);
                                        }
                                        if ui.button("+45°").on_hover_text("Rotate +45° clockwise").clicked() {
                                            self.rotate_selected(45.0);
                                        }
                                        if ui.button("-90°").on_hover_text("Rotate -90° counter-clockwise").clicked() {
                                            self.rotate_selected(-90.0);
                                        }
                                        if ui.button("+90°").on_hover_text("Rotate +90° clockwise").clicked() {
                                            self.rotate_selected(90.0);
                                        }
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label(tr("Angle Step:"));
                                        ui.add(
                                            egui::DragValue::new(&mut self.custom_rot_angle)
                                                .speed(0.1)
                                                .range(0.01..=360.0)
                                                .suffix("°")
                                        );
                                        if ui.button("- Step").on_hover_text("Rotate counter-clockwise by step angle").clicked() {
                                            self.rotate_selected(-self.custom_rot_angle);
                                        }
                                        if ui.button("+ Step").on_hover_text("Rotate clockwise by step angle").clicked() {
                                            self.rotate_selected(self.custom_rot_angle);
                                        }
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label(tr("Flip:"));
                                        if ui.button(tr("Flip H")).on_hover_text("Flip selected shapes horizontally").clicked() {
                                            self.flip_selected_h();
                                        }
                                        if ui.button(tr("Flip V")).on_hover_text("Flip selected shapes vertically").clicked() {
                                            self.flip_selected_v();
                                        }
                                    });

                                    // Layer Assignment
                                    if count >= 1 {
                                        let first_idx = self.selected_shape_indices[0];
                                        let cur_layer = self.doc.shapes[first_idx].layer_index();
                                        ui.horizontal(|ui| {
                                            ui.label(tr("Layer:"));
                                            let cur_label = self.doc.layers.get(&cur_layer).map(|l| l.display_label()).unwrap_or_else(|| format!("[L{:02}]", cur_layer));
                                            let mut new_target_layer = None;
                                            egui::ComboBox::from_id_salt("prop_shape_layer_combo")
                                                .selected_text(cur_label)
                                                .show_ui(ui, |ui| {
                                                    for (&layer_idx, layer) in &self.doc.layers {
                                                        if ui.selectable_label(cur_layer == layer_idx, layer.display_label()).clicked() {
                                                            new_target_layer = Some(layer_idx);
                                                        }
                                                    }
                                                });
                                            if let Some(layer_idx) = new_target_layer {
                                                self.push_undo();
                                                for &s_idx in &self.selected_shape_indices {
                                                    if s_idx < self.doc.shapes.len() {
                                                        self.doc.shapes[s_idx].set_layer_index(layer_idx);
                                                    }
                                                }
                                                self.selected_layer = layer_idx;
                                                self.image_textures.clear();
                                            }
                                        });

                                        // Image Preprocessing Adjustments
                                        let mut open_trace = false;
                                        if first_idx < self.doc.shapes.len() {
                                            if let Shape::Image {
                                                ref mut brightness,
                                                ref mut contrast,
                                                ref mut gamma,
                                                ref mut sharpen,
                                                ..
                                            } = self.doc.shapes[first_idx]
                                            {
                                                ui.add_space(6.0);
                                                ui.group(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(egui::RichText::new(tr("Image Adjustments (Preprocess)")).strong().color(egui::Color32::from_rgb(100, 210, 255)));
                                                        if ui.small_button(tr("Reset")).on_hover_text("Reset Brightness, Contrast, Gamma, and Sharpen to neutral").clicked() {
                                                            *brightness = 0.0;
                                                            *contrast = 0.0;
                                                            *gamma = 1.0;
                                                            *sharpen = 0.0;
                                                            self.image_textures.remove(&first_idx);
                                                            self.doc_version = self.doc_version.wrapping_add(1);
                                                        }
                                                    });

                                                    let mut img_changed = false;
                                                    egui::Grid::new("prop_image_preprocess_grid")
                                                        .num_columns(2)
                                                        .spacing([8.0, 4.0])
                                                        .show(ui, |ui| {
                                                            ui.label(tr("Brightness:"));
                                                            if ui.add(egui::Slider::new(brightness, -100.0..=100.0).suffix("%")).changed() {
                                                                img_changed = true;
                                                            }
                                                            ui.end_row();

                                                            ui.label(tr("Contrast:"));
                                                            if ui.add(egui::Slider::new(contrast, -100.0..=100.0).suffix("%")).changed() {
                                                                img_changed = true;
                                                            }
                                                            ui.end_row();

                                                            ui.label(tr("Gamma:"));
                                                            if ui.add(egui::Slider::new(gamma, 0.20..=3.00)).changed() {
                                                                img_changed = true;
                                                            }
                                                            ui.end_row();

                                                            ui.label(tr("Sharpen:"));
                                                            if ui.add(egui::Slider::new(sharpen, 0.0..=100.0).suffix("%")).changed() {
                                                                img_changed = true;
                                                            }
                                                            ui.end_row();
                                                        });

                                                    ui.add_space(4.0);
                                                    let trace_btn = egui::Button::new(
                                                        egui::RichText::new(tr(" Trace Image to Vectors... ")).strong().color(egui::Color32::WHITE),
                                                    ).fill(egui::Color32::from_rgb(0, 140, 255));
                                                    if ui.add(trace_btn).on_hover_text("Vectorize this bitmap graphic into cutting or engraving paths").clicked() {
                                                        open_trace = true;
                                                    }

                                                    if img_changed {
                                                        self.image_textures.remove(&first_idx);
                                                        self.doc_version = self.doc_version.wrapping_add(1);
                                                    }
                                                });
                                            }
                                        }
                                        if open_trace {
                                            self.open_trace_dialog();
                                        }
                                    }
                                } else {
                                    ui.label(egui::RichText::new(tr("No shape selected.")).italics().color(egui::Color32::GRAY));
                                    ui.label(tr("Click directly INSIDE any shape or click on the Objects List below."));
                                    ui.add_space(4.0);
                                    if ui.button(tr("Open Objects List...")).on_hover_text("Avaa objektilista valitaksesi piirroksen kappaleita luettelosta").clicked() {
                                        self.show_objects_window = true;
                                    }
                                }
                            });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // =============================================================
                        // 2. CUT & LAYER SETTINGS
                        // =============================================================
                        let mut layer_to_remove: Option<usize> = None;
                        let can_remove_layer = self.doc.layers.len() > 1;

                        egui::CollapsingHeader::new(egui::RichText::new(tr("Cut & Layer Settings")).strong().size(13.0))
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.add_space(2.0);

                                ui.horizontal_wrapped(|ui| {
                                    if ui.button(tr("+ Layer")).on_hover_text("Add a new color/cut layer").clicked() {
                                        for idx in 0..30 {
                                            if !self.doc.layers.contains_key(&idx) {
                                                self.doc.get_or_create_layer(idx);
                                                self.selected_layer = idx;
                                                break;
                                            }
                                        }
                                    }
                                    if ui.button(tr("+ Tool")).on_hover_text("Add a guide/tool layer (T01, T02...)").clicked() {
                                        for idx in 30..33 {
                                            if !self.doc.layers.contains_key(&idx) {
                                                self.doc.get_or_create_layer(idx);
                                                self.selected_layer = idx;
                                                break;
                                            }
                                        }
                                    }
                                    if ui.add_enabled(can_remove_layer, egui::Button::new(tr("- Remove"))).on_hover_text("Remove currently selected layer").clicked() {
                                        layer_to_remove = Some(self.selected_layer);
                                    }
                                    if ui.button(tr("Save Palette")).on_hover_text("Save current layers & materials as startup default (~/.config/laser_burn/default_layers.json)").clicked() {
                                        if let Err(e) = Document::save_default_layers(&self.doc.layers) {
                                            self.status_msg = format!("Error saving default layers: {}", e);
                                        } else {
                                            self.status_msg = "Layer & material palette saved as startup default!".to_string();
                                        }
                                    }
                                    if ui.button(tr("Library")).on_hover_text("Open Material Library presets to apply to this layer or save new presets").clicked() {
                                        self.show_material_library = true;
                                    }
                                });

                                egui::ScrollArea::vertical().max_height(140.0).id_salt("prop_layers_list_scroll").show(ui, |ui| {
                                    for (idx, layer) in &mut self.doc.layers {
                                        let is_selected = *idx == self.selected_layer;
                                        let is_tool = layer.is_tool();
                                        let text = layer.display_label();

                                        let color = egui::Color32::from_rgb(
                                            u8::from_str_radix(&layer.color_hex[1..3], 16).unwrap_or(0),
                                            u8::from_str_radix(&layer.color_hex[3..5], 16).unwrap_or(0),
                                            u8::from_str_radix(&layer.color_hex[5..7], 16).unwrap_or(0),
                                        );

                                        ui.horizontal(|ui| {
                                            let (rect, _response) = ui.allocate_exact_size(egui::Vec2::new(14.0, 14.0), egui::Sense::hover());
                                            ui.painter().rect_filled(rect, 2.0, color);

                                            if ui.selectable_label(is_selected, text).clicked() {
                                                self.selected_layer = *idx;
                                            }
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if can_remove_layer {
                                                    if ui.small_button("✕").on_hover_text("Remove this layer").clicked() {
                                                        layer_to_remove = Some(*idx);
                                                    }
                                                }
                                                let lock_lbl = if layer.locked { tr("Locked") } else { tr("Lock") };
                                                let lock_color = if layer.locked { egui::Color32::from_rgb(255, 170, 0) } else { egui::Color32::GRAY };
                                                if ui.small_button(egui::RichText::new(lock_lbl).color(lock_color))
                                                    .on_hover_text(if layer.locked { "Layer is locked (shapes cannot be moved or edited). Click to unlock" } else { "Click to lock layer against accidental mouse moves" })
                                                    .clicked()
                                                {
                                                    layer.locked = !layer.locked;
                                                }
                                                if is_tool {
                                                    ui.label(egui::RichText::new(tr("No Output")).color(egui::Color32::from_rgb(255, 140, 0)).small())
                                                        .on_hover_text("Tool layer: used for visual guides/alignment, laser will never cut");
                                                } else {
                                                    ui.label(format!("{:.0}mm/s | {:.0}%", layer.cut_setting.speed, layer.cut_setting.power_max));
                                                    ui.checkbox(&mut layer.output, "").on_hover_text("Toggle laser output on/off for this layer");
                                                }
                                            });
                                        });
                                    }
                                });

                                if let Some(rem_idx) = layer_to_remove {
                                    if self.doc.layers.len() > 1 && self.doc.layers.remove(&rem_idx).is_some() {
                                        let fallback_layer = *self.doc.layers.keys().next().unwrap_or(&0);
                                        for shape in &mut self.doc.shapes {
                                            if shape.layer_index() == rem_idx {
                                                shape.set_layer_index(fallback_layer);
                                            }
                                        }
                                        if self.selected_layer == rem_idx || !self.doc.layers.contains_key(&self.selected_layer) {
                                            self.selected_layer = fallback_layer;
                                        }
                                        self.image_textures.clear();
                                        self.doc_version = self.doc_version.wrapping_add(1);
                                        self.status_msg = format!("Removed layer L{:02}", rem_idx);
                                    }
                                }

                                // Controls for currently selected layer
                                let mut invalidate_img_preview = false;
                                let mut open_mat_lib = false;
                                let mut open_save_mat_dialog = false;
                                if let Some(layer) = self.doc.layers.get_mut(&self.selected_layer) {
                                    ui.group(|ui| {
                                        if layer.is_tool() {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(tr("Name:")).strong());
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut layer.name)
                                                        .desired_width(140.0)
                                                        .hint_text("Tool Layer name"),
                                                );
                                            });
                                            ui.add_space(4.0);
                                            ui.label(egui::RichText::new("TOOL LAYER (Guide / Reference only)").color(egui::Color32::from_rgb(255, 140, 0)).strong());
                                            ui.label(egui::RichText::new("Shapes on this layer are used as visual alignment frames, jigs, material outlines, and notes. The laser will never cut or burn this layer.").small());
                                        } else {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(tr("Name:")).strong());
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut layer.name)
                                                        .desired_width(140.0)
                                                        .hint_text("e.g. Vaneri, Pahvi"),
                                                ).on_hover_text("Custom name for this layer (e.g. Vaneri, Pahvi, Leikkaus, Kaiverrus)");
                                                ui.checkbox(&mut layer.output, tr("Output")).on_hover_text("When unchecked, shapes on this layer will not be cut by the laser");
                                            });

                                            ui.add_space(2.0);

                                            ui.horizontal(|ui| {
                                                egui::ComboBox::from_label(tr("Mode"))
                                                    .selected_text(match layer.cut_setting.mode {
                                                        CutMode::Line => tr("Line (Cut/Score)"),
                                                        CutMode::Fill => tr("Fill (Engrave)"),
                                                        CutMode::OffsetFill => tr("Offset Fill"),
                                                        CutMode::Image => tr("Image (Dither)"),
                                                    })
                                                    .show_ui(ui, |ui| {
                                                        ui.selectable_value(&mut layer.cut_setting.mode, CutMode::Line, tr("Line (Cut/Score)"));
                                                        ui.selectable_value(&mut layer.cut_setting.mode, CutMode::Fill, tr("Fill (Engrave)"));
                                                        ui.selectable_value(&mut layer.cut_setting.mode, CutMode::OffsetFill, tr("Offset Fill"));
                                                        ui.selectable_value(&mut layer.cut_setting.mode, CutMode::Image, tr("Image (Dither)"));
                                                    });
                                                ui.checkbox(&mut layer.cut_setting.air_assist, tr("Air Assist"));
                                            });

                                            ui.add_space(4.0);

                                            egui::Grid::new("prop_layer_cut_settings_grid")
                                                .num_columns(3)
                                                .spacing([6.0, 6.0])
                                                .show(ui, |ui| {
                                                    // 1. Speed (Numeric input + Slider)
                                                    ui.label(tr("Speed:"));
                                                    ui.add(
                                                        egui::DragValue::new(&mut layer.cut_setting.speed)
                                                            .speed(1.0)
                                                            .range(1.0..=2000.0)
                                                            .suffix(" mm/s"),
                                                    ).on_hover_text("Click to type speed directly or drag");
                                                    ui.add(
                                                        egui::Slider::new(&mut layer.cut_setting.speed, 1.0..=600.0)
                                                            .clamping(egui::SliderClamping::Never)
                                                            .show_value(false),
                                                    );
                                                    ui.end_row();

                                                    // 2. Power (Max Power / Single Power)
                                                    let is_grayscale_power = layer.cut_setting.mode == CutMode::Image
                                                        && layer.cut_setting.dither_mode == DitherMode::GrayscalePower;

                                                    let power_label = if is_grayscale_power { tr("Max Power:") } else { tr("Power:") };
                                                    ui.label(power_label);
                                                    if ui.add(
                                                        egui::DragValue::new(&mut layer.cut_setting.power_max)
                                                            .speed(0.5)
                                                            .range(0.0..=100.0)
                                                            .suffix(" %"),
                                                    ).on_hover_text("Max laser power (e.g. for pure black or deep layer burn)").changed() {
                                                        invalidate_img_preview = true;
                                                    }
                                                    if ui.add(
                                                        egui::Slider::new(&mut layer.cut_setting.power_max, 0.0..=100.0)
                                                            .show_value(false),
                                                    ).changed() {
                                                        invalidate_img_preview = true;
                                                    }
                                                    ui.end_row();

                                                    // Min Power (Only shown for GrayscalePower)
                                                    if is_grayscale_power {
                                                        ui.label(tr("Min Power:"));
                                                        if ui.add(
                                                            egui::DragValue::new(&mut layer.cut_setting.power_min)
                                                                .speed(0.5)
                                                                .range(0.0..=100.0)
                                                                .suffix(" %"),
                                                        ).on_hover_text("Min laser power for light tones / top paint layer").changed() {
                                                            invalidate_img_preview = true;
                                                        }
                                                        if ui.add(
                                                            egui::Slider::new(&mut layer.cut_setting.power_min, 0.0..=100.0)
                                                                .show_value(false),
                                                        ).changed() {
                                                            invalidate_img_preview = true;
                                                        }
                                                        ui.end_row();
                                                    }

                                                    // 3. Passes (Numeric input + Slider)
                                                    ui.label(tr("Passes:"));
                                                    ui.add(
                                                        egui::DragValue::new(&mut layer.cut_setting.num_passes)
                                                            .speed(1)
                                                            .range(1..=100),
                                                    ).on_hover_text("Click to type passes count directly or drag");
                                                    ui.add(
                                                        egui::Slider::new(&mut layer.cut_setting.num_passes, 1..=20)
                                                            .clamping(egui::SliderClamping::Never)
                                                            .show_value(false),
                                                    );
                                                    ui.end_row();

                                                    // 4. Kerf (Full width + calculated offset)
                                                    ui.label(tr("Kerf (Width):"));
                                                    ui.add(
                                                        egui::DragValue::new(&mut layer.cut_setting.kerf)
                                                            .speed(0.005)
                                                            .max_decimals(3)
                                                            .range(0.0..=2.0)
                                                            .suffix(" mm"),
                                                    ).on_hover_text("Full beam width: outer shapes expand and inner holes shrink by half this value (kerf/2). Enter 0.0 to use global default.");
                                                    let eff = if layer.cut_setting.kerf > 0.0 {
                                                        layer.cut_setting.kerf
                                                    } else {
                                                        self.device.settings.default_kerf
                                                    };
                                                    ui.label(egui::RichText::new(format!("±{:.3} mm", eff * 0.5)).small().weak())
                                                        .on_hover_text("Actual contour offset distance (half of beam width)");
                                                    ui.end_row();

                                                    // Tabs & Bridges (Visible when Mode is Line)
                                                    if layer.cut_setting.mode == CutMode::Line {
                                                        ui.label(tr("Tabs / Bridges:"));
                                                        ui.checkbox(&mut layer.cut_setting.tabs_enabled, "");
                                                        ui.label(egui::RichText::new(tr("Enable tabs / bridges to keep cut parts attached")).small().weak());
                                                        ui.end_row();

                                                        if layer.cut_setting.tabs_enabled {
                                                            ui.label(tr("Tab Count:"));
                                                            ui.add(
                                                                egui::DragValue::new(&mut layer.cut_setting.tab_count)
                                                                    .speed(1)
                                                                    .range(1..=32),
                                                            ).on_hover_text("Number of micro-tabs/bridges per closed cut contour");
                                                            ui.label(egui::RichText::new("tabs / contour").small().weak());
                                                            ui.end_row();

                                                            ui.label(tr("Tab Width:"));
                                                            ui.add(
                                                                egui::DragValue::new(&mut layer.cut_setting.tab_width)
                                                                    .speed(0.1)
                                                                    .range(0.1..=10.0)
                                                                    .suffix(" mm"),
                                                            ).on_hover_text("Width of unburned holding gap (e.g. 0.5 mm)");
                                                            ui.label(egui::RichText::new("bridge gap width").small().weak());
                                                            ui.end_row();
                                                        }
                                                    }

                                                    // 5. Dither Algorithm (Visible when Mode is Image)
                                                    if layer.cut_setting.mode == CutMode::Image {
                                                        ui.label(tr("Dither:"));
                                                        egui::ComboBox::from_id_salt("prop_layer_dither_algo_dropdown")
                                                            .selected_text(match layer.cut_setting.dither_mode {
                                                                DitherMode::Atkinson => "Atkinson (Best Detail)",
                                                                DitherMode::FloydSteinberg => "Floyd-Steinberg",
                                                                DitherMode::Jarvis => "Jarvis-Judice-Ninke",
                                                                DitherMode::Stucki => "Stucki",
                                                                DitherMode::Halftone => "Halftone (Dots)",
                                                                DitherMode::OrderedBayer => "Ordered (Bayer 8x8)",
                                                                DitherMode::Threshold => "Threshold (B&W)",
                                                                DitherMode::GrayscalePower => "Grayscale Power (Analog / Ceramics)",
                                                                DitherMode::Sliced3D => "3D Sliced Relief (Korkokuva)",
                                                            })
                                                            .show_ui(ui, |ui| {
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::Atkinson, "Atkinson (Best Detail for Wood/Slate)");
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::FloydSteinberg, "Floyd-Steinberg (Classic Smooth)");
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::Jarvis, "Jarvis-Judice-Ninke (Clean Sharp)");
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::Stucki, "Stucki (High Contrast)");
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::Halftone, "Halftone (Engraving Screen Dots)");
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::OrderedBayer, "Ordered (Bayer 8x8 Matrix)");
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::Threshold, "Threshold (Pure Black & White)");
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::GrayscalePower, "Grayscale Power (Variable Laser Power / Ceramics)");
                                                                ui.selectable_value(&mut layer.cut_setting.dither_mode, DitherMode::Sliced3D, "3D Sliced Relief (Multi-Layer Depth / Wood Carving)");
                                                            });
                                                        ui.label(egui::RichText::new("Photo engraving algorithm").small().weak());
                                                        ui.end_row();

                                                        if layer.cut_setting.dither_mode == DitherMode::Sliced3D {
                                                            ui.label(tr("Thickness:"));
                                                            ui.add(
                                                                egui::DragValue::new(&mut layer.cut_setting.relief_material_thickness)
                                                                    .speed(0.1)
                                                                    .range(0.5..=50.0)
                                                                    .suffix(" mm"),
                                                            ).on_hover_text("Materiaalin kokonaispaksuus (esim. 3.0 mm vaneri)");
                                                            ui.label(egui::RichText::new("Vaneri / Puun paksuus").small().weak());
                                                            ui.end_row();

                                                            ui.label(tr("Target Depth:"));
                                                            let max_allowed = (layer.cut_setting.relief_material_thickness * 0.85).max(0.2);
                                                            ui.add(
                                                                egui::DragValue::new(&mut layer.cut_setting.relief_depth_mm)
                                                                    .speed(0.05)
                                                                    .range(0.1..=max_allowed)
                                                                    .suffix(" mm"),
                                                            ).on_hover_text("Kaiverruksen maksimisyvyys (suositus: enintään 70-80% paksuudesta läpipalamisen estämiseksi)");
                                                            let safety_warning = if layer.cut_setting.relief_depth_mm > layer.cut_setting.relief_material_thickness * 0.75 {
                                                                egui::RichText::new("⚠️ Lähellä läpipalamista!").color(egui::Color32::from_rgb(255, 180, 0)).small()
                                                            } else {
                                                                egui::RichText::new("Turvallinen syvyys").small().weak()
                                                            };
                                                            ui.label(safety_warning);
                                                            ui.end_row();

                                                            ui.label(tr("Slices:"));
                                                            ui.add(
                                                                egui::DragValue::new(&mut layer.cut_setting.relief_slices)
                                                                    .speed(1.0)
                                                                    .range(2..=128)
                                                                    .suffix(" krs"),
                                                            ).on_hover_text("Monessako syvyysviipaleessa 3D-korkokuva kaiverretaan (esim. 16-32 kerrosta)");
                                                            let depth_per_slice = layer.cut_setting.relief_depth_mm / layer.cut_setting.relief_slices.max(1) as f64;
                                                            ui.label(egui::RichText::new(format!("~{:.3} mm/krs", depth_per_slice)).small().weak());
                                                            ui.end_row();

                                                            ui.label(tr("Cross-Pass:"));
                                                            ui.checkbox(&mut layer.cut_setting.relief_alternate_angle, "Alternate (0° / 90°)")
                                                                .on_hover_text("Kääntää polttosuuntaa 90 astetta jokaisella kerroksella raitojen ja uurteiden poistamiseksi");
                                                            ui.label(egui::RichText::new("Tasainen pinta ilman raitoja").small().weak());
                                                            ui.end_row();

                                                            ui.label(tr("Retract (Z):"));
                                                            ui.add(
                                                                egui::DragValue::new(&mut layer.cut_setting.relief_clearance_z)
                                                                    .speed(0.1)
                                                                    .range(0.5..=20.0)
                                                                    .suffix(" mm"),
                                                            ).on_hover_text("Turvakorkeus siirtymissä laserpään ja ilmasuuttimen törmäyksen estämiseksi");
                                                            ui.label(egui::RichText::new("Törmäyssuojakorkeus").small().weak());
                                                            ui.end_row();
                                                        }

                                                        if layer.cut_setting.dither_mode == DitherMode::GrayscalePower {
                                                            ui.label(tr("Invert:"));
                                                            if ui.checkbox(&mut layer.cut_setting.invert_image, "Negative / Invert")
                                                                .on_hover_text("Invert light/dark mapping (e.g. for ceramics where laser removes dark coat to reveal light coat)")
                                                                .changed()
                                                            {
                                                                invalidate_img_preview = true;
                                                            }
                                                            ui.label(egui::RichText::new("For layered tiles / glass").small().weak());
                                                            ui.end_row();

                                                            ui.label(tr("Overscan:"));
                                                            ui.add(
                                                                egui::DragValue::new(&mut layer.cut_setting.overscan)
                                                                    .speed(0.1)
                                                                    .range(0.0..=20.0)
                                                                    .suffix(" mm"),
                                                            ).on_hover_text("Acceleration & deceleration margin outside image to eliminate edge-burning on tiles");
                                                            ui.label(egui::RichText::new("Prevents burnt tile edges").small().weak());
                                                            ui.end_row();
                                                        }
                                                    }

                                                    // 6. Line Interval / Resolution (Visible for Image & Fill modes)
                                                    if layer.cut_setting.mode == CutMode::Image || layer.cut_setting.mode == CutMode::Fill {
                                                        ui.label(tr("Interval:"));
                                                        ui.add(
                                                            egui::DragValue::new(&mut layer.cut_setting.line_interval)
                                                                .speed(0.005)
                                                                .range(0.02..=1.0)
                                                                .max_decimals(3)
                                                                .suffix(" mm"),
                                                        ).on_hover_text("Line interval / pitch between raster strokes (0.08mm = ~318 DPI)");
                                                        let dpi = if layer.cut_setting.line_interval > 0.001 {
                                                            (25.4 / layer.cut_setting.line_interval).round() as i32
                                                        } else {
                                                            318
                                                        };
                                                        ui.label(egui::RichText::new(format!("~{} DPI", dpi)).small().weak());
                                                        ui.end_row();
                                                    }

                                                    // 7. Hatch Scan Angle & Cross-hatch (Visible for Fill modes)
                                                    if layer.cut_setting.mode == CutMode::Fill || layer.cut_setting.mode == CutMode::OffsetFill {
                                                        ui.label(tr("Scan Angle:"));
                                                        ui.horizontal(|ui| {
                                                            ui.add(
                                                                egui::DragValue::new(&mut layer.cut_setting.scan_angle)
                                                                    .speed(1.0)
                                                                    .range(-180.0..=180.0)
                                                                    .suffix("°"),
                                                            );
                                                            if ui.selectable_label((layer.cut_setting.scan_angle - 0.0).abs() < 0.1, "0°").clicked() {
                                                                layer.cut_setting.scan_angle = 0.0;
                                                            }
                                                            if ui.selectable_label((layer.cut_setting.scan_angle - 90.0).abs() < 0.1, "90°").clicked() {
                                                                layer.cut_setting.scan_angle = 90.0;
                                                            }
                                                            if ui.selectable_label((layer.cut_setting.scan_angle - 45.0).abs() < 0.1, "45°").clicked() {
                                                                layer.cut_setting.scan_angle = 45.0;
                                                            }
                                                        });
                                                        let angle_desc = if (layer.cut_setting.scan_angle - 0.0).abs() < 0.1 {
                                                            "Vaaka"
                                                        } else if (layer.cut_setting.scan_angle - 90.0).abs() < 0.1 {
                                                            "Pysty"
                                                        } else if (layer.cut_setting.scan_angle - 45.0).abs() < 0.1 {
                                                            "45°"
                                                        } else {
                                                            "Kulma"
                                                        };
                                                        ui.label(egui::RichText::new(angle_desc).small().weak());
                                                        ui.end_row();

                                                        ui.label("Ristiinpoltto:");
                                                        ui.checkbox(&mut layer.cut_setting.cross_hatch, "Cross-Hatch (+90°)");
                                                        ui.label(egui::RichText::new("2-suuntainen").small().weak());
                                                        ui.end_row();
                                                    }
                                                });

                                            // Ceramic Multi-Layer Paint Color Simulator & Mapper
                                            if layer.cut_setting.mode == CutMode::Image
                                                && layer.cut_setting.dither_mode == DitherMode::GrayscalePower
                                            {
                                                ui.add_space(4.0);
                                                ui.separator();
                                                ui.horizontal(|ui| {
                                                    if ui.checkbox(
                                                        &mut layer.cut_setting.ceramic_preview,
                                                        egui::RichText::new("Ceramic Paint Layer Simulator").strong().color(egui::Color32::from_rgb(255, 190, 80)),
                                                    )
                                                    .on_hover_text("Simulate multi-layer spray-painted ceramic tile colors directly on the canvas based on laser power % thresholds!")
                                                    .changed()
                                                    {
                                                        invalidate_img_preview = true;
                                                    }
                                                    if layer.cut_setting.ceramic_preview {
                                                        if ui.small_button("Reset Colors").clicked() {
                                                            layer.cut_setting.ceramic_bands = crate::document::default_ceramic_bands();
                                                            invalidate_img_preview = true;
                                                        }
                                                    }
                                                });

                                                if layer.cut_setting.ceramic_preview {
                                                    if layer.cut_setting.ceramic_bands.is_empty() {
                                                        layer.cut_setting.ceramic_bands = crate::document::default_ceramic_bands();
                                                    }
                                                    for band in layer.cut_setting.ceramic_bands.iter_mut() {
                                                        match band.label.as_str() {
                                                            "1. Pintamaali (Musta)" => band.label = "1. Top Coat (Black)".to_string(),
                                                            "2. Välikerros (Sininen)" => band.label = "2. Mid Coat (Blue)".to_string(),
                                                            "3. Pohjamaali (Keltainen/Kulta)" => band.label = "3. Base Coat (Yellow/Gold)".to_string(),
                                                            "4. Keraaminen laatta (Valkoinen)" => band.label = "4. Ceramic Tile (White)".to_string(),
                                                            _ => {}
                                                        }
                                                    }
                                                    ui.label(egui::RichText::new("Define paint coat colors & the max laser power (%) that reveals each coat:").small().weak());
                                                    let mut remove_idx = None;
                                                    for (b_idx, band) in layer.cut_setting.ceramic_bands.iter_mut().enumerate() {
                                                        ui.horizontal(|ui| {
                                                            let hex = band.color_hex.trim_start_matches('#');
                                                            let mut srgb = if hex.len() >= 6 {
                                                                [
                                                                    u8::from_str_radix(&hex[0..2], 16).unwrap_or(128),
                                                                    u8::from_str_radix(&hex[2..4], 16).unwrap_or(128),
                                                                    u8::from_str_radix(&hex[4..6], 16).unwrap_or(128),
                                                                ]
                                                            } else {
                                                                [128, 128, 128]
                                                            };
                                                            if ui.color_edit_button_srgb(&mut srgb).changed() {
                                                                band.color_hex = format!("#{:02X}{:02X}{:02X}", srgb[0], srgb[1], srgb[2]);
                                                                invalidate_img_preview = true;
                                                            }
                                                            let text_w = (ui.available_width() - 102.0).max(155.0);
                                                            if ui.add(egui::TextEdit::singleline(&mut band.label).desired_width(text_w)).changed() {
                                                                invalidate_img_preview = true;
                                                            }
                                                            ui.label("≤");
                                                            if ui.add(
                                                                egui::DragValue::new(&mut band.max_power_pct)
                                                                    .speed(0.5)
                                                                    .range(0.0..=100.0)
                                                                    .suffix("%"),
                                                            ).on_hover_text("Up to this laser power %, this paint layer remains visible").changed() {
                                                                invalidate_img_preview = true;
                                                            }
                                                            if ui.small_button("✕").on_hover_text("Remove paint coat").clicked() {
                                                                remove_idx = Some(b_idx);
                                                            }
                                                        });
                                                    }
                                                    if let Some(ri) = remove_idx {
                                                        if layer.cut_setting.ceramic_bands.len() > 2 {
                                                            layer.cut_setting.ceramic_bands.remove(ri);
                                                            invalidate_img_preview = true;
                                                        }
                                                    }
                                                    if layer.cut_setting.ceramic_bands.len() < 8 {
                                                        if ui.small_button(tr("+ Add Paint Coat")).clicked() {
                                                            layer.cut_setting.ceramic_bands.push(crate::document::CeramicColorBand {
                                                                label: format!("Coat {}", layer.cut_setting.ceramic_bands.len() + 1),
                                                                max_power_pct: 90.0,
                                                                color_hex: "#C8C8C8".to_string(),
                                                            });
                                                            invalidate_img_preview = true;
                                                        }
                                                    }
                                                }
                                            }

                                            ui.add_space(6.0);
                                            ui.horizontal(|ui| {
                                                if ui.button(tr("⬇ Assign from Library")).on_hover_text("Open Material Library to apply a calibrated preset to this layer").clicked() {
                                                    open_mat_lib = true;
                                                }
                                                if ui.button(tr("⬆ Save to Library")).on_hover_text("Save this layer's speed, power, and passes into the Material Library").clicked() {
                                                    open_save_mat_dialog = true;
                                                }
                                            });
                                        }
                                    });
                                }
                                if open_mat_lib {
                                    self.show_material_library = true;
                                }
                                if open_save_mat_dialog {
                                    self.open_save_layer_dialog();
                                }
                                if invalidate_img_preview {
                                    self.image_textures.clear();
                                    self.doc_version = self.doc_version.wrapping_add(1);
                                }
                            });
                    });
            });

        if let Some(r) = resp {
            self.record_window_rect("floating_properties_window", r.response.rect);
        }
        self.show_properties_window = is_open;
    }

    /// Renders the standalone floating "Objects List" window (Shapes, Groups, Selection)
    pub fn render_objects_window(&mut self, ctx: &egui::Context) {
        if !self.show_objects_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let mut is_open = true;
        let screen_rect = ctx.screen_rect();
        let obj_count = self.doc.shapes.len();
        let sel_count = self.selected_shape_indices.len();
        let (def_pos, def_size) = self.window_pos_size("floating_objects_window", [screen_rect.width() - 390.0, 480.0], [340.0, 360.0]);

        let resp = egui::Window::new(format!("{} ({})", tr("Objects List"), obj_count))
            .id(egui::Id::new("floating_objects_window"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .min_width(280.0)
            .min_height(160.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                // Quick actions toolbar
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button(tr("Select All")).clicked() {
                        self.selected_shape_indices = (0..self.doc.shapes.len()).collect();
                    }
                    if ui.small_button(tr("Deselect")).clicked() {
                        self.selected_shape_indices.clear();
                    }
                    if sel_count > 0 {
                        let is_locked = self.selected_shape_indices.iter().any(|&i| i < self.doc.shapes.len() && self.doc.shapes[i].is_locked());
                        let lock_lbl = if is_locked { "Avaa" } else { "Lukitse" };
                        if ui.small_button(lock_lbl).clicked() {
                            self.toggle_lock_selected();
                        }
                        if ui.small_button("Poista").clicked() {
                            self.delete_selected();
                        }
                    }
                });

                if sel_count > 0 {
                    ui.label(egui::RichText::new(format!("{} {} {}", sel_count, tr("Shapes Selected"), if sel_count > 1 { "" } else { "" })).small().color(egui::Color32::from_rgb(255, 140, 0)));
                }

                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.doc.shapes.is_empty() {
                            ui.label(egui::RichText::new(tr("No shapes on canvas.")).italics().color(egui::Color32::GRAY));
                        }
                        for (idx, shape) in self.doc.shapes.iter().enumerate() {
                            let is_sel = self.selected_shape_indices.contains(&idx);
                            let b = shape.bounds();
                            let lock_str = if shape.is_locked() { format!(" [{}]", tr("Locked")) } else { "".to_string() };
                            let layer_tag = if shape.layer_index() >= 30 {
                                format!("[T{:02}]", shape.layer_index() - 29)
                            } else {
                                format!("[L{:02}]", shape.layer_index())
                            };
                            let desc = match shape {
                                Shape::Group { shapes, .. } => format!("#{}: {} ({} {}) {}{}", idx + 1, tr("Group (Shape)"), shapes.len(), tr("shapes"), layer_tag, lock_str),
                                Shape::Path { .. } => format!("#{}: {} ({:.1}x{:.1}mm) {}{}", idx + 1, tr("Path"), b.width(), b.height(), layer_tag, lock_str),
                                Shape::Rect { .. } => format!("#{}: {} ({:.1}x{:.1}mm) {}{}", idx + 1, tr("Rect"), b.width(), b.height(), layer_tag, lock_str),
                                Shape::Circle { radius, .. } => format!("#{}: {} (D={:.1}mm) {}{}", idx + 1, tr("Circle"), radius * 2.0, layer_tag, lock_str),
                                Shape::Image { .. } => format!("#{}: {} ({:.1}x{:.1}mm) {}{}", idx + 1, tr("Image"), b.width(), b.height(), layer_tag, lock_str),
                            };

                            let shift_held = ui.input(|i| i.modifiers.shift);
                            if ui.selectable_label(is_sel, desc).clicked() {
                                if shift_held {
                                    if is_sel {
                                        self.selected_shape_indices.retain(|&i| i != idx);
                                    } else {
                                        self.selected_shape_indices.push(idx);
                                    }
                                } else {
                                    self.selected_shape_indices = vec![idx];
                                }
                                if idx < self.doc.shapes.len() {
                                    self.selected_layer = self.doc.shapes[idx].layer_index();
                                }
                                self.status_msg = format!("Selected shape #{}", idx + 1);
                            }
                        }
                    });
            });

        if let Some(r) = resp {
            self.record_window_rect("floating_objects_window", r.response.rect);
        }
        self.show_objects_window = is_open;
    }

    /// Renders the standalone floating "Job Control & Execution" window (Start, Stop, Progress, Simulation Stats)
    pub fn render_job_control_window(&mut self, ctx: &egui::Context) {
        if !self.show_job_window {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let mut is_open = true;
        let screen_rect = ctx.screen_rect();

        let job_header_text = if self.device.is_job_running {
            format!("{} [KÄYNNISSÄ]", tr("Job Control"))
        } else {
            tr("Job Control").to_string()
        };
        let (def_pos, def_size) = self.window_pos_size("floating_job_control_window", [screen_rect.width() - 390.0, 90.0], [350.0, 360.0]);

        let resp = egui::Window::new(job_header_text)
            .id(egui::Id::new("floating_job_control_window"))
            .open(&mut is_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .min_width(290.0)
            .min_height(180.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Execution controls
                        if !self.device.is_job_running {
                            let est = self.get_job_estimate();
                            ui.label(
                                egui::RichText::new(format!("{} ~{}", tr("Estimated Time:"), est.formatted_time))
                                    .strong()
                                    .color(self.color_blue()),
                            );
                            ui.add_space(4.0);

                            let start_btn = egui::Button::new(
                                egui::RichText::new(tr(" START JOB (CUT) ")).strong().color(egui::Color32::WHITE),
                            ).fill(egui::Color32::from_rgb(34, 139, 34));
                            if ui.add_sized([ui.available_width(), 36.0], start_btn)
                                .on_hover_text("Optimize and run cut job with laser")
                                .clicked()
                            {
                                self.start_job();
                            }
                        } else {
                            ui.horizontal(|ui| {
                                if self.device.is_job_paused {
                                    let resume_btn = egui::Button::new(
                                        egui::RichText::new(tr(" RESUME ")).strong().color(egui::Color32::WHITE),
                                    ).fill(egui::Color32::from_rgb(0, 140, 255));
                                    if ui.add_sized([ui.available_width() * 0.5 - 4.0, 32.0], resume_btn).clicked() {
                                        self.device.resume_job();
                                    }
                                } else {
                                    let pause_btn = egui::Button::new(
                                        egui::RichText::new(tr(" PAUSE ")).strong().color(egui::Color32::BLACK),
                                    ).fill(egui::Color32::from_rgb(255, 193, 7));
                                    if ui.add_sized([ui.available_width() * 0.5 - 4.0, 32.0], pause_btn).clicked() {
                                        self.device.pause_job();
                                    }
                                }

                                let stop_btn = egui::Button::new(
                                    egui::RichText::new(tr(" STOP ")).strong().color(egui::Color32::WHITE),
                                ).fill(egui::Color32::RED);
                                if ui.add_sized([ui.available_width(), 32.0], stop_btn).clicked() {
                                    self.device.stop_job();
                                }
                            });

                            let pct = self.device.job_progress;
                            ui.add_space(4.0);
                            ui.add(egui::ProgressBar::new(pct).text(format!("{:.1}% ({}/{})", pct * 100.0, self.device.job_idx, self.device.job_total_lines)));

                            if let Some(st) = self.device.job_start_instant {
                                let elapsed = st.elapsed().as_secs_f64();
                                let elapsed_str = crate::simulator::Simulator::format_duration(elapsed);
                                let remaining_sec = if pct > 0.05 {
                                    let rate_est = elapsed / (pct as f64).clamp(0.01, 1.0);
                                    let model_rem = (self.device.job_estimated_total_sec - elapsed).max(0.0);
                                    let prog_rem = (rate_est - elapsed).max(0.0);
                                    (model_rem * 0.3 + prog_rem * 0.7).max(0.0)
                                } else {
                                    (self.device.job_estimated_total_sec - elapsed).max(0.0)
                                };
                                let rem_str = crate::simulator::Simulator::format_duration(remaining_sec);

                                let c_blue = self.color_blue();
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("{} {}", tr("Elapsed:"), elapsed_str)).size(11.5));
                                    ui.label("|");
                                    ui.label(egui::RichText::new(format!("{} ~{}", tr("Remaining:"), rem_str)).strong().color(c_blue).size(11.5));
                                });
                            }
                            if self.device.is_job_paused {
                                ui.label(egui::RichText::new(tr("JOB PAUSED")).strong().color(self.color_orange()));
                            }
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // Simulation Stats
                        egui::CollapsingHeader::new(egui::RichText::new(tr("Simulation Stats")).strong().size(12.5))
                            .default_open(true)
                            .show(ui, |ui| {
                                let stats = self.get_job_estimate();
                                ui.label(format!("{} {:.1} mm", tr("Cut Distance:"), stats.total_cut_distance_mm));
                                ui.label(format!("{} {:.1} mm", tr("Rapid Distance:"), stats.total_rapid_distance_mm));
                                ui.label(egui::RichText::new(format!("{} {}", tr("Est. Time:"), stats.formatted_time)).strong().color(egui::Color32::from_rgb(0, 180, 255)));
                            });

                        #[cfg(target_os = "macos")]
                        {
                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);
                            egui::CollapsingHeader::new(egui::RichText::new(tr("Apple Shortcuts Automation (macOS)")).strong().size(12.0))
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("Automatically trigger macOS Shortcuts (e.g. smart plug, exhaust fan, or notification) when a burn job starts or finishes.")
                                            .small()
                                            .weak(),
                                    );
                                    ui.horizontal(|ui| {
                                        if ui.small_button("Load Shortcuts List").on_hover_text("Fetch installed Apple Shortcuts from macOS").clicked() {
                                            self.available_shortcuts = crate::device::DeviceManager::list_macos_shortcuts();
                                        }
                                        if !self.available_shortcuts.is_empty() {
                                            ui.label(egui::RichText::new(format!("({} found)", self.available_shortcuts.len())).small().weak());
                                        }
                                    });
                                    ui.add_space(2.0);

                                    // 1. On Job Start
                                    if ui.checkbox(&mut self.device.settings.run_shortcut_on_start, "Run Shortcut when Job Starts").changed() {
                                        let _ = self.device.settings.save();
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label("Start:");
                                        if ui.add(
                                            egui::TextEdit::singleline(&mut self.device.settings.shortcut_on_start_name)
                                                .hint_text("Shortcut name (e.g. Pöytä päälle)")
                                                .desired_width(155.0),
                                        ).changed() {
                                            let _ = self.device.settings.save();
                                        }
                                        if !self.available_shortcuts.is_empty() {
                                            egui::ComboBox::from_id_salt("job_shortcut_start_picker")
                                                .selected_text("Pick")
                                                .width(55.0)
                                                .show_ui(ui, |ui| {
                                                    for sc in &self.available_shortcuts {
                                                        if ui.selectable_label(self.device.settings.shortcut_on_start_name == *sc, sc).clicked() {
                                                            self.device.settings.shortcut_on_start_name = sc.clone();
                                                            let _ = self.device.settings.save();
                                                        }
                                                    }
                                                });
                                        }
                                        if ui.small_button("Test").on_hover_text("Run this Shortcut now").clicked() {
                                            let name = self.device.settings.shortcut_on_start_name.clone();
                                            self.device.trigger_macos_shortcut(&name, "Manual Test");
                                        }
                                    });

                                    ui.add_space(4.0);

                                    // 2. On Job Complete
                                    if ui.checkbox(&mut self.device.settings.run_shortcut_on_complete, "Run Shortcut when Job Finishes").changed() {
                                        let _ = self.device.settings.save();
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label("Done:");
                                        if ui.add(
                                            egui::TextEdit::singleline(&mut self.device.settings.shortcut_on_complete_name)
                                                .hint_text("Shortcut name (e.g. Laser valmis)")
                                                .desired_width(155.0),
                                        ).changed() {
                                            let _ = self.device.settings.save();
                                        }
                                        if !self.available_shortcuts.is_empty() {
                                            egui::ComboBox::from_id_salt("job_shortcut_done_picker")
                                                .selected_text("Pick")
                                                .width(55.0)
                                                .show_ui(ui, |ui| {
                                                    for sc in &self.available_shortcuts {
                                                        if ui.selectable_label(self.device.settings.shortcut_on_complete_name == *sc, sc).clicked() {
                                                            self.device.settings.shortcut_on_complete_name = sc.clone();
                                                            let _ = self.device.settings.save();
                                                        }
                                                    }
                                                });
                                        }
                                        if ui.small_button("Test").on_hover_text("Run this Shortcut now").clicked() {
                                            let name = self.device.settings.shortcut_on_complete_name.clone();
                                            self.device.trigger_macos_shortcut(&name, "Manual Test");
                                        }
                                    });
                                });
                        }
                    });
            });

        if let Some(r) = resp {
            self.record_window_rect("floating_job_control_window", r.response.rect);
        }
        self.show_job_window = is_open;
    }

    /// Renders the floating "Data Traffic (TX / RX)" window (Live G-Code stream, filters, export)
    pub fn render_traffic_window(&mut self, ctx: &egui::Context) {
        if !self.show_traffic_panel {
            return;
        }

        let lang = self.device.settings.language;
        let tr = |k| crate::i18n::t(k, lang);
        let mut is_traffic_open = true;
        let (def_pos, def_size) = self.window_pos_size("serial_traffic_window", [ctx.screen_rect().width() - 480.0, 140.0], [360.0, 480.0]);

        let resp = egui::Window::new(tr("Data Traffic (TX / RX)"))
            .id(egui::Id::new("serial_traffic_window"))
            .open(&mut is_traffic_open)
            .default_pos(def_pos)
            .default_size(def_size)
            .min_width(260.0)
            .min_height(200.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Live G-code Stream").weak().size(11.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(tr("Clear")).on_hover_text("Clear traffic history").clicked() {
                            self.device.console_history.clear();
                        }
                    });
                });

                ui.separator();

                // File logging checkbox & Open Log File button
                ui.horizontal(|ui| {
                    let log_path_str = crate::device::DeviceSettings::traffic_log_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "~/Documents/PhotonCAM_traffic.log".to_string());
                    if ui.checkbox(&mut self.device.settings.save_traffic_log, tr("Save to Log File"))
                        .on_hover_text(format!("Record all TX/RX data traffic to:\n{}", log_path_str))
                        .changed()
                    {
                        let _ = self.device.settings.save();
                        if self.device.settings.save_traffic_log {
                            self.device.log(&format!("[LOG] File logging enabled -> {}", log_path_str));
                        } else {
                            self.device.log("[LOG] File logging disabled");
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(tr("Open Log")).on_hover_text(format!("Open log file:\n{}", log_path_str)).clicked() {
                            if let Some(path) = crate::device::DeviceSettings::traffic_log_path() {
                                if !path.exists() {
                                    let _ = std::fs::write(&path, "# PhotonCAM Data Traffic Log\n");
                                }
                                #[cfg(target_os = "macos")]
                                let _ = std::process::Command::new("open").arg(&path).spawn();
                                #[cfg(target_os = "windows")]
                                let _ = std::process::Command::new("explorer").arg(&path).spawn();
                                #[cfg(target_os = "linux")]
                                let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                            }
                        }
                    });
                });

                // Filter options
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.explain_traffic_codes, tr("Explain"))
                        .on_hover_text("Append human-readable English descriptions after G-code and GRBL commands/responses");
                    if ui.checkbox(&mut self.device.settings.log_status_polls, "<Status>")
                        .on_hover_text("Include periodic GRBL status frames (<Idle|MPos:...>) in log")
                        .changed()
                    {
                        let _ = self.device.settings.save();
                    }
                    if ui.checkbox(&mut self.hide_ok_in_traffic_log, tr("Hide 'ok'"))
                        .on_hover_text("Filter out simple 'ok' confirmations from controller to keep stream readable")
                        .changed()
                    {
                        // filter flag updated
                    }
                });

                ui.separator();

                // Live G-code Stream Log
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let filter_ok = self.hide_ok_in_traffic_log;
                        let explain = self.explain_traffic_codes;

                        for line in self.device.console_history.iter() {
                            if filter_ok && (line.trim() == "ok" || line.contains("ok")) {
                                continue;
                            }
                            let is_sent = line.starts_with("> ");
                            let is_status = line.starts_with("<") && line.contains('|');
                            let is_error = line.to_lowercase().contains("error") || line.to_lowercase().contains("alarm");

                            let color = if is_error {
                                egui::Color32::from_rgb(255, 80, 80)
                            } else if is_sent {
                                egui::Color32::from_rgb(100, 200, 255)
                            } else if is_status {
                                egui::Color32::from_rgb(140, 160, 180)
                            } else {
                                egui::Color32::from_rgb(120, 220, 140)
                            };

                            let mut label_text = line.clone();
                            if explain {
                                let trimmed = line.trim();
                                let cmd = if trimmed.starts_with("> ") {
                                    trimmed.trim_start_matches("> ").trim()
                                } else {
                                    trimmed
                                };
                                if let Some(desc) = crate::device::DeviceManager::explain_traffic_line(cmd) {
                                    label_text = format!("{}  # {}", line, desc);
                                }
                            }

                            ui.label(egui::RichText::new(label_text).font(egui::FontId::monospace(10.5)).color(color));
                        }
                    });
            });

        if let Some(r) = resp {
            self.record_window_rect("serial_traffic_window", r.response.rect);
        }
        self.show_traffic_panel = is_traffic_open;
    }
}

pub mod box_maker;
pub mod living_hinge;
pub mod mesh_slicer;
pub mod qr_code;
pub mod variable_text;

pub use box_maker::{generate_box_parts, layout_box_shapes, BoxLidType, BoxMakerParams, BoxPart, EnclosurePreset};
pub use living_hinge::{generate_living_hinge, living_hinge_to_shapes, HingePattern, LivingHingeParams};
pub use mesh_slicer::{layout_sliced_layers, slice_mesh_to_layers, MeshSlicerParams, SliceAxis, SlicedLayer};
pub use qr_code::{generate_qr_shapes, QrCodeParams, QrEcLevel, QrStyle};
pub use variable_text::{generate_variable_text_shapes, resolve_variable_strings, VariableTextMode, VariableTextParams};

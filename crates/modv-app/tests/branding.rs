//! Guards for the VmodV branding assets.
//!
//! The AppImage copies `resources/icon.png` to the bundle root and the `.deb`
//! installs it into `hicolor/512x512/apps`. Both need a square, high-resolution
//! PNG, and the editable source must keep the mirrored V that distinguishes
//! VmodV from the legacy modV mark.

use std::path::{Path, PathBuf};

fn resource(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources")
        .join(name)
}

#[test]
fn the_shipped_icon_is_a_square_png_at_least_512px() {
    let bytes = std::fs::read(resource("icon.png")).expect("resources/icon.png is shipped");
    assert!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "resources/icon.png must be a PNG"
    );
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    assert_eq!(width, height, "app icons must be square");
    assert!(
        width >= 512,
        "the .deb installs the icon into hicolor/512x512; got {width}px"
    );
}

#[test]
fn the_logo_has_an_editable_svg_source_with_a_mirrored_v() {
    let svg = std::fs::read_to_string(resource("icon.svg"))
        .expect("resources/icon.svg is the editable source for the app icon");
    assert!(
        svg.contains("viewBox=\"0 0 276.7 276.7\""),
        "the source keeps the legacy 276.7-unit artboard"
    );
    assert!(
        svg.contains("#00E676"),
        "the mark uses the VmodV neon-green accent"
    );
    // The legacy right-hand V is duplicated on the left so the mark reads
    // V-M-V. Keep both copies in the source.
    let legacy_v = "M187.6,189.2";
    assert_eq!(
        svg.matches(legacy_v).count(),
        2,
        "the V must be mirrored on the left of the mark"
    );
}

//! The bundled IBM Plex faces.
//!
//! The VmodV shell ships four static TrueType faces so it looks the same on
//! every host instead of inheriting whatever system fonts the platform has:
//!
//! - IBM Plex Sans Regular — body/button text
//! - IBM Plex Sans SemiBold — headings and the named strong family
//! - IBM Plex Mono Regular — ids, rates and numeric read-outs
//! - IBM Plex Mono Medium — the named medium family
//!
//! egui's bundled Ubuntu/Hack faces stay in each family list *after* Plex, so
//! symbols and emoji the Plex subsets do not cover still render. The faces are
//! SIL OFL 1.1; see `assets/fonts/OFL.txt` and `THIRD_PARTY_NOTICES.md`.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// IBM Plex Sans Regular (`IBMPlexSans-Regular.ttf`).
pub const SANS_REGULAR: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf");
/// IBM Plex Sans SemiBold (`IBMPlexSans-SemiBold.ttf`).
pub const SANS_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf");
/// IBM Plex Mono Regular (`IBMPlexMono-Regular.ttf`).
pub const MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf");
/// IBM Plex Mono Medium (`IBMPlexMono-Medium.ttf`).
pub const MONO_MEDIUM: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf");

/// Key for Plex Sans Regular in [`FontDefinitions::font_data`].
pub const SANS_REGULAR_KEY: &str = "IBM Plex Sans";
/// Key for Plex Sans SemiBold in [`FontDefinitions::font_data`].
pub const SANS_SEMIBOLD_KEY: &str = "IBM Plex Sans SemiBold";
/// Key for Plex Mono Regular in [`FontDefinitions::font_data`].
pub const MONO_REGULAR_KEY: &str = "IBM Plex Mono";
/// Key for Plex Mono Medium in [`FontDefinitions::font_data`].
pub const MONO_MEDIUM_KEY: &str = "IBM Plex Mono Medium";

/// The named family `RichText::family` can select for semibold text.
pub const SANS_SEMIBOLD_FAMILY: &str = "plex-sans-semibold";
/// The named family for medium monospace text.
pub const MONO_MEDIUM_FAMILY: &str = "plex-mono-medium";

/// Build the font definitions: Plex first in each family, egui's bundled faces
/// as fallbacks, plus the named semibold/medium families.
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut insert = |key: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(key.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    insert(SANS_REGULAR_KEY, SANS_REGULAR);
    insert(SANS_SEMIBOLD_KEY, SANS_SEMIBOLD);
    insert(MONO_REGULAR_KEY, MONO_REGULAR);
    insert(MONO_MEDIUM_KEY, MONO_MEDIUM);

    // egui picks glyphs from the head of each family list, so Plex goes first
    // and the bundled faces keep their order behind it.
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, SANS_REGULAR_KEY.to_owned());
    proportional.insert(1, SANS_SEMIBOLD_KEY.to_owned());
    let sans_fallbacks = proportional.clone();

    let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
    monospace.insert(0, MONO_REGULAR_KEY.to_owned());
    monospace.insert(1, MONO_MEDIUM_KEY.to_owned());
    let mono_fallbacks = monospace.clone();

    // The per-glyph fallback chain is the same for the named families, with the
    // requested weight in front: `RichText::strong()` on the default family
    // keeps the regular face (egui has no synthetic bold), so headings and any
    // explicit strong family opt into SemiBold through these lists.
    fonts.families.insert(
        FontFamily::Name(SANS_SEMIBOLD_FAMILY.into()),
        with_head(SANS_SEMIBOLD_KEY, &sans_fallbacks),
    );
    fonts.families.insert(
        FontFamily::Name(MONO_MEDIUM_FAMILY.into()),
        with_head(MONO_MEDIUM_KEY, &mono_fallbacks),
    );
    fonts
}

/// A family list with `key` at the head and the remaining entries behind it.
fn with_head(key: &str, rest: &[String]) -> Vec<String> {
    let mut family = vec![key.to_owned()];
    family.extend(rest.iter().filter(|entry| *entry != key).cloned());
    family
}

/// Install the bundled fonts on `ctx` (called from [`crate::theme::apply`]).
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_face_has_bytes() {
        for (name, bytes) in [
            ("IBMPlexSans-Regular.ttf", SANS_REGULAR),
            ("IBMPlexSans-SemiBold.ttf", SANS_SEMIBOLD),
            ("IBMPlexMono-Regular.ttf", MONO_REGULAR),
            ("IBMPlexMono-Medium.ttf", MONO_MEDIUM),
        ] {
            assert!(bytes.len() > 100_000, "{name} should be a full TTF");
            // TrueType outlines start with 0x00010000 (also 0x74727565 "true";
            // Plex is 0x00010000).
            let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            assert!(
                magic == 0x0001_0000 || magic == 0x7472_7565,
                "{name} is not a TrueType font (magic {magic:#010x})"
            );
        }
    }

    #[test]
    fn plex_leads_every_family_and_keeps_the_bundled_fallbacks() {
        let fonts = definitions();
        let proportional = &fonts.families[&FontFamily::Proportional];
        assert_eq!(
            proportional.first().map(String::as_str),
            Some(SANS_REGULAR_KEY)
        );
        assert!(proportional.contains(&SANS_SEMIBOLD_KEY.to_owned()));
        assert!(
            proportional.contains(&"Ubuntu-Light".to_owned()),
            "egui's bundled proportional face stays as the glyph fallback"
        );

        let monospace = &fonts.families[&FontFamily::Monospace];
        assert_eq!(
            monospace.first().map(String::as_str),
            Some(MONO_REGULAR_KEY)
        );
        assert!(monospace.contains(&MONO_MEDIUM_KEY.to_owned()));
        assert!(
            monospace.contains(&"Hack".to_owned()),
            "egui's bundled monospace face stays as the glyph fallback"
        );
    }

    #[test]
    fn the_named_weight_families_start_with_their_weight() {
        let fonts = definitions();
        let semibold = &fonts.families[&FontFamily::Name(SANS_SEMIBOLD_FAMILY.into())];
        assert_eq!(
            semibold.first().map(String::as_str),
            Some(SANS_SEMIBOLD_KEY)
        );
        let medium = &fonts.families[&FontFamily::Name(MONO_MEDIUM_FAMILY.into())];
        assert_eq!(medium.first().map(String::as_str), Some(MONO_MEDIUM_KEY));
        assert!(fonts.font_data.contains_key(SANS_SEMIBOLD_KEY));
        assert!(fonts.font_data.contains_key(MONO_MEDIUM_KEY));
    }
}

//! Load bundled UI icons and discover installed monospace fonts, including user directories.
use egui::{FontData, FontDefinitions, FontFamily};
use std::path::PathBuf;

pub const MATERIAL_SYMBOLS: &str = "Material Symbols Rounded";

pub fn load(ctx: &egui::Context) -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    // Include the standard user directories explicitly, even when Fontconfig's
    // configured directories omit them. load_fonts_dir scans nested directories.
    if let Some(user_home) = std::env::var_os("HOME") {
        let user_home = PathBuf::from(user_home);
        database.load_fonts_dir(user_home.join(".fonts"));
        database.load_fonts_dir(user_home.join(".local/share/fonts"));
    }
    if let Some(user_data) = std::env::var_os("XDG_DATA_HOME").filter(|path| !path.is_empty()) {
        database.load_fonts_dir(PathBuf::from(user_data).join("fonts"));
    }
    let (definitions, names) = definitions(&database);
    ctx.set_fonts(definitions);
    names
}

fn definitions(database: &fontdb::Database) -> (FontDefinitions, Vec<String>) {
    let mut definitions = FontDefinitions::default();
    definitions.font_data.insert(
        MATERIAL_SYMBOLS.into(),
        FontData::from_static(include_bytes!(
            "../assets/MaterialSymbolsRounded-Regular.ttf"
        ))
        .into(),
    );
    definitions.families.insert(
        FontFamily::Name(MATERIAL_SYMBOLS.into()),
        vec![MATERIAL_SYMBOLS.into()],
    );
    let mut names = vec!["Monospace".into()];
    let mut faces: Vec<_> = database
        .faces()
        .filter(|face| is_terminal_font(database, face))
        .collect();
    // Prefer a normal, regular-weight face for each family. Sorting also makes the
    // picker order stable when a family has multiple files or collection faces.
    faces.sort_by_key(|face| {
        (
            face.families.first().map(|family| family.0.to_lowercase()),
            face.style != fontdb::Style::Normal,
            face.weight.0.abs_diff(400),
            face.stretch != fontdb::Stretch::Normal,
            face.post_script_name.clone(),
        )
    });
    for face in faces {
        let Some((name, _)) = face.families.first() else {
            continue;
        };
        if names.contains(name) {
            continue;
        }
        let Some(data) = database.with_face_data(face.id, |bytes, index| {
            let mut data = FontData::from_owned(bytes.to_vec());
            data.index = index;
            data
        }) else {
            continue;
        };
        definitions.font_data.insert(name.clone(), data.into());
        let mut family = vec![name.clone()];
        family.extend(definitions.families[&FontFamily::Monospace].clone());
        definitions
            .families
            .insert(FontFamily::Name(name.clone().into()), family);
        names.push(name.clone());
    }
    (definitions, names)
}

fn is_terminal_font(database: &fontdb::Database, face: &fontdb::FaceInfo) -> bool {
    if face.monospaced {
        return true;
    }
    // Some fixed-width variable fonts and Nerd Fonts omit the fixed-pitch flag.
    // Recognize them by their printable ASCII advances rather than their name.
    database
        .with_face_data(face.id, |bytes, index| {
            let Ok(font) = ttf_parser::Face::parse(bytes, index) else {
                return false;
            };
            let Some(width) = font
                .glyph_index('M')
                .and_then(|glyph| font.glyph_hor_advance(glyph))
            else {
                return false;
            };
            width > 0
                && (' '..='~').all(|ch| {
                    font.glyph_index(ch)
                        .and_then(|glyph| font.glyph_hor_advance(glyph))
                        == Some(width)
                })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_in_nested_user_directories_are_discovered_and_rendered() {
        let directory = std::env::temp_dir().join(format!(
            "rs-button-builder-font-test-{}",
            std::process::id()
        ));
        let nested = directory.join("fonts/custom-family");
        std::fs::create_dir_all(&nested).unwrap();
        let builtin = FontDefinitions::default();
        let data = &builtin.font_data[&builtin.families[&FontFamily::Monospace][0]];
        let mut user_font = data.font.to_vec();
        let post_offset = {
            let face = ttf_parser::Face::parse(&user_font, data.index).unwrap();
            let post = face
                .raw_face()
                .table(ttf_parser::Tag::from_bytes(b"post"))
                .unwrap();
            post.as_ptr() as usize - user_font.as_ptr() as usize
        };
        // Reproduce fonts that have fixed widths but no fixed-pitch metadata flag.
        user_font[post_offset + 12..post_offset + 16].fill(0);
        std::fs::write(nested.join("user-font.ttf"), user_font).unwrap();
        std::fs::write(nested.join("invalid.ttf"), b"not a font").unwrap();
        let mut database = fontdb::Database::new();
        database.load_fonts_dir(&directory);
        assert!(database.faces().all(|face| !face.monospaced));
        let (definitions, names) = definitions(&database);
        assert_eq!(
            names.len(),
            2,
            "The user font must appear beside built-in Monospace"
        );
        let ctx = egui::Context::default();
        ctx.set_fonts(definitions);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let font = egui::FontId::new(16.0, FontFamily::Name(names[1].clone().into()));
            ctx.fonts_mut(|fonts| {
                let width = fonts.glyph_width(&font, 'M');
                assert!(width > 0.0);
                assert!((width - fonts.glyph_width(&font, 'i')).abs() < 0.01);
            });
        });
    }

    #[test]
    fn every_discovered_font_family_can_render_terminal_text() {
        let ctx = egui::Context::default();
        let names = load(&ctx);
        println!("Available terminal fonts: {}", names.join(", "));
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for name in &names {
                let family = if name == "Monospace" {
                    FontFamily::Monospace
                } else {
                    FontFamily::Name(name.clone().into())
                };
                let font = egui::FontId::new(16.0, family);
                ctx.fonts_mut(|fonts| {
                    let galley =
                        fonts.layout_no_wrap("MWi0 café".into(), font, egui::Color32::WHITE);
                    assert!(
                        galley.size().x.is_finite() && galley.size().x > 0.0,
                        "Could not render {name}"
                    );
                });
            }
        });
    }
}

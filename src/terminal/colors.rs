//! Terminal colors are resolved when painting so existing output follows theme changes.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Indexed(u8),
    Rgb([u8; 3]),
}

pub struct Palette {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    ansi: &'static [[u8; 3]; 16],
}

impl Palette {
    pub fn new(dark: bool, foreground: Option<[u8; 3]>, background: Option<[u8; 3]>) -> Self {
        // Custom backgrounds determine which ANSI palette and default text stay readable.
        let dark = background.map_or(dark, |[r, g, b]| {
            299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b) < 128_000
        });
        Self {
            foreground: foreground.unwrap_or(if dark { [220, 227, 238] } else { [31, 37, 46] }),
            background: background.unwrap_or(if dark { [15, 19, 27] } else { [247, 249, 252] }),
            ansi: if dark { &DARK_ANSI } else { &LIGHT_ANSI },
        }
    }

    pub fn resolve(&self, color: Color) -> [u8; 3] {
        match color {
            Color::Rgb(rgb) => rgb,
            Color::Indexed(index @ 0..=15) => self.ansi[usize::from(index)],
            Color::Indexed(index @ 16..=231) => {
                let n = index - 16;
                let channel = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
                [channel(n / 36), channel((n / 6) % 6), channel(n % 6)]
            }
            Color::Indexed(index) => [8 + 10 * (index - 232); 3],
        }
    }
}

const DARK_ANSI: [[u8; 3]; 16] = [
    [0, 0, 0],
    [205, 49, 49],
    [13, 188, 121],
    [229, 229, 16],
    [36, 114, 200],
    [188, 63, 188],
    [17, 168, 205],
    [229, 229, 229],
    [102, 102, 102],
    [241, 76, 76],
    [35, 209, 139],
    [245, 245, 67],
    [59, 142, 234],
    [214, 112, 214],
    [41, 184, 219],
    [255, 255, 255],
];

const LIGHT_ANSI: [[u8; 3]; 16] = [
    [31, 37, 46],
    [166, 32, 42],
    [28, 111, 66],
    [126, 91, 0],
    [32, 83, 164],
    [137, 49, 143],
    [0, 105, 125],
    [92, 99, 110],
    [89, 99, 110],
    [184, 39, 51],
    [32, 120, 70],
    [139, 99, 0],
    [40, 95, 181],
    [150, 57, 155],
    [0, 116, 136],
    [64, 71, 82],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_palette_is_readable_and_extended_colors_are_theme_independent() {
        let dark = Palette::new(true, None, None);
        let light = Palette::new(false, None, None);
        let luminance = |[r, g, b]: [u8; 3]| {
            let color = egui::Rgba::from(egui::Color32::from_rgb(r, g, b));
            0.2126 * color.r() + 0.7152 * color.g() + 0.0722 * color.b()
        };
        for foreground in std::iter::once(light.foreground).chain(LIGHT_ANSI) {
            let contrast = (luminance(light.background) + 0.05) / (luminance(foreground) + 0.05);
            assert!(contrast >= 4.5, "{foreground:?}: {contrast}");
        }
        for index in 16..=255 {
            assert_eq!(
                dark.resolve(Color::Indexed(index)),
                light.resolve(Color::Indexed(index))
            );
        }
        for (index, expected) in [
            (16, [0, 0, 0]),
            (196, [255, 0, 0]),
            (231, [255, 255, 255]),
            (232, [8; 3]),
            (255, [238; 3]),
        ] {
            assert_eq!(light.resolve(Color::Indexed(index)), expected);
        }
        assert_eq!(light.resolve(Color::Rgb([205, 49, 49])), [205, 49, 49]);
        assert_ne!(light.resolve(Color::Indexed(1)), [205, 49, 49]);
    }

    #[test]
    fn custom_colors_are_preserved_and_background_selects_the_ansi_palette() {
        for dark in [false, true] {
            let custom = Palette::new(dark, Some([210, 220, 230]), Some([5, 10, 15]));
            assert_eq!(custom.foreground, [210, 220, 230]);
            assert_eq!(custom.background, [5, 10, 15]);
            assert_eq!(custom.resolve(Color::Indexed(2)), DARK_ANSI[2]);
            let custom_light = Palette::new(dark, None, Some([255; 3]));
            assert_eq!(
                custom_light.foreground,
                Palette::new(false, None, None).foreground
            );
            assert_eq!(custom_light.resolve(Color::Indexed(2)), LIGHT_ANSI[2]);
        }
    }
}

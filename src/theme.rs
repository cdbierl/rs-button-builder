//! Persisted UI theme choices, independent of button and terminal appearance.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSettings {
    pub mode: Appearance,
    pub accent: Accent,
}

impl ThemeSettings {
    pub fn is_dark(self, system: Option<egui::Theme>) -> bool {
        match self.mode {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::System => system.unwrap_or(egui::Theme::Dark) == egui::Theme::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Light,
    #[default]
    Dark,
    System,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Orange,
    Blue,
    Green,
    Purple,
    Rose,
}

impl Accent {
    pub const ALL: [Self; 5] = [
        Self::Orange,
        Self::Blue,
        Self::Green,
        Self::Purple,
        Self::Rose,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Orange => "Orange",
            Self::Blue => "Blue",
            Self::Green => "Green",
            Self::Purple => "Purple",
            Self::Rose => "Rose",
        }
    }

    /// Material primary, on-primary, container, and on-container roles.
    pub fn colors(self, dark: bool) -> [[u8; 3]; 4] {
        match (self, dark) {
            (Self::Orange, true) => [[255, 183, 123], [79, 37, 0], [112, 55, 0], [255, 220, 193]],
            (Self::Orange, false) => [[143, 76, 0], [255, 255, 255], [255, 220, 193], [47, 21, 0]],
            (Self::Blue, true) => [[168, 199, 250], [6, 46, 111], [8, 66, 160], [211, 227, 253]],
            (Self::Blue, false) => [[11, 87, 208], [255, 255, 255], [211, 227, 253], [4, 30, 73]],
            (Self::Green, true) => [[173, 210, 142], [27, 55, 4], [49, 79, 25], [200, 238, 168]],
            (Self::Green, false) => [[72, 103, 47], [255, 255, 255], [200, 238, 168], [11, 32, 0]],
            (Self::Purple, true) => [
                [208, 188, 255],
                [56, 30, 114],
                [79, 55, 139],
                [234, 221, 255],
            ],
            (Self::Purple, false) => [
                [103, 80, 164],
                [255, 255, 255],
                [234, 221, 255],
                [33, 0, 93],
            ],
            (Self::Rose, true) => [
                [255, 177, 200],
                [95, 17, 52],
                [124, 41, 75],
                [255, 217, 226],
            ],
            (Self::Rose, false) => [[154, 64, 99], [255, 255, 255], [255, 217, 226], [62, 0, 29]],
        }
    }
}

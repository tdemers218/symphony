//! Palettes. Strokes point at swatches by ID, so editing a swatch
//! recolours every stroke that uses it (E2.3).

use serde::{Deserialize, Serialize};

use crate::id::Id;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub id: Id,
    pub name: String,
    /// sRGB (E10.4).
    pub rgb: [u8; 3],
    #[serde(default = "opaque")]
    pub opacity: u8,
    /// Harmony's Protect Colour: while Respect Protected Colours is on,
    /// painting never repaints or unpaints what uses this swatch.
    #[serde(default)]
    pub protected: bool,
    /// Behaviours that vary the colour as it's painted (dynamic swatches).
    /// `None` is a plain colour, and is what every swatch saved before
    /// dynamics loads as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamics: Option<Box<crate::dynamics::Dynamics>>,
}

fn opaque() -> u8 {
    255
}

impl Swatch {
    pub fn rgba(&self) -> [u8; 4] {
        let [r, g, b] = self.rgb;
        [r, g, b, self.opacity]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub swatches: Vec<Swatch>,
}

impl Palette {
    pub fn get(&self, id: Id) -> Option<&Swatch> {
        self.swatches.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: Id) -> Option<&mut Swatch> {
        self.swatches.iter_mut().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swatches_saved_before_protection_load_unprotected() {
        let old = r#"{"id":7,"name":"Ink","rgb":[0,0,0]}"#;
        let sw: Swatch = serde_json::from_str(old).unwrap();
        assert!(!sw.protected);
        assert_eq!(sw.opacity, 255);
        assert!(sw.dynamics.is_none());
        // A plain swatch saves exactly as before.
        assert!(!serde_json::to_string(&sw).unwrap().contains("dynamics"));
    }
}

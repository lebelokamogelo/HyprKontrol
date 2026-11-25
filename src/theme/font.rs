//! Keeps text in the family a theme asks for.
//!
//! Text is matched by exact weight before family. When the family has no face
//! of the requested weight, the match moves on to a different family that does,
//! and a medium label ends up in another typeface than the text around it.
//!
//! Two common cases run into this. A variable font registers a single face, at
//! its default weight, although it can draw all of them; Adwaita Sans and Inter
//! are shipped that way. And many monospaced families have no medium at all.
//!
//! Registering the nearest face again under the missing weight settles both.
//! The renderer applies the weight axis where there is one, and otherwise draws
//! the face as it is.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use iced::advanced::graphics::text::font_system;

/// Weights the interface uses besides regular.
const WEIGHTS: [u16; 3] = [500, 600, 700];

/// Makes every weight of `family` available. Repeated calls cost nothing.
pub fn register_weights(family: &str) {
    static DONE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut done = DONE.get_or_init(Default::default).lock().expect("font registry poisoned");
    if !done.insert(family.to_owned()) {
        return;
    }

    let mut system = font_system().write().expect("font system poisoned");
    let database = system.raw().db_mut();
    let faces: Vec<_> = database
        .faces()
        .filter(|face| face.families.iter().any(|(name, _)| name == family))
        .cloned()
        .collect();
    for face in &faces {
        let siblings: Vec<u16> = faces
            .iter()
            .filter(|known| known.style == face.style && known.stretch == face.stretch)
            .map(|known| known.weight.0)
            .collect();
        for weight in WEIGHTS {
            if !siblings.contains(&weight) && nearest(weight, &siblings) == Some(face.weight.0) {
                let mut copy = face.clone();
                copy.weight.0 = weight;
                database.push_face_info(copy);
            }
        }
    }
}

/// The available weight closest to `wanted`, the heavier one on a tie.
fn nearest(wanted: u16, available: &[u16]) -> Option<u16> {
    available
        .iter()
        .copied()
        .min_by_key(|weight| (weight.abs_diff(wanted), std::cmp::Reverse(*weight)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_closest_weight() {
        assert_eq!(nearest(500, &[400, 700]), Some(400));
        assert_eq!(nearest(600, &[400, 700]), Some(700));
        assert_eq!(nearest(600, &[500, 700]), Some(700));
        assert_eq!(nearest(700, &[400]), Some(400));
        assert_eq!(nearest(500, &[]), None);
    }
}

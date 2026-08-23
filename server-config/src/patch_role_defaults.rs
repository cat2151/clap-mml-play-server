//! Plugin-specific defaults for the TUI's abstract patch roles.
//!
//! The server repository owns these values because they describe the preset
//! taxonomy exposed by an audio plugin.  Clients receive only the resolved
//! role filters and never need to identify a concrete plugin.

use crate::{patch_form_of, PatchForm, PatchRoleFilters, SURGE_XT_PLUGIN_ID};

pub const SURGE_CHORD_CATEGORIES: [&str; 4] = ["Keys", "Organs", "Pads", "Polysynths"];
pub const SURGE_BASS_CATEGORIES: [&str; 1] = ["Basses"];
pub const SURGE_ARPEGGIO_CATEGORIES: [&str; 10] = [
    "Bells", "Brass", "Guitars", "Keys", "Leads", "Mallets", "Modelled", "MPE", "Organs", "Plucks",
];
pub const SURGE_DRUM_CATEGORIES: [&str; 2] = ["Percussion", "Drums"];
pub const KICK_KEYWORDS: [&str; 2] = ["kick", "bass drum"];
pub const SNARE_KEYWORDS: [&str; 3] = ["snare", "rimshot", "clap"];
pub const HIHAT_KEYWORDS: [&str; 3] = ["hat", "hi-hat", "hihat"];

pub const VAPORIZER2_CHORD_CATEGORIES: [&str; 5] = ["Pad", "Chord", "Organ", "Synth", "Atmosphere"];
pub const VAPORIZER2_BASS_CATEGORIES: [&str; 1] = ["Bass"];
pub const VAPORIZER2_ARPEGGIO_CATEGORIES: [&str; 5] =
    ["Arpeggio", "Plucked", "Bell", "Mallet", "Trancegate"];
pub const VAPORIZER2_DRUM_CATEGORIES: [&str; 2] = ["Drum", "Drum kit"];

/// Category-code labels used by the Vaporizer2 adapter.
pub const VAPORIZER2_CATEGORY_CODES: [(&str, &str); 28] = [
    ("AR", "Arpeggio"),
    ("AT", "Atmosphere"),
    ("BA", "Bass"),
    ("BL", "Bell"),
    ("BR", "Brass"),
    ("CH", "Chord"),
    ("DK", "Drum kit"),
    ("DL", "Drum loop"),
    ("DR", "Drum"),
    ("FX", "Effect"),
    ("GT", "Guitar"),
    ("IN", "Instrument"),
    ("KB", "Keyboard"),
    ("LD", "Lead"),
    ("MA", "Mallet"),
    ("OC", "Orchestral"),
    ("OR", "Organ"),
    ("PD", "Pad"),
    ("PL", "Plucked"),
    ("PN", "Piano"),
    ("RD", "Reed"),
    ("RI", "Riser"),
    ("SQ", "Sequence"),
    ("ST", "String"),
    ("SY", "Synth"),
    ("TG", "Trancegate"),
    ("VC", "Vocal"),
    ("WW", "Woodwind"),
];

/// Resolve the built-in role filters for one plugin profile.
///
/// Unknown plugins intentionally default to no filtering.  A new adapter can
/// add its taxonomy here without requiring a client release.
pub fn builtin_patch_role_filters(plugin_id: Option<&str>, plugin_path: &str) -> PatchRoleFilters {
    if is_surge(plugin_id, plugin_path) {
        filters(
            &SURGE_CHORD_CATEGORIES,
            &SURGE_BASS_CATEGORIES,
            &SURGE_ARPEGGIO_CATEGORIES,
            &SURGE_DRUM_CATEGORIES,
        )
    } else if patch_form_of(plugin_id, plugin_path) == PatchForm::Vvp {
        filters(
            &VAPORIZER2_CHORD_CATEGORIES,
            &VAPORIZER2_BASS_CATEGORIES,
            &VAPORIZER2_ARPEGGIO_CATEGORIES,
            &VAPORIZER2_DRUM_CATEGORIES,
        )
    } else {
        PatchRoleFilters::unfiltered()
    }
}

fn is_surge(plugin_id: Option<&str>, plugin_path: &str) -> bool {
    match plugin_id {
        Some(id) => id == SURGE_XT_PLUGIN_ID,
        None => crate::plugin_file_stem(plugin_path)
            .to_ascii_lowercase()
            .contains("surge"),
    }
}

fn filters(chord: &[&str], bass: &[&str], arpeggio: &[&str], drum: &[&str]) -> PatchRoleFilters {
    PatchRoleFilters {
        chord_patch_categories: Some(owned(chord)),
        bass_patch_categories: Some(owned(bass)),
        arpeggio_patch_categories: Some(owned(arpeggio)),
        drum_patch_categories: Some(owned(drum)),
        kick_patch_keywords: Some(owned(&KICK_KEYWORDS)),
        snare_patch_keywords: Some(owned(&SNARE_KEYWORDS)),
        hihat_patch_keywords: Some(owned(&HIHAT_KEYWORDS)),
    }
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[cfg(test)]
mod tests;

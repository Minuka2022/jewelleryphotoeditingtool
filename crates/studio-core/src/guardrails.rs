//! Rejects wording that would ask the model to change the jewelry itself
//! instead of the scene around it. Applied to style prompts and user notes.
//!
//! The master prompt is not checked: it legitimately names these things in
//! order to forbid them.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Viewpoint,
    Framing,
    OnBody,
    DesignChange,
}

impl RuleKind {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Viewpoint => "changes the viewing angle or position of the item",
            Self::Framing => "crops or re-frames the item",
            Self::OnBody => "shows the item worn or on a body",
            Self::DesignChange => "changes the item's design",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub phrase: &'static str,
    pub kind: RuleKind,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\" {}", self.phrase, self.kind.reason())
    }
}

/// Phrases are matched as whole words, ignoring case and punctuation, so
/// "neck" does not match "necklace" and "top-down" matches "top down".
const RULES: &[(RuleKind, &[&str])] = &[
    (
        RuleKind::Viewpoint,
        &[
            "rotate",
            "rotated",
            "rotating",
            "rotation",
            "flip",
            "flipped",
            "mirror image",
            "tilt",
            "tilted",
            "tilting",
            "turned",
            "upside down",
            "side view",
            "side angle",
            "side profile",
            "from the side",
            "top view",
            "top down",
            "birds eye",
            "overhead view",
            "overhead shot",
            "viewed from above",
            "seen from above",
            "shot from above",
            "view from above",
            "viewed from below",
            "back view",
            "rear view",
            "from behind",
            "different angle",
            "new angle",
            "another angle",
            "change the angle",
            "three quarter view",
            "flat lay",
            "flatlay",
            "standing upright",
            "lying flat",
            "laid flat",
            "propped up",
            "leaning against",
            "hanging from",
            "draped over",
            "draped on",
            "wrapped around",
        ],
    ),
    (
        RuleKind::Framing,
        &[
            "zoom",
            "zoomed",
            "zooming",
            "extreme close up",
            "crop",
            "cropped",
            "cropping",
            "detail shot",
            "partially visible",
        ],
    ),
    (
        RuleKind::OnBody,
        &[
            "hand",
            "hands",
            "finger",
            "fingers",
            "wrist",
            "wrists",
            "neck",
            "ear",
            "ears",
            "earlobe",
            "skin",
            "arm",
            "body",
            "collarbone",
            "worn",
            "wear",
            "wears",
            "wearing",
            "model",
            "mannequin",
            "bust",
            "person",
            "woman",
            "man",
            "girl",
            "bride",
        ],
    ),
    (
        RuleKind::DesignChange,
        &[
            "add diamonds",
            "add a diamond",
            "add stones",
            "add a stone",
            "add gems",
            "add gemstones",
            "more diamonds",
            "more stones",
            "extra diamonds",
            "extra stones",
            "bigger stone",
            "larger stone",
            "smaller stone",
            "bigger diamond",
            "larger diamond",
            "smaller diamond",
            "replace the stone",
            "remove the stone",
            "change the stone",
            "change the metal",
            "change the design",
            "change the shape",
            "change the colour of the",
            "change the color of the",
            "make it gold",
            "make it silver",
            "make it rose gold",
            "make it platinum",
            "thicker band",
            "thinner band",
            "resize",
            "reshape",
            "redesign",
            "restyle",
            "recolor",
            "recolour",
            "engrave",
            "straighten",
            "simplify",
        ],
    ),
];

/// Returns every banned phrase found in `text`. An empty result means the
/// text is safe to send.
pub fn check(text: &str) -> Vec<Violation> {
    let words = tokenize(text);
    let mut found = Vec::new();
    for &(kind, phrases) in RULES {
        for &phrase in phrases {
            if contains_sequence(&words, &tokenize(phrase)) {
                found.push(Violation { phrase, kind });
            }
        }
    }
    found
}

/// One line listing the violations, for error messages.
pub fn describe(violations: &[Violation]) -> String {
    violations
        .iter()
        .map(Violation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Words that describe a piece of jewelry rather than a scene. A style prompt
/// that says "gold ring with a diamond" can pull the result towards that
/// description and away from the customer's item.
const ITEM_WORDS: &[&str] = &[
    "ring",
    "rings",
    "necklace",
    "necklaces",
    "earring",
    "earrings",
    "pendant",
    "pendants",
    "bracelet",
    "bracelets",
    "bangle",
    "bangles",
    "brooch",
    "jewelry",
    "jewellery",
    "gold",
    "golden",
    "silver",
    "platinum",
    "diamond",
    "diamonds",
    "ruby",
    "rubies",
    "sapphire",
    "sapphires",
    "emerald",
    "emeralds",
    "pearl",
    "pearls",
    "gemstone",
    "gemstones",
    "carat",
    "karat",
];

/// Words in a style prompt that may describe the jewelry itself. Unlike
/// [`check`], these are advice, not errors: "gold" can also be a light colour.
pub fn item_words(text: &str) -> Vec<&'static str> {
    let words = tokenize(text);
    ITEM_WORDS
        .iter()
        .copied()
        .filter(|candidate| words.iter().any(|word| word == candidate))
        .collect()
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['\'', '\u{2019}'], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

fn contains_sequence(words: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && words.windows(needle.len()).any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrases(text: &str) -> Vec<&'static str> {
        check(text).into_iter().map(|v| v.phrase).collect()
    }

    #[test]
    fn words_describing_the_jewelry_are_pointed_out() {
        let prompt = "A gold ring with Diamonds on beige silk, soft morning light";
        assert_eq!(item_words(prompt), vec!["ring", "gold", "diamonds"]);
        assert!(item_words("Place the {item} on beige silk, soft light").is_empty());
    }

    #[test]
    fn scene_descriptions_pass() {
        let scene = "Place the necklace on rich black velvet. A focused spotlight from \
                     above-left creates sparkle. A mirrored tray sits beside the earrings.";
        assert_eq!(phrases(scene), Vec::<&str>::new());
    }

    #[test]
    fn viewpoint_changes_are_caught() {
        assert_eq!(phrases("Side view of the ring"), vec!["side view"]);
        assert_eq!(phrases("a Top-Down shot"), vec!["top down"]);
        assert_eq!(phrases("bird's-eye photo"), vec!["birds eye"]);
        assert_eq!(phrases("Rotate it slightly"), vec!["rotate"]);
    }

    #[test]
    fn on_body_wording_is_caught() {
        assert_eq!(
            phrases("a woman wearing it on her hand"),
            vec!["hand", "wearing", "woman"]
        );
    }

    #[test]
    fn whole_words_only() {
        assert!(check("necklace earrings handcrafted armchair manicured").is_empty());
    }

    #[test]
    fn design_changes_are_caught() {
        assert_eq!(
            phrases("please add diamonds around it"),
            vec!["add diamonds"]
        );
        assert_eq!(phrases("Make it rose gold"), vec!["make it rose gold"]);
    }

    #[test]
    fn violations_explain_themselves() {
        let text = describe(&check("zoom in"));
        assert_eq!(text, "\"zoom\" crops or re-frames the item");
    }
}

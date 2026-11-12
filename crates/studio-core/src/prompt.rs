//! Builds the text sent to the model from the four prompt layers:
//! global rules, category focus, style scene and an optional user note.

use serde::Serialize;

use crate::library::{JewelryCategory, Look, MasterPrompts, StyleTemplate, ITEM_PLACEHOLDER};

pub struct PromptRequest<'a> {
    pub master: &'a MasterPrompts,
    pub category: &'a JewelryCategory,
    pub style: &'a StyleTemplate,
    /// How many reference photos will be attached.
    pub reference_count: usize,
    pub user_note: Option<&'a str>,
    /// What the piece looks like, written after studying every reference photo.
    pub design_notes: Option<&'a str>,
}

/// The request that asks the AI to study the reference photos and describe
/// the piece. Its answer becomes the piece's design notes.
pub fn analysis_prompt(
    master: &MasterPrompts,
    category: &JewelryCategory,
    reference_count: usize,
) -> String {
    let mut text = master
        .analysis
        .prompt
        .trim()
        .replace(ITEM_PLACEHOLDER, category.noun.trim());
    if reference_count > 1 {
        text.push_str(&format!(
            "\n\nThere are {reference_count} photos. Combine what each one shows; \
             details hidden in one photo may be visible in another."
        ));
    }
    text.push_str(&format!(
        "\n\nDetails that matter most for this kind of piece: {}",
        category.focus.trim()
    ));
    text
}

/// The finished prompt, with the versions of the wording that produced it.
#[derive(Debug, Clone, Serialize)]
pub struct BuiltPrompt {
    pub text: String,
    pub master_version: u32,
    pub category_id: String,
    pub style_id: String,
    pub style_version: u32,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromptError {
    #[error("at least one reference photo is required")]
    NoReference,
    #[error("style \"{style}\" is not set up for category \"{category}\"")]
    StyleNotForCategory { style: String, category: String },
}

pub fn build_prompt(request: &PromptRequest<'_>) -> Result<BuiltPrompt, PromptError> {
    let PromptRequest {
        master,
        category,
        style,
        ..
    } = *request;

    if request.reference_count == 0 {
        return Err(PromptError::NoReference);
    }
    if !style.supports(&category.id) {
        return Err(PromptError::StyleNotForCategory {
            style: style.id.clone(),
            category: category.id.clone(),
        });
    }
    // The note is sent exactly as typed.
    let note = request
        .user_note
        .map(str::trim)
        .filter(|note| !note.is_empty());

    let fill = |text: &str| text.trim().replace(ITEM_PLACEHOLDER, category.noun.trim());
    let global = &master.global;
    let task = if request.reference_count > 1 {
        &global.task_multi
    } else {
        &global.task
    };

    let mut text = format!("{}\n\n{}", fill(&global.role), fill(task));

    // The style decides the last rule: a real photograph or a flawless render.
    let finish = match style.look {
        Look::Photo => &global.finish_photo,
        Look::Render => &global.finish_render,
    };
    text.push_str("\n\nRULES (these override everything else in this request):");
    for (index, rule) in global.rules.iter().chain([finish]).enumerate() {
        text.push_str(&format!("\n{}. {}", index + 1, fill(rule)));
    }

    text.push_str("\n\nDETAILS TO PROTECT:\n");
    text.push_str(&fill(&category.focus));

    let notes = request
        .design_notes
        .map(str::trim)
        .filter(|notes| !notes.is_empty());
    if let Some(notes) = notes {
        text.push_str(
            "\n\nDESIGN NOTES (written after studying every reference photo; \
             where a note and a photo differ, follow the photo):\n",
        );
        text.push_str(notes);
    }

    text.push_str("\n\nSCENE:\n");
    text.push_str(&fill(&style.prompt));

    if let Some(note) = note {
        text.push_str(
            "\n\nEXTRA NOTE (applies to the scene only and never overrides the rules):\n",
        );
        text.push_str(note);
    }

    let avoid: Vec<&str> = global
        .avoid
        .iter()
        .chain(&style.avoid)
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .collect();
    if !avoid.is_empty() {
        // These are defaults: the user's scene or note can ask for any of them.
        text.push_str("\n\nAVOID (unless the scene or the note asks for it): ");
        text.push_str(&avoid.join("; "));
        text.push('.');
    }

    Ok(BuiltPrompt {
        text,
        master_version: master.version,
        category_id: category.id.clone(),
        style_id: style.id.clone(),
        style_version: style.version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::shipped_library;

    #[test]
    fn layers_appear_in_order_with_the_item_filled_in() {
        let library = shipped_library();
        let prompt = build_prompt(&PromptRequest {
            master: &library.master,
            category: library.category("ring").unwrap(),
            style: library.style("black-velvet").unwrap(),
            reference_count: 1,
            user_note: Some("  slightly warmer light  "),
            design_notes: None,
        })
        .unwrap();

        let text = &prompt.text;
        let position = |needle: &str| {
            text.find(needle)
                .unwrap_or_else(|| panic!("missing {needle:?} in:\n{text}"))
        };
        let order = [
            position("professional jewelry product photographer"),
            position("RULES"),
            position("1. Reproduce the ring exactly"),
            position("DETAILS TO PROTECT"),
            position("SCENE"),
            position("Place the ring on rich black velvet"),
            position("EXTRA NOTE"),
            position("slightly warmer light"),
            position("AVOID (unless the scene or the note asks for it): "),
        ];
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{text}");
        assert!(!text.contains('{') && !text.contains('}'), "{text}");
        assert_eq!(prompt.style_id, "black-velvet");
        assert_eq!(prompt.style_version, 1);
    }

    #[test]
    fn every_shipped_style_builds_for_every_category_it_supports() {
        let library = shipped_library();
        for style in &library.styles {
            for category in &library.master.categories {
                if !style.supports(&category.id) {
                    continue;
                }
                let prompt = build_prompt(&PromptRequest {
                    master: &library.master,
                    category,
                    style,
                    reference_count: 1,
                    user_note: None,
                    design_notes: None,
                })
                .unwrap();
                assert!(prompt.text.contains(&category.noun));
                assert!(!prompt.text.contains('{'), "{}", prompt.text);
                assert!(!prompt.text.contains("EXTRA NOTE"));
            }
        }
    }

    #[test]
    fn the_last_rule_follows_the_styles_look() {
        let library = shipped_library();
        let mut style = library.style("studio-white").unwrap().clone();
        let build = |style: &StyleTemplate| {
            build_prompt(&PromptRequest {
                master: &library.master,
                category: library.category("ring").unwrap(),
                style,
                reference_count: 1,
                user_note: None,
                design_notes: None,
            })
            .unwrap()
            .text
        };
        let last_rule = library.master.global.rules.len() + 1;

        let photo = build(&style);
        assert!(photo.contains(&format!(
            "{last_rule}. The result must look like a real photograph"
        )));
        assert!(!photo.contains("3D CAD jewelry render"));

        style.look = Look::Render;
        let render = build(&style);
        assert!(render.contains(&format!(
            "{last_rule}. The result must look like a flawless high-end 3D CAD jewelry render"
        )));
        assert!(!render.contains("must look like a real photograph"));
        // Whatever the look, the design itself stays protected.
        assert!(render.contains("1. Reproduce the ring exactly"));
        assert!(render.contains("The design of the ring must still match the reference exactly"));
    }

    #[test]
    fn design_notes_follow_the_details_to_protect() {
        let library = shipped_library();
        let notes =
            "Yellow gold band; oval blue centre stone with 4 prongs; halo of 12 round diamonds.";
        let prompt = build_prompt(&PromptRequest {
            master: &library.master,
            category: library.category("ring").unwrap(),
            style: library.style("studio-white").unwrap(),
            reference_count: 3,
            user_note: None,
            design_notes: Some(notes),
        })
        .unwrap()
        .text;
        let protect = prompt.find("DETAILS TO PROTECT").unwrap();
        let design = prompt.find("DESIGN NOTES").unwrap();
        let scene = prompt.find("SCENE:").unwrap();
        assert!(protect < design && design < scene, "{prompt}");
        assert!(prompt.contains(notes));
        assert!(prompt.contains("where a note and a photo differ, follow the photo"));

        let without = build_prompt(&PromptRequest {
            master: &library.master,
            category: library.category("ring").unwrap(),
            style: library.style("studio-white").unwrap(),
            reference_count: 1,
            user_note: None,
            design_notes: Some("   "),
        })
        .unwrap()
        .text;
        assert!(!without.contains("DESIGN NOTES"));
    }

    #[test]
    fn the_analysis_request_names_the_piece_and_counts_the_photos() {
        let library = shipped_library();
        let ring = library.category("ring").unwrap();
        let several = analysis_prompt(&library.master, ring, 4);
        assert!(several.contains("the same ring"), "{several}");
        assert!(several.contains("There are 4 photos."));
        assert!(several.contains("prongs"));
        assert!(!several.contains("{item}"));
        assert!(!analysis_prompt(&library.master, ring, 1).contains("There are"));
    }

    #[test]
    fn several_references_use_the_multi_photo_task() {
        let library = shipped_library();
        let prompt = build_prompt(&PromptRequest {
            master: &library.master,
            category: library.category("pendant").unwrap(),
            style: library.style("studio-white").unwrap(),
            reference_count: 3,
            user_note: None,
            design_notes: None,
        })
        .unwrap();
        assert!(prompt
            .text
            .contains("The FIRST photo is the primary reference"));
    }

    #[test]
    fn a_note_is_sent_exactly_as_typed() {
        let library = shipped_library();
        let note = "show it on a model's hand, side view, gold rim light";
        let prompt = build_prompt(&PromptRequest {
            master: &library.master,
            category: library.category("ring").unwrap(),
            style: library.style("studio-white").unwrap(),
            reference_count: 1,
            user_note: Some(note),
            design_notes: None,
        })
        .unwrap();
        assert!(prompt.text.contains(note), "{}", prompt.text);
    }

    #[test]
    fn a_style_limited_to_other_categories_is_refused() {
        let library = shipped_library();
        let mut style = library.style("studio-white").unwrap().clone();
        style.categories = vec!["necklace".into()];
        let error = build_prompt(&PromptRequest {
            master: &library.master,
            category: library.category("ring").unwrap(),
            style: &style,
            reference_count: 1,
            user_note: None,
            design_notes: None,
        })
        .unwrap_err();
        assert!(matches!(error, PromptError::StyleNotForCategory { .. }));
    }
}

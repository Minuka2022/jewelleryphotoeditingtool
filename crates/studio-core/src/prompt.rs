//! Builds the text sent to the model from the four prompt layers:
//! global rules, category focus, style scene and an optional user note.

use serde::Serialize;

use crate::guardrails;
use crate::library::{JewelryCategory, Look, MasterPrompts, StyleTemplate, ITEM_PLACEHOLDER};

pub struct PromptRequest<'a> {
    pub master: &'a MasterPrompts,
    pub category: &'a JewelryCategory,
    pub style: &'a StyleTemplate,
    /// How many reference photos will be attached.
    pub reference_count: usize,
    pub user_note: Option<&'a str>,
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
    #[error("the note is not allowed: {0}. A note may adjust only the scene around the item.")]
    NoteRejected(String),
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
    let note = request
        .user_note
        .map(str::trim)
        .filter(|note| !note.is_empty());
    if let Some(note) = note {
        let violations = guardrails::check(note);
        if !violations.is_empty() {
            return Err(PromptError::NoteRejected(guardrails::describe(&violations)));
        }
    }

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
        text.push_str("\n\nAVOID: ");
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
            position("AVOID: "),
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
    fn several_references_use_the_multi_photo_task() {
        let library = shipped_library();
        let prompt = build_prompt(&PromptRequest {
            master: &library.master,
            category: library.category("pendant").unwrap(),
            style: library.style("studio-white").unwrap(),
            reference_count: 3,
            user_note: None,
        })
        .unwrap();
        assert!(prompt
            .text
            .contains("The FIRST photo is the primary reference"));
    }

    #[test]
    fn a_note_that_changes_the_item_is_rejected() {
        let library = shipped_library();
        let error = build_prompt(&PromptRequest {
            master: &library.master,
            category: library.category("ring").unwrap(),
            style: library.style("studio-white").unwrap(),
            reference_count: 1,
            user_note: Some("rotate it and add diamonds"),
        })
        .unwrap_err();
        assert!(matches!(error, PromptError::NoteRejected(_)));
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
        })
        .unwrap_err();
        assert!(matches!(error, PromptError::StyleNotForCategory { .. }));
    }
}

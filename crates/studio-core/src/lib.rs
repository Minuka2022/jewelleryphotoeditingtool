//! Shared core of the jewelry studio: the editable prompt library, the
//! guardrails that keep prompts from asking for a different piece of jewelry,
//! reference-photo preparation, cost estimates and the Gemini client.

pub mod cost;
pub mod gemini;
pub mod guardrails;
pub mod library;
pub mod prompt;
pub mod reference;

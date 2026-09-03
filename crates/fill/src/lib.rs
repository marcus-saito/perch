//! Filling an application form, and stopping.
//!
//! # There is no submit
//!
//! Perch never submits an application. That is not a setting or a default
//! someone could relax: [`Action`] has no variant that activates a control.
//! The type this crate emits cannot express a click or a submit, so no caller
//! can ask for one.
//!
//! What it produces is a [`FillPlan`]: a list of values, each with a selector
//! and a note saying where the value came from. The person reads the plan.
//! Perch types those values into the page and stops. The form is left open,
//! filled in, with them in front of it.
//!
//! # Three kinds of empty, all on purpose
//!
//! * **Flagged**: Perch does not know the answer and will not guess.
//! * **Left to you**: a cover letter, or why you want to work there. Perch
//!   does not write these.
//! * **Never**: demographic and EEO questions. Perch does not fill them and
//!   does not store them. They are refused by label as well as by kind, so a
//!   field cannot slip through by being described as something else.

pub mod flavor;
pub mod plan;
pub mod script;

pub use flavor::{Flavor, FormField, Kind};
pub use plan::{Entry, FillPlan, Flagged, LeftToYou, Provenance, Refused};
pub use script::to_script;

/// Everything Perch is willing to do to a form.
///
/// Nothing here activates a control. A variant that did would break the
/// promise this crate exists to keep.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum Action {
    /// Type a value into a text input or textarea.
    ///
    /// `selector` is tried first and may be empty. `labels` are the words
    /// printed beside the box, for a board that names its boxes with an id
    /// minted per posting. Both are here because one board does each.
    SetText {
        selector: String,
        labels: Vec<String>,
        value: String,
    },
    /// Choose an option in a select, or set a radio or checkbox.
    SetChoice { selector: String, value: String },
    /// Attach a file the person already chose.
    AttachFile { selector: String, path: String },
}

impl Action {
    pub fn selector(&self) -> &str {
        match self {
            Action::SetText { selector, .. }
            | Action::SetChoice { selector, .. }
            | Action::AttachFile { selector, .. } => selector,
        }
    }

    /// What will be put there, for the plan's preview.
    pub fn shown_value(&self) -> &str {
        match self {
            Action::SetText { value, .. } | Action::SetChoice { value, .. } => value,
            Action::AttachFile { path, .. } => path,
        }
    }
}

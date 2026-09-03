//! Perch's optional model layer.
//!
//! No inference runs here. One OpenAI-compatible client covers Ollama, LM
//! Studio, llama.cpp's server, OpenRouter and Fireworks, and the only task is
//! reading a résumé into proposed profile fields.
//!
//! Nothing this crate produces is ever written anywhere. Every value comes back
//! as a proposal, anchored to the text it was read from, for a person to accept
//! one at a time. See [`verify`] for the part that makes that true.

pub mod client;
pub mod config;
pub mod document;
pub mod error;
pub mod extract;
pub mod verify;

pub use client::{Available, Client};
pub use config::{Consent, Model, Permission};
pub use document::Document;
pub use error::{Error, Result};
pub use extract::{Field, Proposal};
pub use verify::{Anchor, Source};

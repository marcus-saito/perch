//! Span verification: the constraint the whole feature rests on.
//!
//! A model asked to read a résumé will occasionally produce something
//! plausible that the document does not say. Perch checks every proposed value
//! back against the source text and refuses to offer anything that does not
//! anchor there.
//!
//! Schema-constrained output makes the model's replies easier to parse. It is
//! not the guarantee. This module is, whatever the endpoint on the other end
//! does. A hostile or broken model can waste the person's time. It cannot get
//! an invented value in front of them with an Accept button attached.
//!
//! Three outcomes, and the interface treats them differently:
//!
//! * [`Anchor::Exact`]: the document says this. Offered, and pre-accepted.
//! * [`Anchor::Loose`]: the document says something this was read *from*.
//!   Offered with the source shown, and never pre-accepted.
//! * [`Anchor::None`]: the document does not say this. **Not offered at all.**

/// Where a proposed value came from in the source document.
#[derive(Debug, Clone, PartialEq)]
pub enum Anchor {
    /// The value is in the document, give or take spacing and punctuation.
    Exact { span: (usize, usize) },
    /// The value overlaps the document but is not a quotation of it. The model
    /// normalised, reordered or abbreviated. Worth a look.
    Loose {
        span: (usize, usize),
        /// How much of the value was actually found, 0.0 to 1.0.
        overlap: f32,
        /// The characters are all there, but only as part of something larger:
        /// "dferreira.dev" sitting inside "dana@dferreira.dev". The document
        /// contains the text without asserting it as its own value.
        fragment: bool,
    },
    /// Nothing in the document supports this. Perch does not offer it.
    None,
}

impl Anchor {
    pub fn span(&self) -> Option<(usize, usize)> {
        match self {
            Anchor::Exact { span } | Anchor::Loose { span, .. } => Some(*span),
            Anchor::None => None,
        }
    }

    /// Whether the interface may show an Accept control for this at all.
    pub fn offerable(&self) -> bool {
        !matches!(self, Anchor::None)
    }

    /// Whether it may arrive already accepted. Only a quotation may.
    pub fn pre_accepted(&self) -> bool {
        matches!(self, Anchor::Exact { .. })
    }
}

/// A value must match at least this much of itself to count as loosely read.
const LOOSE_THRESHOLD: f32 = 0.6;

/// A single word can be found almost anywhere, so a one-word value has to be
/// quoted exactly or not offered. "Cloudflare" as an employer must really be
/// in the document; it must not be *nearly* in it.
const MIN_TOKENS_FOR_LOOSE: usize = 2;

/// The document, indexed so a match in the normalised form can be reported as
/// a span in the original text. Built once and reused across every field.
pub struct Source {
    original: String,
    /// Lowercased alphanumerics only, with everything else dropped.
    squashed: String,
    /// For each byte of `squashed`, the byte range of the character it came
    /// from in `original`. The range's own end matters: taking the *next*
    /// character's start instead would swallow whatever punctuation sat
    /// between them, and the quoted evidence would not be the match.
    offsets: Vec<(usize, usize)>,
    tokens: Vec<Token>,
}

struct Token {
    text: String,
    /// Byte range in `original`.
    at: (usize, usize),
}

fn tokenise(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start = 0usize;
    for (at, ch) in text.char_indices() {
        if ch.is_alphanumeric() {
            if current.is_empty() {
                start = at;
            }
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(Token {
                text: std::mem::take(&mut current),
                at: (start, at),
            });
        }
    }
    if !current.is_empty() {
        tokens.push(Token {
            text: current,
            at: (start, text.len()),
        });
    }
    tokens
}

impl Source {
    pub fn new(text: &str) -> Self {
        // Walk the original's own characters so every recorded offset is a
        // real byte boundary in it. Case folding changes byte lengths in both
        // directions, so an offset taken from a folded copy cannot be trusted
        // against the original.
        let mut squashed = String::with_capacity(text.len());
        let mut offsets = Vec::with_capacity(text.len());
        for (at, ch) in text.char_indices() {
            if !ch.is_alphanumeric() {
                continue;
            }
            let range = (at, at + ch.len_utf8());
            for lower in ch.to_lowercase() {
                let mut buf = [0u8; 4];
                for _ in lower.encode_utf8(&mut buf).bytes() {
                    offsets.push(range);
                }
                squashed.push(lower);
            }
        }
        Self {
            original: text.to_string(),
            squashed,
            offsets,
            tokens: tokenise(text),
        }
    }

    pub fn text(&self) -> &str {
        &self.original
    }

    /// The span, widened to the lines around it, so the interface can quote the
    /// document rather than a fragment of it.
    pub fn line_around(&self, span: (usize, usize)) -> (usize, usize) {
        let start = self.original[..span.0.min(self.original.len())]
            .rfind('\n')
            .map(|at| at + 1)
            .unwrap_or(0);
        let end = self.original[span.1.min(self.original.len())..]
            .find('\n')
            .map(|rel| span.1 + rel)
            .unwrap_or(self.original.len());
        (start, end)
    }

    /// Which line of the document a span falls on, counting from one.
    pub fn line_number(&self, span: (usize, usize)) -> usize {
        self.original[..span.0.min(self.original.len())]
            .matches('\n')
            .count()
            + 1
    }

    fn map_back(&self, from: usize, to: usize) -> Option<(usize, usize)> {
        let start = self.offsets.get(from)?.0;
        let end = self.offsets.get(to.checked_sub(1)?)?.1;
        (start < end).then_some((start, end))
    }

    /// Verify one proposed value against the document.
    pub fn anchor(&self, value: &str) -> Anchor {
        let needle: String = value
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if needle.is_empty() {
            return Anchor::None;
        }

        // A quotation: the same characters, whatever the spacing and
        // punctuation were. This covers "+1 (415) 555-0148" against
        // "+1 415 555 0148".
        if let Some(at) = self.squashed.find(&needle) {
            if let Some(span) = self.map_back(at, at + needle.len()) {
                return if self.is_fragment(span) {
                    // Present, but only inside something bigger. The document
                    // contains these characters without claiming them as a
                    // value of their own, so it is not a quotation.
                    Anchor::Loose {
                        span,
                        overlap: 1.0,
                        fragment: true,
                    }
                } else {
                    Anchor::Exact { span }
                };
            }
        }

        let wanted = tokenise(value);
        if wanted.len() < MIN_TOKENS_FOR_LOOSE {
            // Too short to match loosely without matching almost anything.
            return Anchor::None;
        }

        // Slide a window the size of the value over the document and keep the
        // one that accounts for most of the value's words.
        let width = wanted.len();
        let mut best: Option<((usize, usize), f32)> = None;
        for start in 0..self.tokens.len().saturating_sub(1) {
            let end = (start + width + 2).min(self.tokens.len());
            if end <= start {
                break;
            }
            let window = &self.tokens[start..end];
            let found = wanted
                .iter()
                .filter(|w| window.iter().any(|t| t.text == w.text))
                .count();
            let overlap = found as f32 / wanted.len() as f32;
            if overlap >= LOOSE_THRESHOLD && best.map(|(_, b)| overlap > b).unwrap_or(true) {
                // Report only the part of the window that actually matched,
                // so the quoted source is the evidence and not its neighbours.
                let hits: Vec<&Token> = window
                    .iter()
                    .filter(|t| wanted.iter().any(|w| w.text == t.text))
                    .collect();
                if let (Some(first), Some(last)) = (hits.first(), hits.last()) {
                    best = Some(((first.at.0, last.at.1), overlap));
                }
            }
        }

        match best {
            Some((span, overlap)) => Anchor::Loose {
                span,
                overlap,
                fragment: false,
            },
            None => Anchor::None,
        }
    }

    /// Is this span only part of a larger identifier?
    ///
    /// Characters that bind a compound token together (a dot in a domain, an
    /// at sign in an address, a slash in a path) are walked across. A plus or a
    /// bracket in front of a phone number is not: those are ordinary
    /// punctuation around a value rather than part of one.
    fn is_fragment(&self, span: (usize, usize)) -> bool {
        const BINDING: [char; 5] = ['.', '@', '_', '-', '/'];
        let text = &self.original;

        let mut before = text[..span.0].chars().rev().peekable();
        while let Some(ch) = before.next() {
            if BINDING.contains(&ch) {
                if before.peek().is_some_and(|c| c.is_alphanumeric()) {
                    return true;
                }
                continue;
            }
            break;
        }

        let mut after = text[span.1..].chars().peekable();
        while let Some(ch) = after.next() {
            if BINDING.contains(&ch) {
                if after.peek().is_some_and(|c| c.is_alphanumeric()) {
                    return true;
                }
                continue;
            }
            break;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESUME: &str = "\
DANA FERREIRA
dana@dferreira.dev · +1 (415) 555-0148 · github.com/dferreira
SF Bay Area · open to remote

EXPERIENCE

Cloudflare — Senior Software Engineer, Storage
March 2023 – February 2026 · remote
Built the replication layer for a globally distributed object store.

Honeycomb — Software Engineer, Infrastructure
August 2020 – February 2023

SKILLS
Rust, Go, Tokio, gRPC, PostgreSQL, FoundationDB, Kubernetes, Linux";

    fn source() -> Source {
        Source::new(RESUME)
    }

    fn quoted(src: &Source, anchor: &Anchor) -> String {
        let (a, b) = anchor.span().expect("expected an anchor");
        src.text()[a..b].to_string()
    }

    #[test]
    fn a_value_the_document_states_is_quoted_back() {
        let src = source();
        let anchor = src.anchor("Cloudflare — Senior Software Engineer, Storage");
        assert!(matches!(anchor, Anchor::Exact { .. }));
        assert_eq!(
            quoted(&src, &anchor),
            "Cloudflare — Senior Software Engineer, Storage"
        );
        assert!(anchor.pre_accepted());
    }

    #[test]
    fn punctuation_and_case_do_not_make_it_a_different_value() {
        let src = source();
        // The same phone number, written the way a form wants it. The span
        // covers the characters that matched, so it starts at the first digit.
        // The leading '+' is punctuation both sides ignore. The interface
        // quotes the whole line around it, which carries the '+'.
        let anchor = src.anchor("+1 415 555 0148");
        assert!(matches!(anchor, Anchor::Exact { .. }), "{anchor:?}");
        assert_eq!(quoted(&src, &anchor), "1 (415) 555-0148");
        let (a, b) = src.line_around(anchor.span().unwrap());
        assert!(src.text()[a..b].contains("+1 (415) 555-0148"));

        assert!(matches!(
            src.anchor("dana@dferreira.dev"),
            Anchor::Exact { .. }
        ));
        assert!(matches!(src.anchor("DANA FERREIRA"), Anchor::Exact { .. }));
        assert!(matches!(src.anchor("Dana Ferreira"), Anchor::Exact { .. }));
    }

    #[test]
    fn an_invented_value_is_not_offered_at_all() {
        let src = source();
        // The kind of thing a model writes when it is summarising rather than
        // reading: true-sounding, and nowhere in the document.
        for invented in [
            "Backend engineer with deep experience in distributed storage.",
            "Stripe — Staff Engineer",
            "dana@gmail.com",
            "+1 212 555 9999",
            "Ten years of experience",
        ] {
            let anchor = src.anchor(invented);
            assert_eq!(anchor, Anchor::None, "{invented:?} was offered");
            assert!(!anchor.offerable());
        }
    }

    #[test]
    fn a_rewrite_is_offered_but_never_pre_accepted() {
        let src = source();
        // "SF Bay Area · open to remote" read as this. Overlapping, not quoted.
        let anchor = src.anchor("SF Bay Area, open to remote work");
        match anchor {
            Anchor::Loose { overlap, .. } => {
                assert!(overlap >= LOOSE_THRESHOLD);
                assert!(!anchor.pre_accepted(), "a rewrite must not arrive accepted");
                assert!(anchor.offerable());
            }
            other => panic!("expected a loose anchor, got {other:?}"),
        }
    }

    #[test]
    fn text_found_only_inside_something_larger_is_not_a_quotation() {
        // A model that invents a website from the domain half of an email
        // produces characters the document really contains. It is still not
        // something the document says, so it must not arrive pre-accepted.
        let src = source();
        match src.anchor("dferreira.dev") {
            Anchor::Loose { fragment, .. } => assert!(fragment),
            other => panic!("expected a fragment, got {other:?}"),
        }
        assert!(!src.anchor("dferreira.dev").pre_accepted());
        assert!(src.anchor("dferreira.dev").offerable());

        // The whole address is a real value and stays a quotation.
        assert!(matches!(
            src.anchor("dana@dferreira.dev"),
            Anchor::Exact { .. }
        ));
        // And so does a phone number wrapped in ordinary punctuation.
        assert!(matches!(
            src.anchor("+1 415 555 0148"),
            Anchor::Exact { .. }
        ));
        // A word beside a comma is not a fragment.
        assert!(matches!(src.anchor("Rust"), Anchor::Exact { .. }));
        assert!(matches!(src.anchor("Cloudflare"), Anchor::Exact { .. }));
    }

    #[test]
    fn one_word_has_to_be_quoted_or_it_is_not_offered() {
        let src = source();
        // Present as a word: fine.
        assert!(matches!(src.anchor("Rust"), Anchor::Exact { .. }));
        // Absent: a single token can be found almost anywhere by fuzzy match,
        // so it must not be offered on a near miss.
        assert_eq!(src.anchor("Python"), Anchor::None);
        assert_eq!(src.anchor("Stripe"), Anchor::None);
    }

    #[test]
    fn the_quoted_span_is_always_real_text_from_the_document() {
        // The property that matters most: whatever Perch shows as evidence has
        // to be a genuine substring of the source, at a real char boundary.
        let src = source();
        let values = [
            "Cloudflare",
            "dana@dferreira.dev",
            "March 2023 – February 2026",
            "SF Bay Area, open to remote work",
            "Honeycomb — Software Engineer, Infrastructure",
            "Rust, Go, Tokio",
            "globally distributed object store",
            "nothing like this appears here",
            "",
            "   ",
            "…",
        ];
        for value in values {
            let anchor = src.anchor(value);
            if let Some((a, b)) = anchor.span() {
                assert!(a < b && b <= RESUME.len(), "{value:?} gave a bad span");
                assert!(RESUME.is_char_boundary(a) && RESUME.is_char_boundary(b));
                let _ = &RESUME[a..b];
            }
        }
    }

    #[test]
    fn a_unicode_document_does_not_shift_the_span() {
        // İ grows a byte when lowercased and ẞ shrinks one. The offsets have to
        // come from the original, not from a folded copy of it.
        let text = "İstanbul ẞtraße\nCloudflare — Senior Engineer\nDANA FERREİRA";
        let src = Source::new(text);
        for value in ["Cloudflare", "Senior Engineer", "İstanbul"] {
            let anchor = src.anchor(value);
            if let Some((a, b)) = anchor.span() {
                assert!(text.is_char_boundary(a) && text.is_char_boundary(b));
                let quoted = &text[a..b];
                assert_eq!(
                    quoted.to_lowercase().replace(['—', ' '], ""),
                    value.to_lowercase().replace(['—', ' '], ""),
                    "quoted {quoted:?} for {value:?}"
                );
            }
        }
    }

    #[test]
    fn an_empty_or_punctuation_only_value_anchors_nowhere() {
        let src = source();
        for value in ["", " ", "\n\t", "—", "···", "!!!"] {
            assert_eq!(src.anchor(value), Anchor::None, "{value:?}");
        }
    }

    #[test]
    fn lines_can_be_quoted_around_a_span() {
        let src = source();
        let anchor = src.anchor("Cloudflare — Senior Software Engineer, Storage");
        let span = anchor.span().unwrap();
        let (a, b) = src.line_around(span);
        assert_eq!(
            &RESUME[a..b],
            "Cloudflare — Senior Software Engineer, Storage"
        );
        assert_eq!(src.line_number(span), 7);
    }

    #[test]
    fn nothing_here_panics_on_a_hostile_document() {
        for doc in ["", "\n\n\n", "é".repeat(500).as_str(), "\u{0}\u{1}", "aaaa"] {
            let src = Source::new(doc);
            for value in ["anything", "", "aaaa", "é é é"] {
                let anchor = src.anchor(value);
                if let Some((a, b)) = anchor.span() {
                    assert!(doc.is_char_boundary(a) && doc.is_char_boundary(b));
                }
                let _ = src.line_around((0, doc.len()));
            }
        }
    }
}

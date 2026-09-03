//! Turning a job board's HTML into something safe to render.
//!
//! Boards hand back arbitrary HTML written by whoever posted the role.
//! Rendering that in a webview unescaped would hand a stranger a script tag in
//! an app that holds someone's profile, so Perch never does. It reduces the
//! markup to a short list of blocks (paragraphs, bullets, headings) and the
//! interface renders those as real elements. Nothing from a board is
//! interpreted as markup.

/// The only shapes Perch will render from a posting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading(String),
    Paragraph(String),
    Bullet(String),
}

impl Block {
    pub fn text(&self) -> &str {
        match self {
            Block::Heading(t) | Block::Paragraph(t) | Block::Bullet(t) => t,
        }
    }
}

/// Expand the entities a board escapes its markup with. Unknown entities are
/// left alone rather than guessed at.
pub fn unescape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        // Look ahead a little for the closing semicolon, by characters rather
        // than by bytes: slicing at a fixed byte offset lands inside a
        // multi-byte character the moment a board writes a curly apostrophe
        // near an ampersand, and panics.
        let end = rest
            .char_indices()
            .take_while(|(at, _)| *at < 12)
            .find(|(_, ch)| *ch == ';')
            .map(|(at, _)| at);
        let Some(end) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let replacement = match entity {
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "amp" => Some("&".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" | "#39" => Some("'".to_string()),
            "nbsp" | "#160" => Some(" ".to_string()),
            "hellip" => Some("…".to_string()),
            "reg" => Some("®".to_string()),
            "trade" => Some("™".to_string()),
            "copy" => Some("©".to_string()),
            "deg" => Some("°".to_string()),
            "mdash" => Some("\u{2014}".to_string()),
            "ndash" => Some("–".to_string()),
            "rsquo" => Some("’".to_string()),
            "lsquo" => Some("‘".to_string()),
            "ldquo" => Some("“".to_string()),
            "rdquo" => Some("”".to_string()),
            other => other
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse::<u32>().ok(),
                })
                .and_then(char::from_u32)
                .map(String::from),
        };
        match replacement {
            Some(text) => {
                out.push_str(&text);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reduce a posting's markup to blocks. Tolerant by design: boards produce
/// malformed HTML constantly, and a posting Perch cannot parse perfectly is
/// still worth showing.
pub fn to_blocks(html: &str) -> Vec<Block> {
    // Entities are resolved to a fixed point before anything is scanned.
    //
    // Boards escape their markup, sometimes twice, so some decoding has to
    // happen first. Doing only part of it first and the rest after the tags are
    // stripped lets the two disagree about where a tag ends: `&gt;` is not a
    // `>` while the scanner is looking for one, so `<img src=x
    // onerror=alert(1)&amp;gt;` reads as an unterminated tag, is kept as text,
    // and then has its `>` decoded back in underneath, handing a complete tag
    // out of the thing whose job is to remove them.
    //
    // Each pass strictly shortens the text, so this terminates; the bound is
    // there so a pathological posting cannot make it the slow part of a sync.
    let mut source = unescape(html);
    for _ in 0..8 {
        let next = unescape(&source);
        if next == source {
            break;
        }
        source = next;
    }

    let mut blocks = Vec::new();
    let mut buffer = String::new();
    let mut kind = Kind::Paragraph;
    let mut chars = source.char_indices().peekable();

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Paragraph,
        Bullet,
        Heading,
    }

    let flush = |buffer: &mut String, kind: Kind, blocks: &mut Vec<Block>| {
        // No decoding here. Everything was decoded before the scan, so what is
        // in the buffer is already text, and turning more of it into `<` or `>`
        // at this point would be doing it behind the scanner's back.
        let text = tidy(buffer);
        buffer.clear();
        if text.is_empty() {
            return;
        }
        blocks.push(match kind {
            Kind::Paragraph => Block::Paragraph(text),
            Kind::Bullet => Block::Bullet(text),
            Kind::Heading => Block::Heading(text),
        });
    };

    while let Some((at, ch)) = chars.next() {
        if ch != '<' {
            buffer.push(ch);
            continue;
        }
        // Read to the closing angle bracket. An unterminated tag is text.
        let Some(close) = source[at..].find('>') else {
            buffer.push('<');
            continue;
        };
        let tag = &source[at + 1..at + close];
        while let Some((next, _)) = chars.peek() {
            if *next >= at + close {
                break;
            }
            chars.next();
        }
        chars.next();

        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let closing = tag.starts_with('/');

        match name.as_str() {
            // Anything that ends a line of prose ends a block.
            "p" | "div" | "br" | "tr" | "section" | "article" | "blockquote" => {
                flush(&mut buffer, kind, &mut blocks);
                kind = Kind::Paragraph;
            }
            "li" => {
                flush(&mut buffer, kind, &mut blocks);
                kind = if closing {
                    Kind::Paragraph
                } else {
                    Kind::Bullet
                };
            }
            "ul" | "ol" => {
                flush(&mut buffer, kind, &mut blocks);
                kind = Kind::Paragraph;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                flush(&mut buffer, kind, &mut blocks);
                kind = if closing {
                    Kind::Paragraph
                } else {
                    Kind::Heading
                };
            }
            // A script or style body is not prose; drop it wholesale.
            "script" | "style" => {
                if !closing {
                    let after = at + close + 1;
                    let end = source[after..]
                        .find(&format!("</{name}"))
                        .map(|rel| after + rel)
                        .unwrap_or(source.len());
                    while let Some((next, _)) = chars.peek() {
                        if *next >= end {
                            break;
                        }
                        chars.next();
                    }
                }
                buffer.clear();
            }
            // Inline markup: keep the words, drop the tag.
            _ => {}
        }
    }
    flush(&mut buffer, kind, &mut blocks);
    blocks
}

/// The blocks as plain text, for the terminal.
pub fn to_text(html: &str) -> String {
    to_blocks(html)
        .iter()
        .map(|b| match b {
            Block::Bullet(t) => format!("• {t}"),
            other => other.text().to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_come_back_as_characters() {
        assert_eq!(unescape("&lt;p&gt;hi&lt;/p&gt;"), "<p>hi</p>");
        assert_eq!(unescape("Figma&rsquo;s team"), "Figma’s team");
        assert_eq!(unescape("&#39;quoted&#39;"), "'quoted'");
        assert_eq!(unescape("&#x2014;"), "\u{2014}");
        assert_eq!(
            unescape("caf&eacute;"),
            "caf&eacute;",
            "unknown entities stay put"
        );
        assert_eq!(unescape("A &amp; B"), "A & B");
        assert_eq!(unescape("bare & ampersand"), "bare & ampersand");
        assert_eq!(unescape(""), "");
    }

    #[test]
    fn a_posting_becomes_paragraphs_and_bullets() {
        let html = "<div class=\"content\"><p>We are hiring.</p><p>Second thought.</p>\
                    <h3>What we want</h3><ul><li>Rust in production</li><li>Some V8</li></ul></div>";
        assert_eq!(
            to_blocks(html),
            vec![
                Block::Paragraph("We are hiring.".into()),
                Block::Paragraph("Second thought.".into()),
                Block::Heading("What we want".into()),
                Block::Bullet("Rust in production".into()),
                Block::Bullet("Some V8".into()),
            ]
        );
    }

    #[test]
    fn markup_from_a_board_can_never_come_back_as_markup() {
        // A posting carrying a script tag yields words, and the interface
        // renders blocks as text nodes, so there is nothing to inject into.
        let nasty = "<p>Hello</p><script>alert('x')</script><img src=x onerror=alert(1)>\
                     <p>Goodbye</p>";
        let blocks = to_blocks(nasty);
        assert_eq!(
            blocks,
            vec![
                Block::Paragraph("Hello".into()),
                Block::Paragraph("Goodbye".into())
            ]
        );
        for block in &blocks {
            assert!(!block.text().contains('<'), "markup survived: {block:?}");
            assert!(!block.text().contains("alert"));
        }
    }

    #[test]
    fn entities_inside_the_escaped_markup_are_resolved_too() {
        // A real Greenhouse posting: the HTML is escaped once, so a literal
        // non-breaking space arrives as `&amp;nbsp;` and has to survive two
        // layers to come out as a space.
        assert_eq!(
            to_blocks("&lt;p&gt;our values&amp;nbsp;and continuous exchange&lt;/p&gt;"),
            vec![Block::Paragraph(
                "our values and continuous exchange".into()
            )]
        );
        assert_eq!(
            to_blocks("&lt;p&gt;Fortune 500&amp;reg; is a trademark&lt;/p&gt;")[0].text(),
            "Fortune 500® is a trademark"
        );
        // Plain, unescaped HTML still works. The second pass is a no-op.
        assert_eq!(
            to_blocks("<p>values&nbsp;and exchange</p>"),
            vec![Block::Paragraph("values and exchange".into())]
        );
    }

    #[test]
    fn double_escaped_markup_is_unwrapped_too() {
        let doubly = "&amp;lt;p&amp;gt;Twice escaped&amp;lt;/p&amp;gt;";
        assert_eq!(
            to_blocks(doubly),
            vec![Block::Paragraph("Twice escaped".into())]
        );
    }

    #[test]
    fn malformed_markup_still_yields_the_words() {
        for broken in [
            "<p>unclosed paragraph",
            "no tags at all",
            "<p>a</p><p>b",
            "<<>><p>weird</p>",
            "<p>text with < a stray bracket</p>",
            "<UL><LI>Uppercase tags</LI></UL>",
        ] {
            let blocks = to_blocks(broken);
            assert!(!blocks.is_empty(), "{broken:?} produced nothing");
        }
        assert_eq!(
            to_blocks("<UL><LI>Uppercase tags</LI></UL>"),
            vec![Block::Bullet("Uppercase tags".into())]
        );
    }

    #[test]
    fn whitespace_is_collapsed_the_way_a_browser_would() {
        assert_eq!(
            to_blocks("<p>  lots   of\n\n  space  </p>"),
            vec![Block::Paragraph("lots of space".into())]
        );
        assert!(to_blocks("<p>   </p><p></p>").is_empty());
    }

    #[test]
    fn an_ampersand_near_a_curly_apostrophe_does_not_panic() {
        // Straight out of a real GitLab posting: an "&" that is not an entity,
        // with a multi-byte character straddling the look-ahead window. This
        // panicked on live board data.
        assert_eq!(unescape("&123456789\u{2019}x"), "&123456789\u{2019}x");
        assert_eq!(
            unescape("Data &amp; Insights \u{2014} GitLab\u{2019}s platform"),
            "Data & Insights \u{2014} GitLab\u{2019}s platform"
        );
        assert!(!to_blocks("<p>Change &amp; Release \u{2014} GitLab\u{2019}s way</p>").is_empty());

        // An ampersand followed by a multi-byte character at every offset
        // inside the look-ahead window. This is the shape that crashed.
        for pad in 0..16 {
            let x = "x".repeat(pad);
            let _ = unescape(&format!("&{x}\u{2019} trailing"));
            let _ = unescape(&format!("&{x}\u{2014};"));
            let _ = to_blocks(&format!("<p>&{x}\u{65e5}\u{672c};</p>"));
        }
    }

    #[test]
    fn an_entity_cannot_smuggle_a_tag_past_the_stripper() {
        // Found by fuzzing, not by reading. A posting escaped once, with no
        // `&lt;` in it, used to defer `&gt;` decoding until after the tag scan
        // had given up looking for a `>`, so the stripper handed back the very
        // thing it exists to remove.
        for smuggled in [
            "<script&amp;gt;alert(1)",
            "<img src=x onerror=alert(1)&amp;gt;",
            "<p>ok</p><img src=x onerror=alert(1)&amp;gt;",
            "&amp;lt;img src=x onerror=alert(1)&amp;gt;",
        ] {
            for block in to_blocks(smuggled) {
                let text = block.text();
                let starts_a_tag = text.match_indices('<').any(|(i, _)| {
                    text[i + 1..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '/')
                });
                assert!(!starts_a_tag, "{smuggled:?} came back as markup: {text:?}");
                assert!(!text.contains("onerror"), "{smuggled:?} kept an attribute");
            }
        }
    }

    #[test]
    fn text_around_a_smuggled_tag_is_still_kept() {
        // Removing the tag must not take the prose with it.
        assert_eq!(
            to_blocks("A role. <b&amp;gt;bold"),
            vec![Block::Paragraph("A role. bold".into())]
        );
    }

    #[test]
    fn nothing_in_here_panics_on_odd_input() {
        for input in [
            "",
            "<",
            ">",
            "&",
            "&#;",
            "&#xZZ;",
            "<script>",
            "</p>",
            "&lt;",
            "<p>é</p>",
        ] {
            let _ = to_blocks(input);
            let _ = unescape(input);
        }
    }
}

//! A page that marks its own postings up as schema.org JobPosting.
//!
//! The fallback for a careers page running on no ATS Perch knows. It reads
//! what the page states about itself inside
//! `<script type="application/ld+json">` and nothing else.
//!
//! JobPosting is a per-posting format. Google's guidance is to put it on each
//! job's own page, and a probe of about thirty live sites found it there and
//! almost never on a careers index: Greenhouse, Ashby, Workable,
//! SmartRecruiters, Recruitee, Teamtailor, Apple, Meta, Indeed and ZipRecruiter
//! index pages all carried none. So a document here holds one posting as often
//! as it holds many, and both have to work.
//!
//! Perch can watch such a page but cannot fill it. There is no form it knows,
//! so `fill_supported` is false and the role opens in the browser for the
//! person to fill themselves. This is the first adapter that watches without
//! filling, which is why the trait declares the two separately.

use super::AtsAdapter;
use crate::error::{Error, Result};
use crate::html;
use crate::http::Http;
use crate::model::{Ats, DetectedBoard, Listing, RemoteRole};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime, PrimitiveDateTime};
use url::Url;

pub struct JsonLd;

/// The plain-date form of `datePosted`, which is the common one.
const DATE_ONLY: &[time::format_description::BorrowedFormatItem<'static>] =
    time::macros::format_description!("[year]-[month]-[day]");

/// A timestamp written with a space where RFC3339 puts a `T`, carrying no
/// offset. Live on remotive.com, and rejected by the RFC3339 parser.
const DATE_TIME_SPACE: &[time::format_description::BorrowedFormatItem<'static>] =
    time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

impl JsonLd {
    /// The page address, normalised. That address is this adapter's token,
    /// because the page is all there is: a document that marks its postings up
    /// is not a board with a slug of its own.
    ///
    /// `board` is UNIQUE(ats, token), so what comes out of here decides when
    /// two inputs are one board.
    ///
    /// Parsed with the `url` crate rather than by hand, for the reason written
    /// out in perch-fill's `flavor::may_fill`: a hand-rolled URL parser that
    /// disagrees with a real one by one character is a whole class of
    /// vulnerability, and the way not to have it is to have one parser.
    fn token_from(input: &str) -> Option<String> {
        let mut url = Url::parse(input.trim()).ok()?;
        // http and https only. Perch fetches whatever this returns, and a
        // file: or javascript: address is not a careers page.
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        // The fragment is never sent to the server, so a page and the same
        // page with an anchor on it are one document and have to be one board.
        url.set_fragment(None);
        Some(url.to_string())
    }
}

impl AtsAdapter for JsonLd {
    fn ats(&self) -> Ats {
        Ats::JsonLd
    }

    /// perch-fill knows no form here: `flavor::for_ats(Ats::JsonLd)` is None
    /// and its host list is empty, so no address from this adapter is ever
    /// filled. The role opens in the browser and the person fills it.
    fn fill_supported(&self) -> bool {
        false
    }

    fn detect(&self, input: &str, http: &Http) -> Result<Option<DetectedBoard>> {
        let Some(token) = Self::token_from(input) else {
            return Ok(None);
        };
        let Some(page) = http.get_html(&token)? else {
            return Ok(None);
        };

        // A page carrying no JobPosting is not something Perch can watch, and
        // "no public board found" is the true answer for it. That is what lets
        // a person point Perch at any address without being told otherwise.
        let postings = job_postings(&page);
        let Some(first) = postings.first() else {
            return Ok(None);
        };

        let company_name = hiring_organization(first)
            .or_else(|| host_of(&token))
            .unwrap_or_else(|| token.clone());

        Ok(Some(DetectedBoard {
            ats: Ats::JsonLd,
            token: token.clone(),
            url: token,
            company_name,
            fill_supported: self.fill_supported(),
        }))
    }

    fn fetch(&self, token: &str, http: &Http) -> Result<Listing> {
        // A page that is not there has told us nothing, which is not the same
        // as a page saying it has nothing open. Only the latter may close
        // roles, so the former has to be an error.
        let Some(page) = http.get_html(token)? else {
            return Err(Error::BoardUnreadable(token.to_string()));
        };
        listing_from(token, &page)
    }

    fn fetch_description(
        &self,
        token: &str,
        external_id: &str,
        http: &Http,
    ) -> Result<Option<String>> {
        // The description arrives with the listing, so a role from here
        // normally has its words already. This is the fallback for one stored
        // without them. There is no per-posting endpoint to ask, so the page
        // is read again and the posting picked out of it by id.
        let Some(page) = http.get_html(token)? else {
            return Ok(None);
        };
        Ok(job_postings(&page)
            .iter()
            .find(|posting| id_of(posting) == external_id)
            .and_then(description))
    }
}

/// The page turned into a listing. Kept out of [`AtsAdapter::fetch`] so the
/// case that decides whether roles get closed can be tested without the
/// network.
fn listing_from(token: &str, page: &str) -> Result<Listing> {
    let postings = job_postings(page);

    // A page with no markup on it has not said it has nothing open. It has
    // stopped being machine readable, which is a different fact. An empty
    // listing here would close every role at that company, so a site that
    // switches to client side rendering has to go visibly stale instead.
    if postings.is_empty() {
        return Err(Error::BoardUnreadable(token.to_string()));
    }

    // What each posting says it is, before any collision is settled. Read for
    // the whole document first, because a posting cannot know it shares an id
    // until every other posting has been asked.
    let stated: Vec<String> = postings.iter().map(id_of).collect();

    let mut listing = Listing::default();
    for (posting, id) in postings.iter().zip(&stated) {
        let shared = stated.iter().filter(|other| *other == id).count() > 1;
        let id = unshared(id.clone(), posting, shared, &listing.listed_ids);
        // Recorded even when the entry will not parse: the page is still
        // naming the posting, so it has not come down.
        listing.listed_ids.push(id.clone());
        if let Some(role) = parse_posting(token, posting, id) {
            listing.roles.push(role);
        }
    }
    Ok(listing)
}

/// `id` again, made different from the ids this document has already handed
/// out.
///
/// A role is stored under its board and its id, so two postings arriving under
/// one id are one row: the second overwrites the first and a posting the page
/// is listing is never stored. A page hands one id out twice in ways the id
/// itself cannot settle. arbeitnow states the company slug in `identifier` on
/// every posting it publishes, and two postings that state nothing beyond the
/// same title, date and place digest the same, because that is all they said.
///
/// The date a posting states settles it, because that belongs to the posting
/// and so reads the same however the page orders them. Numbering by position
/// is the last resort, for postings that state nothing telling them apart at
/// all, and holds only for as long as the page lists them in the order it does.
fn unshared(id: String, posting: &Value, shared: bool, taken: &[String]) -> String {
    if !shared {
        return id;
    }
    // Every posting holding this id takes the dated form, not just the ones
    // after the first. Leaving the first with the bare id made which posting
    // got it depend on the order the page listed them in, so the two swapped
    // names whenever the page reordered and each swap read as two roles
    // closing and two opening.
    let dated = digest(&[id.as_str(), posted_key(posting).as_str()]);
    let mut unique = dated.clone();
    let mut repeat = 1;
    while taken.contains(&unique) {
        repeat += 1;
        unique = format!("{dated}-{repeat}");
    }
    unique
}

/// Every JobPosting a document marks up, in the order the page states them.
///
/// The shapes below are all real on live pages, so the walk has to survive each
/// of them rather than assume the one a given site happens to emit.
pub fn job_postings(page: &str) -> Vec<Value> {
    let mut found = Vec::new();
    for block in ld_json_blocks(page) {
        // One block that will not parse must not lose the good ones beside it
        // in the same document.
        let Ok(value) = serde_json::from_str::<Value>(block) else {
            continue;
        };
        collect(&value, &mut found);
    }
    found
}

/// Walk one parsed block for postings.
///
/// Only the wrappers schema.org actually nests a posting inside are followed.
/// Walking every field instead would read a posting out of something that
/// merely mentions one, such as an Organization describing its openings.
///
/// serde_json refuses input nested deeper than 128 levels, so a document cannot
/// drive this recursion past that.
fn collect(value: &Value, found: &mut Vec<Value>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect(item, found);
            }
        }
        Value::Object(_) => {
            if is_job_posting(value) {
                found.push(value.clone());
                return;
            }
            // "@graph" holds a document's nodes. An ItemList names its entries
            // in "itemListElement", each of which is either the posting or a
            // ListItem wrapping it in "item".
            for key in ["@graph", "itemListElement", "item"] {
                if let Some(inner) = value.get(key) {
                    collect(inner, found);
                }
            }
        }
        _ => {}
    }
}

/// "@type" is a string on most pages and an array on some.
///
/// "@context" is not read. It arrives as `http://schema.org`,
/// `https://schema.org` and either of those with a trailing slash, and none of
/// those differences mean anything. The type is what names a posting.
fn is_job_posting(value: &Value) -> bool {
    match value.get("@type") {
        Some(Value::String(stated)) => stated == "JobPosting",
        Some(Value::Array(stated)) => stated.iter().any(|t| t.as_str() == Some("JobPosting")),
        _ => false,
    }
}

/// The text inside every `<script type="application/ld+json">` in a document.
///
/// Scanned by hand rather than with an HTML parser. The shape being looked for
/// is one tag name and one attribute, and perch-core has no parser to reach
/// for.
///
/// Markup inside a comment is markup the page is not serving. A posting read
/// out of one is listed as open, and because the comment keeps arriving it
/// never closes.
fn ld_json_blocks(page: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut at = 0;

    while at < page.len() {
        let script = find_ignoring_case(&page[at..], "<script").map(|found| at + found);
        let comment = page[at..].find("<!--").map(|found| at + found);
        let opened = match (script, comment) {
            (Some(script), Some(comment)) if comment < script => {
                at = end_of_comment(page, comment);
                continue;
            }
            (None, Some(comment)) => {
                at = end_of_comment(page, comment);
                continue;
            }
            (Some(script), _) => script,
            (None, None) => break,
        };

        let attrs_start = opened + "<script".len();
        let Some(open_end) = end_of_open_tag(page, attrs_start) else {
            break;
        };
        let attrs = &page[attrs_start..open_end];
        // The tag name has to end here, or this is <scriptural>, which holds
        // no script text to step over.
        if !attrs.is_empty() && !attrs.starts_with([' ', '\t', '\n', '\r', '/']) {
            at = open_end + 1;
            continue;
        }

        // A script's text is not markup, whatever the script is for, so the
        // scan resumes after it rather than inside it. A document cut off
        // mid-block still has whatever arrived in it, and the JSON parse
        // decides whether that is a posting.
        let text = open_end + 1;
        let end = find_ignoring_case(&page[text..], "</script")
            .map(|found| text + found)
            .unwrap_or(page.len());
        if attribute(attrs, "type")
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/ld+json")
        {
            blocks.push(&page[text..end]);
        }
        at = end;
    }
    blocks
}

/// Where the document goes on after a comment. An unterminated one runs to the
/// end, which is the rule HTML itself states for it.
fn end_of_comment(page: &str, from: usize) -> usize {
    let inside = from + "<!--".len();
    match page[inside..].find("-->") {
        Some(found) => inside + found + "-->".len(),
        None => page.len(),
    }
}

/// Where `needle` next appears in `haystack`, ignoring the case of its ASCII
/// letters. Its first character is matched as written, so a needle starts with
/// one that has no case. Both of the ones here start with `<`.
///
/// A lowercased copy of the whole page would read more plainly. This is here so
/// a document arriving near the size the http cap allows is not held twice
/// over.
fn find_ignoring_case(haystack: &str, needle: &str) -> Option<usize> {
    let (first, rest) = needle.split_at(1);
    let mut at = 0;
    while let Some(found) = haystack[at..].find(first) {
        let start = at + found;
        let after = start + first.len();
        if haystack.len() >= after + rest.len()
            && haystack.as_bytes()[after..after + rest.len()].eq_ignore_ascii_case(rest.as_bytes())
        {
            return Some(start);
        }
        at = after;
    }
    None
}

/// Where a tag's opening `>` is, skipping any sitting inside a quoted
/// attribute value.
fn end_of_open_tag(page: &str, from: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (offset, byte) in page[from..].bytes().enumerate() {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => return Some(from + offset),
                _ => {}
            },
        }
    }
    None
}

/// One attribute's value out of a tag's attribute text. Handles single quotes,
/// double quotes, no quotes, and spaces on either side of the equals sign.
///
/// The attributes are walked rather than searched for as a substring, because
/// the name has to be a name and has to end where it ends. `data-type=` would
/// answer for `type=`, and so would the `type=` inside
/// `data-note="mime type=ld"`, which reads a posting's own block as something
/// other than ld+json and drops it.
fn attribute<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let bytes = attrs.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
            at += 1;
        }
        let name_start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'=' {
            at += 1;
        }
        let found = &attrs[name_start..at];
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        // An attribute stated without a value, such as `defer`. The next name
        // starts where this one stopped.
        if bytes.get(at) != Some(&b'=') {
            continue;
        }
        at += 1;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let value_start;
        let value_end;
        match bytes.get(at) {
            Some(&quote @ (b'"' | b'\'')) => {
                at += 1;
                value_start = at;
                while at < bytes.len() && bytes[at] != quote {
                    at += 1;
                }
                value_end = at;
                // Past the closing quote, or past the end if there is none.
                at = (at + 1).min(bytes.len());
            }
            _ => {
                value_start = at;
                while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                    at += 1;
                }
                value_end = at;
            }
        }
        if found.eq_ignore_ascii_case(name) {
            return Some(&attrs[value_start..value_end]);
        }
    }
    None
}

/// What Perch calls this posting, for as long as the page keeps stating it.
///
/// A role closes when its id stops arriving, so the id has to be the same
/// string on every sync. Anything that moves turns one posting into a closure
/// and a new opening that never happened.
///
/// In order of preference: the identifier the page states, then the posting's
/// own address, then a digest of what it says about itself. The fixture this
/// was written against carries no identifier and no url at all, which is why
/// the third case exists rather than being a precaution.
fn id_of(posting: &Value) -> String {
    if let Some(stated) = identifier(posting) {
        return stated;
    }
    if let Some(url) = trimmed(posting, "url") {
        return url;
    }
    // Title and place only. The date a posting states is not part of what it
    // is: publishers are told to bump `datePosted` when they put a still open
    // job back up, and keying identity on it made that repost read as the role
    // coming down and a different one arriving in its place. It is still the
    // thing that tells two postings apart when title and place do not, which
    // is why `unshared` reaches for it first.
    //
    // The whole location, the same string the role is stored with. Keyed on
    // the locality alone, a Springfield IL posting and a Springfield MO one
    // shared an id: only one of them was ever stored, and it moved between the
    // two states on every sync.
    digest(&[
        trimmed(posting, "title").unwrap_or_default().as_str(),
        location(posting).as_str(),
    ])
}

/// `datePosted` as the instant it names, so a page restating one date in
/// another of the forms this adapter reads keeps the ids it had.
///
/// One instant is written "2019-03-21", "2019-03-21 00:00:00" and
/// "2019-03-21T00:00:00Z" across the sites measured, and all three parse here.
/// A site changing which one it emits would otherwise close every role on the
/// board and open a new one in its place.
///
/// A date Perch cannot read keeps its text. That is what still separates two
/// postings whose unreadable dates are all they differ in.
fn posted_key(posting: &Value) -> String {
    match parse_time(posting.get("datePosted")) {
        Some(stamp) => stamp.unix_timestamp().to_string(),
        None => trimmed(posting, "datePosted").unwrap_or_default(),
    }
}

/// schema.org allows a bare string here or a PropertyValue naming the scheme
/// it came from, and the value inside one is a number as often as a string.
fn identifier(posting: &Value) -> Option<String> {
    let stated = match posting.get("identifier")? {
        Value::String(text) => text.trim().to_string(),
        object @ Value::Object(_) => match object.get("value") {
            Some(Value::String(text)) => text.trim().to_string(),
            Some(Value::Number(number)) => number.to_string(),
            _ => return None,
        },
        _ => return None,
    };
    (!stated.is_empty()).then_some(stated)
}

/// FNV-1a finished with fmix64, the construction `model::role_reference` uses.
/// Written out here rather than reused because that function hashes a board
/// token and an external id, and this is the external id being made.
///
/// The finisher is the part that matters. Two postings on one page differ in
/// their tail, and FNV-1a's last multiply only carries bits upward, so without
/// it near-identical postings come out sharing a long prefix.
fn digest(parts: &[&str]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        // The separator is what keeps ("ab", "c") and ("a", "bc") apart.
        for byte in part.as_bytes().iter().chain(std::iter::once(&0x1f)) {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    hash ^= hash >> 33;
    format!("{hash:016x}")
}

/// One string field, with the entities a page escapes its values with expanded.
///
/// Every value here may arrive as an explicit JSON null instead of being
/// absent, which `as_str` already reads as nothing.
fn trimmed(posting: &Value, key: &str) -> Option<String> {
    let text = html::unescape(posting.get(key)?.as_str()?);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The address the posting states.
///
/// `jobLocation` is a Place on most pages and an array of them on some, so both
/// shapes have to be read. An array's first entry with an address on it is the
/// one used, because Perch stores one location and that is the one the page
/// leads with.
fn address(posting: &Value) -> Option<&Value> {
    let stated = posting.get("jobLocation")?;
    match stated {
        Value::Array(places) => places.iter().find_map(|place| place.get("address")),
        _ => stated.get("address"),
    }
}

fn locality(posting: &Value) -> Option<String> {
    trimmed(address(posting)?, "addressLocality")
}

/// Where the posting says the work is. The region is added when the page states
/// both, because "Arlington" and "Arlington, TX" are different amounts of
/// answer. A page that states only a region says that much and no more.
fn location(posting: &Value) -> String {
    let region = address(posting).and_then(|a| trimmed(a, "addressRegion"));
    match (locality(posting), region) {
        (Some(locality), Some(region)) => format!("{locality}, {region}"),
        (Some(locality), None) => locality,
        (None, Some(region)) => region,
        (None, None) => String::new(),
    }
}

fn hiring_organization(posting: &Value) -> Option<String> {
    trimmed(posting.get("hiringOrganization")?, "name")
}

fn host_of(url: &str) -> Option<String> {
    Url::parse(url).ok()?.host_str().map(str::to_string)
}

/// The posting's own words, as HTML. Passed through untouched: `crate::html`
/// reduces it to renderable blocks when someone opens the role.
///
/// An empty description is left as None. Stored as Some(""), it would count as
/// the posting's words and the detail pane would show a blank where they go,
/// with nothing left to fetch them again.
fn description(posting: &Value) -> Option<String> {
    let html = posting.get("description")?.as_str()?;
    (!html.trim().is_empty()).then(|| html.to_string())
}

/// `datePosted` is a plain date more often than it is a timestamp, and both
/// forms are live. A plain date is taken as midnight UTC.
///
/// A date read wrong is not a small error. The time signal drives the feed's
/// whole ordering, so a date parsed as the epoch puts the role in 1970 and
/// moves everything around it.
fn parse_time(value: Option<&Value>) -> Option<OffsetDateTime> {
    let text = value?.as_str()?.trim();
    if let Ok(stamp) = OffsetDateTime::parse(text, &Rfc3339) {
        return Some(stamp);
    }
    // Stated without an offset, so it is taken as UTC the same way a plain
    // date is.
    if let Ok(local) = PrimitiveDateTime::parse(text, DATE_TIME_SPACE) {
        return Some(local.assume_utc());
    }
    Date::parse(text, DATE_ONLY)
        .ok()
        .map(|date| date.midnight().assume_utc())
}

fn parse_posting(page_url: &str, posting: &Value, external_id: String) -> Option<RemoteRole> {
    let title = trimmed(posting, "title")?;

    Some(RemoteRole {
        external_id,
        title,
        location: location(posting),
        // A posting that states its own address is linked to at that address.
        // One that does not was found on the page Perch fetched, which is
        // where the person will read it.
        url: trimmed(posting, "url").unwrap_or_else(|| page_url.to_string()),
        posted_at: parse_time(posting.get("datePosted")),
        // schema.org names this dateModified. Most postings carry none, and
        // Perch does not guess at a date it was not given.
        updated_at: parse_time(posting.get("dateModified")),
        description: description(posting),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block from a live Lever posting page, verbatim, shortened only in
    /// the description. There is no identifier, no url and no validThrough,
    /// and four of its values are an explicit null.
    const REAL_BLOCK: &str = r#"{
        "@context": "http://schema.org",
        "@type": "JobPosting",
        "title": "AbelsonTaylor Writer",
        "hiringOrganization": {
            "@type": "Organization",
            "name": "Lever Demo 2",
            "logo": "https://lever-client-logos.s3.amazonaws.com/0f0b.png"
        },
        "jobLocation": {
            "@type": "Place",
            "address": {
                "@type": "PostalAddress",
                "addressLocality": "Arlington, TX",
                "addressRegion": null,
                "addressCountry": null,
                "postalCode": null
            }
        },
        "employmentType": "Regular Full Time (Salary)",
        "datePosted": "2019-03-21",
        "description": "<p>Welcome to the <b>Demo Job Listing</b> for Lever!</p>"
    }"#;

    fn page(blocks: &str) -> String {
        format!("<html><head><title>Careers</title>{blocks}</head><body></body></html>")
    }

    fn script(json: &str) -> String {
        format!(r#"<script type="application/ld+json">{json}</script>"#)
    }

    fn posting(json: &str) -> Value {
        serde_json::from_str(json).expect("a posting")
    }

    fn a_real_page() -> String {
        page(&script(REAL_BLOCK))
    }

    #[test]
    fn the_one_block_a_job_page_carries_yields_its_posting() {
        let found = job_postings(&a_real_page());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["title"], "AbelsonTaylor Writer");
    }

    #[test]
    fn only_the_job_postings_come_out_of_a_document_marked_up_with_other_things() {
        // Real pages carry several blocks and most of them are not postings.
        let document = page(&format!(
            "{}{}{}{}",
            script(r#"{"@context":"https://schema.org","@type":"Organization","name":"Acme"}"#),
            script(r#"{"@type":"BreadcrumbList","itemListElement":[]}"#),
            script(REAL_BLOCK),
            script(r#"{"@type":"WebSite","name":"Acme"}"#),
        ));
        let found = job_postings(&document);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["title"], "AbelsonTaylor Writer");
    }

    #[test]
    fn a_block_whose_top_level_is_an_array_is_read_through() {
        let document = page(&script(&format!(
            r#"[{{"@type":"Organization","name":"Acme"}},{REAL_BLOCK}]"#
        )));
        assert_eq!(job_postings(&document).len(), 1);
    }

    #[test]
    fn a_block_that_wraps_its_nodes_in_a_graph_is_read_through() {
        let document = page(&script(&format!(
            r#"{{"@context":"https://schema.org/","@graph":[{{"@type":"WebSite"}},{REAL_BLOCK}]}}"#
        )));
        assert_eq!(job_postings(&document).len(), 1);
    }

    #[test]
    fn an_item_list_yields_its_postings_wrapped_or_not() {
        // Some indexes name each posting directly, some wrap each in a
        // ListItem. Both shapes are live.
        let document = page(&script(&format!(
            r#"{{"@type":"ItemList","itemListElement":[
                {REAL_BLOCK},
                {{"@type":"ListItem","position":2,"item":{REAL_BLOCK}}}
            ]}}"#
        )));
        assert_eq!(job_postings(&document).len(), 2);
    }

    #[test]
    fn a_type_stated_as_an_array_is_still_a_job_posting() {
        let document = page(&script(r#"{"@type":["JobPosting"],"title":"Engineer"}"#));
        assert_eq!(job_postings(&document).len(), 1);
    }

    #[test]
    fn the_context_a_page_states_does_not_decide_whether_a_posting_is_read() {
        // The four spellings all mean the same thing, and one of them is no
        // spelling at all: a node inside a graph often states no context.
        for context in [
            r#""@context":"http://schema.org","#,
            r#""@context":"https://schema.org","#,
            r#""@context":"http://schema.org/","#,
            r#""@context":"https://schema.org/","#,
            "",
        ] {
            let document = page(&script(&format!(
                r#"{{{context}"@type":"JobPosting","title":"Engineer"}}"#
            )));
            assert_eq!(job_postings(&document).len(), 1, "{context} was not read");
        }
    }

    #[test]
    fn a_block_that_is_not_json_does_not_lose_the_good_block_beside_it() {
        let document = page(&format!(
            "{}{}{}",
            script("{ this is not json at all "),
            script(REAL_BLOCK),
            script("</not json either>"),
        ));
        assert_eq!(job_postings(&document).len(), 1);
    }

    #[test]
    fn a_document_with_no_ld_json_in_it_names_no_postings() {
        let document = "<html><body><h1>Careers</h1><p>Nothing here.</p></body></html>";
        assert!(job_postings(document).is_empty());
        // A page whose script tags are ordinary JavaScript is the same answer.
        let scripted = r#"<html><script src="/app.js"></script>
            <script>window.jobs = [{"title":"Engineer"}];</script></html>"#;
        assert!(job_postings(scripted).is_empty());
    }

    #[test]
    fn a_document_whose_only_markup_is_an_organization_names_no_postings() {
        let document = page(&script(
            r#"{"@context":"https://schema.org","@type":"Organization","name":"Acme",
                "sameAs":"https://acme.example"}"#,
        ));
        assert!(job_postings(&document).is_empty());
    }

    #[test]
    fn the_script_tag_is_recognised_however_its_attributes_are_written() {
        for tag in [
            r#"<script type="application/ld+json">"#,
            r#"<script type='application/ld+json'>"#,
            r#"<script type = "application/ld+json">"#,
            r#"<script  type ='application/ld+json' >"#,
            r#"<script type=application/ld+json>"#,
            r#"<script type="APPLICATION/LD+JSON">"#,
            r#"<script id="job" type="application/ld+json" data-turbo="false">"#,
            r#"<script data-note="a>b" type="application/ld+json">"#,
        ] {
            let document = format!("<html>{tag}{REAL_BLOCK}</script></html>");
            assert_eq!(job_postings(&document).len(), 1, "{tag} was not recognised");
        }
    }

    #[test]
    fn a_tag_that_is_not_ld_json_is_not_read_as_one() {
        for tag in [
            "<script>",
            r#"<script type="text/javascript">"#,
            r#"<script data-type="application/ld+json">"#,
            r#"<script src="application/ld+json.js">"#,
        ] {
            let document = format!("<html>{tag}{REAL_BLOCK}</script></html>");
            assert!(job_postings(&document).is_empty(), "{tag} was read");
        }
    }

    #[test]
    fn a_value_stated_as_an_explicit_null_reads_as_absent() {
        // The real fixture states four of these. A null where a string goes
        // must read the same as the key not being there.
        let nulled = posting(
            r#"{"@type":"JobPosting","title":"Engineer","identifier":null,"url":null,
                "datePosted":null,"description":null,
                "jobLocation":{"address":{"addressLocality":null,"addressRegion":null}}}"#,
        );
        assert_eq!(location(&nulled), "");
        assert!(description(&nulled).is_none());
        assert!(parse_time(nulled.get("datePosted")).is_none());
        assert!(identifier(&nulled).is_none());
        let role = parse_posting("https://acme.example/jobs", &nulled, id_of(&nulled)).unwrap();
        assert_eq!(role.url, "https://acme.example/jobs");
        assert!(role.posted_at.is_none());
    }

    #[test]
    fn entities_inside_a_value_are_expanded_in_the_words_perch_shows() {
        let escaped = posting(
            r#"{"@type":"JobPosting","title":"Research &amp; Development Lead",
                "description":"<p>R&amp;D, and it stays escaped.</p>",
                "jobLocation":{"address":{"addressLocality":"Saint-Bruno",
                "addressRegion":"Qu&#233;bec"}}}"#,
        );
        let role = parse_posting("https://acme.example", &escaped, id_of(&escaped)).unwrap();
        assert_eq!(role.title, "Research & Development Lead");
        assert_eq!(role.location, "Saint-Bruno, Québec");

        // crate::html expands the entities it knows and leaves the rest as
        // they were, rather than guessing. A name Perch does not have keeps
        // its ampersand, which reads oddly but never invents a character.
        let unknown = posting(r#"{"@type":"JobPosting","title":"Ing&eacute;nieur"}"#);
        assert_eq!(
            trimmed(&unknown, "title").as_deref(),
            Some("Ing&eacute;nieur")
        );
        // The description is markup, and crate::html expands it when the role
        // is opened. Expanding it here would turn an escaped tag into a tag.
        assert_eq!(
            role.description.as_deref(),
            Some("<p>R&amp;D, and it stays escaped.</p>")
        );
    }

    #[test]
    fn the_same_posting_yields_the_same_id_on_two_separate_parses() {
        // A role closes when its id stops arriving. An id that moves between
        // syncs writes a closure and a new opening that never happened.
        let once = job_postings(&a_real_page());
        let again = job_postings(&a_real_page());
        assert_eq!(id_of(&once[0]), id_of(&again[0]));
        // And it survives the page being served with its keys reordered and
        // its whitespace changed, because the digest reads values not bytes.
        let reordered = page(&script(
            r#"{"@type":"JobPosting","datePosted":"2019-03-21",
                "jobLocation":{"address":{"addressLocality":"Arlington, TX"}},
                "title":"AbelsonTaylor Writer"}"#,
        ));
        assert_eq!(id_of(&once[0]), id_of(&job_postings(&reordered)[0]));
    }

    #[test]
    fn two_postings_on_one_page_do_not_share_an_id() {
        let document = page(&script(
            r#"[{"@type":"JobPosting","title":"Staff Engineer","datePosted":"2026-01-05",
                 "jobLocation":{"address":{"addressLocality":"Berlin"}}},
                {"@type":"JobPosting","title":"Senior Engineer","datePosted":"2026-01-05",
                 "jobLocation":{"address":{"addressLocality":"Berlin"}}},
                {"@type":"JobPosting","title":"Staff Engineer","datePosted":"2026-01-06",
                 "jobLocation":{"address":{"addressLocality":"Berlin"}}},
                {"@type":"JobPosting","title":"Staff Engineer","datePosted":"2026-01-05",
                 "jobLocation":{"address":{"addressLocality":"Lisbon"}}}]"#,
        ));
        // Through `listing_from`, which is where identity is actually settled:
        // `id_of` reads title and place, and the date separates the pair that
        // agree on both.
        let listing = listing_from("https://acme.example/careers", &document).unwrap();
        let ids: std::collections::HashSet<&String> = listing.listed_ids.iter().collect();
        assert_eq!(ids.len(), 4, "postings differing in one field share an id");
    }

    #[test]
    fn the_id_is_the_identifier_the_page_states_before_anything_derived() {
        let stated = posting(
            r#"{"@type":"JobPosting","title":"Engineer","identifier":"R-4821",
            "url":"https://acme.example/jobs/4821"}"#,
        );
        assert_eq!(id_of(&stated), "R-4821");

        // A PropertyValue names the scheme the identifier came from, and its
        // value is a number about as often as a string.
        let as_object = posting(
            r#"{"@type":"JobPosting","title":"Engineer",
                "identifier":{"@type":"PropertyValue","name":"Acme","value":"R-4821"}}"#,
        );
        assert_eq!(id_of(&as_object), "R-4821");
        let as_number = posting(
            r#"{"@type":"JobPosting","title":"Engineer",
                "identifier":{"@type":"PropertyValue","value":4821}}"#,
        );
        assert_eq!(id_of(&as_number), "4821");

        // No identifier leaves the posting's own address.
        let by_url = posting(
            r#"{"@type":"JobPosting","title":"Engineer","url":"https://acme.example/jobs/4821"}"#,
        );
        assert_eq!(id_of(&by_url), "https://acme.example/jobs/4821");

        // Neither leaves the digest, which is the case the real fixture is in.
        let digested = job_postings(&a_real_page());
        assert_eq!(id_of(&digested[0]).len(), 16);
        assert!(id_of(&digested[0]).chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_plain_date_is_midnight_utc_and_a_timestamp_keeps_the_instant_it_names() {
        let plain = parse_time(Some(&Value::String("2019-03-21".into()))).unwrap();
        assert_eq!(plain.year(), 2019);
        assert_eq!(plain.month(), time::Month::March);
        assert_eq!(plain.day(), 21);
        assert_eq!(plain.hour(), 0);
        assert_eq!(plain.offset(), time::UtcOffset::UTC);

        let stamp = parse_time(Some(&Value::String("2019-03-21T16:33:55-04:00".into()))).unwrap();
        assert_eq!(stamp.unix_timestamp(), 1553200435);

        // A date Perch cannot read is no date. Anything else puts the role in
        // 1970 and moves every row around it.
        for unreadable in ["", "   ", "yesterday", "21/03/2019", "2019-03"] {
            assert!(
                parse_time(Some(&Value::String(unreadable.into()))).is_none(),
                "{unreadable:?} was read as a date"
            );
        }
    }

    #[test]
    fn a_timestamp_written_with_a_space_instead_of_a_t_is_read_as_utc() {
        // remotive.com states datePosted this way. It is neither a plain date
        // nor RFC3339, so before it was read it left the role with no date at
        // all and the feed ordered it as undated.
        let stamp = parse_time(Some(&Value::String("2026-08-20 09:54:55".into()))).unwrap();
        assert_eq!(stamp.year(), 2026);
        assert_eq!(stamp.month(), time::Month::August);
        assert_eq!(stamp.day(), 20);
        assert_eq!(stamp.hour(), 9);
        assert_eq!(stamp.minute(), 54);
        assert_eq!(stamp.second(), 55);
        // No offset is stated, so it is taken as UTC rather than guessed at.
        assert_eq!(stamp.offset(), time::UtcOffset::UTC);
    }

    #[test]
    fn a_timestamp_that_states_an_offset_keeps_the_instant_not_the_wall_clock() {
        // teamtailor and arbeitnow both state +02:00. Taking the date part of
        // such a string and calling it midnight UTC lands a day late.
        let stamp = parse_time(Some(&Value::String("2026-08-19T00:00:00+02:00".into()))).unwrap();
        assert_eq!(stamp.to_offset(time::UtcOffset::UTC).day(), 18);
        assert_eq!(stamp.to_offset(time::UtcOffset::UTC).hour(), 22);
    }

    #[test]
    fn a_location_stated_as_a_list_of_places_is_read_like_a_single_one() {
        // jobs.ac.uk and teamtailor both state jobLocation as an array. Read
        // only as an object it yields nothing, and the role loses a location
        // the page plainly stated.
        let listed = posting(
            r#"{"@type":"JobPosting","title":"Engineer","jobLocation":[
                {"@type":"Place","address":{"@type":"PostalAddress",
                 "addressLocality":"Birmingham","addressRegion":"England"}}]}"#,
        );
        assert_eq!(location(&listed), "Birmingham, England");

        // An entry stating a locality and no region says that much and no more.
        let one_field = posting(
            r#"{"@type":"JobPosting","title":"Engineer","jobLocation":[
                {"@type":"Place","address":{"addressLocality":"Stockholm",
                 "addressRegion":null}}]}"#,
        );
        assert_eq!(location(&one_field), "Stockholm");

        // An empty list states no place, which is not a place called "".
        let empty = posting(r#"{"@type":"JobPosting","title":"Engineer","jobLocation":[]}"#);
        assert_eq!(location(&empty), "");
    }

    #[test]
    fn a_posting_parses_out_of_the_real_page_shape() {
        let listing =
            listing_from("https://jobs.lever.co/leverdemo/33538a2f", &a_real_page()).unwrap();
        assert_eq!(listing.roles.len(), 1);
        let role = &listing.roles[0];
        assert_eq!(role.title, "AbelsonTaylor Writer");
        // addressRegion is an explicit null, so the locality stands alone.
        assert_eq!(role.location, "Arlington, TX");
        assert_eq!(role.url, "https://jobs.lever.co/leverdemo/33538a2f");
        assert_eq!(role.posted_at.unwrap().year(), 2019);
        assert!(role.updated_at.is_none());
        assert!(role
            .description
            .as_deref()
            .unwrap()
            .contains("Demo Job Listing"));
        assert_eq!(listing.listed_ids, vec![role.external_id.clone()]);
    }

    #[test]
    fn a_posting_that_states_its_own_address_is_stored_at_that_address() {
        let document = page(&script(
            r#"{"@type":"JobPosting","title":"Engineer","url":"https://acme.example/jobs/1"}"#,
        ));
        let listing = listing_from("https://acme.example/careers", &document).unwrap();
        assert_eq!(listing.roles[0].url, "https://acme.example/jobs/1");
    }

    #[test]
    fn an_entry_that_will_not_parse_still_counts_as_listed() {
        // Otherwise the next sync reports it as having come down, and writes
        // a closure into the history that never happened.
        let document = page(&script(
            r#"[{"@type":"JobPosting","title":"   ","identifier":"R-1"},
                {"@type":"JobPosting","title":"Engineer","identifier":"R-2"}]"#,
        ));
        let listing = listing_from("https://acme.example/careers", &document).unwrap();
        let titles: Vec<&str> = listing.roles.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["Engineer"]);
        assert_eq!(listing.listed_ids, vec!["R-1", "R-2"]);
    }

    #[test]
    fn a_page_with_no_job_posting_on_it_is_unreadable_rather_than_empty() {
        // This is the rule the whole adapter turns on. A page that stopped
        // being machine readable has not said it has nothing open, and an
        // empty listing here would close every role at that company.
        for quiet in [
            page(""),
            page(&script(r#"{"@type":"Organization","name":"Acme"}"#)),
            "<html><body>Loading...</body></html>".to_string(),
            String::new(),
        ] {
            assert!(matches!(
                listing_from("https://acme.example/careers", &quiet),
                Err(Error::BoardUnreadable(_))
            ));
        }
    }

    #[test]
    fn the_token_is_the_page_address_normalised() {
        let t = JsonLd::token_from;
        assert_eq!(
            t("https://acme.example/careers").as_deref(),
            Some("https://acme.example/careers")
        );
        // One board, however the address was typed: the host case, the empty
        // path and the fragment all normalise away, and board is
        // UNIQUE(ats, token).
        for same in [
            "https://ACME.example",
            "https://acme.example",
            "https://acme.example/",
            "  https://acme.example/  ",
            "https://acme.example/#openings",
        ] {
            assert_eq!(
                t(same).as_deref(),
                Some("https://acme.example/"),
                "{same} normalised to something else"
            );
        }
        // A query names a different page, so it is kept.
        assert_eq!(
            t("https://acme.example/jobs?team=infra").as_deref(),
            Some("https://acme.example/jobs?team=infra")
        );
    }

    #[test]
    fn an_address_that_is_not_http_is_not_this_adapters_problem() {
        // Perch fetches whatever the token names, so nothing else may become
        // one. A bare company name is not an address and belongs to the
        // adapters that can resolve it.
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ftp://acme.example/jobs",
            "acme.example/careers",
            "figma",
            "Val Town",
            "",
            "   ",
        ] {
            assert_eq!(JsonLd::token_from(refused), None, "{refused} was accepted");
        }
    }

    #[test]
    fn the_company_is_named_by_the_posting_and_falls_back_to_the_host() {
        let named = job_postings(&a_real_page());
        assert_eq!(
            hiring_organization(&named[0]).as_deref(),
            Some("Lever Demo 2")
        );

        let unnamed = posting(r#"{"@type":"JobPosting","title":"Engineer"}"#);
        assert!(hiring_organization(&unnamed).is_none());
        assert_eq!(
            host_of("https://acme.example/careers").as_deref(),
            Some("acme.example")
        );
    }

    #[test]
    fn this_adapter_does_not_claim_to_fill() {
        // perch-fill knows no form for JSON-LD, so a role from here opens in
        // the browser and the person fills it themselves.
        assert!(!JsonLd.fill_supported());
    }

    #[test]
    fn perch_core_never_says_a_json_ld_board_can_be_filled() {
        // perch-fill is a separate crate, so what this crate can assert is
        // that nothing it hands out claims a fill. Everything downstream reads
        // the flag off the adapter and the detected board.
        let registered = crate::ats::adapter_for(Ats::JsonLd).expect("JSON-LD is registered");
        assert!(!registered.fill_supported());

        let detected = DetectedBoard {
            ats: Ats::JsonLd,
            token: "https://acme.example/careers".into(),
            url: "https://acme.example/careers".into(),
            company_name: "acme.example".into(),
            fill_supported: JsonLd.fill_supported(),
        };
        assert!(!detected.fill_supported);
    }

    #[test]
    fn two_postings_that_differ_only_in_the_region_they_state_are_two_roles() {
        // The id and the stored location have to read one address. Keyed on
        // the locality alone these were one role, holding whichever state came
        // last and recording a move to the other on every sync of a page that
        // never changed.
        let document = page(&script(
            r#"[{"@type":"JobPosting","title":"Store Manager","datePosted":"2026-08-01",
                 "jobLocation":{"address":{"addressLocality":"Springfield","addressRegion":"IL"}}},
                {"@type":"JobPosting","title":"Store Manager","datePosted":"2026-08-01",
                 "jobLocation":{"address":{"addressLocality":"Springfield","addressRegion":"MO"}}}]"#,
        ));
        let listing = listing_from("https://acme.example/careers", &document).unwrap();
        assert_eq!(listing.listed_ids.len(), 2);
        assert_ne!(listing.listed_ids[0], listing.listed_ids[1]);
        let where_the_work_is: Vec<&str> =
            listing.roles.iter().map(|r| r.location.as_str()).collect();
        assert_eq!(where_the_work_is, ["Springfield, IL", "Springfield, MO"]);
    }

    #[test]
    fn a_job_put_back_up_with_a_new_date_is_the_same_role() {
        // Publishers are told to bump `datePosted` when they repost a job that
        // never came down. With the date in the id this read as the role
        // closing and a different one arriving, so the sync reported "1 new, 1
        // came down" about a page whose one posting had not moved.
        let posted = |date: &str| {
            page(&script(&format!(
                r#"{{"@type":"JobPosting","title":"Systems Engineer","datePosted":"{date}",
                    "jobLocation":{{"address":{{"addressLocality":"Portland","addressRegion":"OR"}}}}}}"#
            )))
        };
        let january = listing_from("https://acme.example/careers", &posted("2026-01-15")).unwrap();
        let june = listing_from("https://acme.example/careers", &posted("2026-06-01")).unwrap();
        assert_eq!(january.listed_ids, june.listed_ids);
        assert_eq!(
            june.roles[0].posted_at.unwrap().date().to_string(),
            "2026-06-01"
        );
    }

    #[test]
    fn two_postings_a_date_tells_apart_keep_their_ids_when_the_page_reorders() {
        // Numbering a repeat by its position holds only while the order does.
        // A date belongs to the posting, so it settles the pair the same way
        // whichever order the page lists them in.
        let pair = |first: &str, second: &str| {
            page(&script(&format!(
                r#"[{{"@type":"JobPosting","title":"Courier","identifier":"plutos","datePosted":"{first}"}},
                    {{"@type":"JobPosting","title":"Courier","identifier":"plutos","datePosted":"{second}"}}]"#
            )))
        };
        let one =
            listing_from("https://acme.example/c", &pair("2026-03-01", "2026-04-01")).unwrap();
        let swapped =
            listing_from("https://acme.example/c", &pair("2026-04-01", "2026-03-01")).unwrap();
        let mut first: Vec<&String> = one.listed_ids.iter().collect();
        let mut second: Vec<&String> = swapped.listed_ids.iter().collect();
        first.sort();
        second.sort();
        assert_eq!(first, second, "reordering the page renamed a posting");
    }

    #[test]
    fn a_page_naming_two_postings_the_same_way_still_lists_two_of_them() {
        // A role is stored under its id, so a page handing one id out twice is
        // a page with a posting Perch never stores. arbeitnow states the
        // company slug as the identifier of every posting it publishes.
        let same_identifier = page(&script(
            r#"[{"@type":"JobPosting","title":"Backend Engineer","identifier":"plutos"},
                {"@type":"JobPosting","title":"Frontend Engineer","identifier":"plutos"}]"#,
        ));
        let listing = listing_from("https://acme.example/careers", &same_identifier).unwrap();
        assert_eq!(listing.listed_ids.len(), 2);
        assert_ne!(listing.listed_ids[0], listing.listed_ids[1]);
        // Neither keeps the bare "plutos". Leaving it to whichever posting the
        // page happened to list first is what made the two swap names.
        assert!(!listing.listed_ids.contains(&"plutos".to_string()));
        assert_eq!(listing.roles.len(), 2);

        // And the same two ids the next time the page is read, or the second
        // posting closes and opens again on every sync.
        let again = listing_from("https://acme.example/careers", &same_identifier).unwrap();
        assert_eq!(listing.listed_ids, again.listed_ids);

        // Two postings can also say nothing that tells them apart. They are
        // still two postings.
        let alike = page(&script(
            r#"[{"@type":"JobPosting","title":"Barista","datePosted":"2026-08-01",
                 "employmentType":"FULL_TIME"},
                {"@type":"JobPosting","title":"Barista","datePosted":"2026-08-01",
                 "employmentType":"PART_TIME"}]"#,
        ));
        let listing = listing_from("https://acme.example/careers", &alike).unwrap();
        assert_eq!(listing.listed_ids.len(), 2);
        assert_ne!(listing.listed_ids[0], listing.listed_ids[1]);
    }

    #[test]
    fn one_instant_written_in_any_form_this_adapter_reads_keeps_one_id() {
        // A site that changes how it writes its dates has not changed its
        // postings. Keyed on the text, every role on such a board closed and
        // opened again as a new one the night that happened.
        let ids: Vec<String> = ["2019-03-21", "2019-03-21 00:00:00", "2019-03-21T00:00:00Z"]
            .iter()
            .map(|stated| {
                let document = page(&script(&format!(
                    r#"{{"@type":"JobPosting","title":"Writer","datePosted":"{stated}",
                        "jobLocation":{{"address":{{"addressLocality":"Arlington"}}}}}}"#
                )));
                id_of(&job_postings(&document)[0])
            })
            .collect();
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[0], ids[2]);

        // A date Perch cannot read keeps its text, which is what still tells
        // two postings apart when the date is all they differ in.
        let unreadable = page(&script(
            r#"[{"@type":"JobPosting","title":"Writer","datePosted":"Spring 2026"},
                {"@type":"JobPosting","title":"Writer","datePosted":"Autumn 2026"}]"#,
        ));
        let listing = listing_from("https://acme.example/careers", &unreadable).unwrap();
        assert_ne!(listing.listed_ids[0], listing.listed_ids[1]);
    }

    #[test]
    fn a_type_inside_another_attributes_value_is_not_read_as_the_type_attribute() {
        // Every one of these tags is ld+json and its posting has to come out.
        // Read as a plain substring, the `type=` sitting in the earlier value
        // answered first, the block was passed over, and the next sync read
        // the posting as having come down.
        for tag in [
            r#"<script data-note="mime type=ld" type="application/ld+json">"#,
            r#"<script class="x type=y" type="application/ld+json">"#,
            r#"<script data-x='a type=b' type="application/ld+json">"#,
        ] {
            let document = format!("<html>{tag}{REAL_BLOCK}</script></html>");
            assert_eq!(job_postings(&document).len(), 1, "{tag} was not read");
        }
    }

    #[test]
    fn a_posting_inside_a_comment_is_not_one_the_page_is_listing() {
        let commented = script(r#"{"@type":"JobPosting","title":"Old role","identifier":"R-OLD"}"#);
        let served = script(r#"{"@type":"JobPosting","title":"Open role","identifier":"R-NEW"}"#);
        let document = format!("<html><!-- kept for reference {commented} -->{served}</html>");
        let listing = listing_from("https://acme.example/careers", &document).unwrap();
        assert_eq!(listing.listed_ids, ["R-NEW"]);

        // A page whose only markup is commented out is one Perch cannot read,
        // not one that says it has nothing open. Read as live, the posting
        // would be listed as open and, the comment arriving every time, would
        // never close.
        let only_commented = format!("<html><!-- {commented} --><p>No open roles.</p></html>");
        assert!(job_postings(&only_commented).is_empty());
        assert!(listing_from("https://acme.example/careers", &only_commented).is_err());
    }

    #[test]
    fn a_comment_written_inside_a_script_does_not_hide_the_markup_after_it() {
        // What a script holds is text, not markup, so a `<!--` in one opens
        // nothing. A page whose scripts were read as markup would lose every
        // posting stated after the first script that mentioned a comment.
        let document = format!(
            r#"<html><script>var s = "<!--";</script>{}</html>"#,
            script(REAL_BLOCK)
        );
        assert_eq!(job_postings(&document).len(), 1);

        // Including a comment written inside the posting's own description,
        // which arrives as the page's markup and is stored as it is.
        let describes = script(
            r#"{"@type":"JobPosting","title":"Writer","identifier":"R-1",
                "description":"<p>An aside <!-- like this --> in the words.</p>"}"#,
        );
        let document = format!("<html>{describes}{}</html>", script(REAL_BLOCK));
        assert_eq!(job_postings(&document).len(), 2);
    }
}

//! The desktop app's thin waist.
//!
//! Everything here is a translation layer: `perch-core` decides, this crate
//! shapes the answer for the interface. Two rules hold it in place.
//!
//! Nothing from a job board ever reaches the webview as markup. Descriptions
//! arrive as [`BlockDto`]s that React renders as text nodes, so a posting
//! cannot carry a script tag into an app holding someone's profile.
//!
//! And there is no command here that submits anything. There is no such
//! primitive in `perch-core` to call, so the promise is structural.

use perch_core::{
    ats::adapter_for, detect_board, ensure_description, html, rules::Rules, sync::sync_all,
    ApplicationState, FeedFilter, Http, Paths, Profile, Role, Store,
};
use perch_llm::{document as llm_document, extract as llm_extract, Permission, Proposal};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use tauri::{Manager, State};
use time::OffsetDateTime;

pub struct App {
    store: Mutex<Store>,
    paths: Paths,
    http: Http,
    /// What the last résumé read proposed, kept here so a decision about one
    /// arrives as an answer about something Perch verified rather than as a
    /// value the webview supplied.
    import: Mutex<Vec<Proposal>>,
}

/// Errors reach the interface as one plain sentence, in the app's own voice.
type Answer<T> = Result<T, String>;

fn plainly(err: impl std::fmt::Display) -> String {
    err.to_string()
}

// ---- what the interface receives -----------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDto {
    pub reference: String,
    pub company: String,
    pub title: String,
    pub location: String,
    pub url: String,
    pub ats: String,
    pub fill_supported: bool,
    /// Already written for a person: "posted 6 hours ago", "open 143 days".
    pub signal: String,
    /// fresh · recent · settled · stale · tired
    pub freshness: String,
    /// Today · This week · Earlier this month · Older
    pub bucket: String,
    pub why_rule: Option<String>,
    pub why_because: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDto {
    /// heading · paragraph · bullet. Never markup.
    pub kind: String,
    pub text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    pub when: String,
    pub what: String,
    pub latest: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VelocityDto {
    pub opened_recently: usize,
    pub still_open: usize,
    pub median_days_to_close: Option<i64>,
    /// The sentence that bounds the claim.
    pub caveat: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailDto {
    pub role: RoleDto,
    pub blocks: Vec<BlockDto>,
    /// Present only when the board still has the posting's text.
    pub description_missing: bool,
    pub history: Vec<EventDto>,
    pub velocity: Option<VelocityDto>,
    pub applied: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardDto {
    pub company: String,
    pub key: String,
    pub ats: String,
    pub url: String,
    pub checked: String,
    pub fill_supported: bool,
    pub history: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDto {
    pub reference: String,
    pub company: String,
    pub title: String,
    pub state: String,
    pub heading: String,
    pub applied: String,
    pub note: Option<String>,
    pub archived_quietly: bool,
    pub freshness: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDto {
    pub lines: Vec<String>,
    pub failures: Vec<String>,
    pub quiet: bool,
    pub boards_read: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDto {
    pub name: String,
    pub conditions: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    pub configured: bool,
    pub name: String,
    pub endpoint: String,
    /// Where things stand, in one sentence, at the point of use.
    pub consequence: String,
    /// True only when the endpoint is on this machine.
    pub local: bool,
    /// Remote and not consented: Perch will not send the résumé.
    pub refused: bool,
    /// Models Ollama on this Mac reports, if it is running.
    pub on_this_mac: Vec<String>,
    pub ollama_running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub profile: Profile,
    pub profile_path: String,
    pub rules_path: String,
    pub database_path: String,
    pub rules: Vec<RuleDto>,
    /// Set when the rules file will not parse. The feed still works.
    pub rules_error: Option<String>,
    pub model: ModelDto,
    pub model_path: String,
    /// The document an application attaches unless told otherwise. Decided in
    /// `perch-core` so both front ends cannot drift onto different files.
    pub preferred_resume: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedDto {
    pub roles: Vec<RoleDto>,
    /// Roles open but held back by the rules, for the empty state's sentence.
    pub open_but_unmatched: usize,
    pub has_boards: bool,
    pub rules_error: Option<String>,
}

// ---- shaping --------------------------------------------------------------

fn role_dto(role: &Role, why: Option<&perch_core::Why>, now: OffsetDateTime) -> RoleDto {
    let signal = role.signal(now);
    RoleDto {
        reference: role.reference.clone(),
        company: role.company_name.clone(),
        title: role.title.clone(),
        location: role.location.clone(),
        url: role.url.clone(),
        ats: role.ats.label().to_string(),
        fill_supported: role.fill_supported,
        signal: signal.text.clone(),
        freshness: signal.freshness(now).class().to_string(),
        bucket: signal.bucket(now).heading().to_string(),
        why_rule: why.map(|w| w.rule.clone()),
        why_because: why.map(|w| w.because.clone()),
    }
}

// ---- commands -------------------------------------------------------------

#[tauri::command]
fn feed(state: State<'_, App>, fresh: bool, company: Option<String>, all: bool) -> Answer<FeedDto> {
    let now = OffsetDateTime::now_utc();
    let store = state.store.lock().map_err(plainly)?;

    let slug = match company.as_deref() {
        Some(name) => store
            .watched_company(name)
            .map_err(plainly)?
            .map(|c| c.slug),
        None => None,
    };
    let open = store
        .feed(
            &FeedFilter {
                company: slug,
                fresh_only: fresh,
            },
            now,
        )
        .map_err(plainly)?;
    let open_count = open.len();

    // A rules file that will not parse must not take the feed away.
    let mut rules_error = None;
    let rules = if all {
        Rules::default()
    } else {
        match Rules::load(&state.paths.rules()) {
            Ok(rules) => rules,
            Err(err) => {
                rules_error = Some(plainly(err));
                Rules::default()
            }
        }
    };

    let matched = rules.apply(open);
    Ok(FeedDto {
        roles: matched
            .iter()
            .map(|(role, why)| role_dto(role, why.as_ref(), now))
            .collect(),
        open_but_unmatched: open_count.saturating_sub(matched.len()),
        has_boards: !store.boards().map_err(plainly)?.is_empty(),
        rules_error,
    })
}

#[tauri::command]
fn detail(state: State<'_, App>, reference: String) -> Answer<DetailDto> {
    let now = OffsetDateTime::now_utc();
    let store = state.store.lock().map_err(plainly)?;
    let role = store.role_by_reference(&reference).map_err(plainly)?;

    // Fetched the first time someone opens it, then kept.
    let text = ensure_description(&store, &role, &state.http, now).map_err(plainly)?;
    let blocks = text
        .as_deref()
        .map(html::to_blocks)
        .unwrap_or_default()
        .into_iter()
        .map(|b| BlockDto {
            kind: match b {
                html::Block::Heading(_) => "heading",
                html::Block::Bullet(_) => "bullet",
                html::Block::Paragraph(_) => "paragraph",
            }
            .to_string(),
            text: b.text().to_string(),
        })
        .collect();

    let events = store.events(role.id).map_err(plainly)?;
    let last = events.len().saturating_sub(1);
    let history = events
        .iter()
        .enumerate()
        .map(|(i, event)| EventDto {
            when: perch_core::verb_signal("", event.at, now)
                .trim()
                .to_string(),
            what: match (event.kind, event.detail.as_deref()) {
                (perch_core::EventKind::FirstSeen, _) => "Perch first saw it".into(),
                (perch_core::EventKind::Reposted, _) => "the board reposted it".into(),
                (perch_core::EventKind::Retitled, Some(d)) => format!("retitled: {d}"),
                (perch_core::EventKind::Retitled, None) => "retitled".into(),
                (perch_core::EventKind::Relocated, Some(d)) => format!("moved: {d}"),
                (perch_core::EventKind::Relocated, None) => "moved".into(),
                (perch_core::EventKind::Closed, _) => "it came down".into(),
                (perch_core::EventKind::Reopened, _) => "it went back up".into(),
            },
            latest: i == last,
        })
        .collect();

    let velocity = store
        .velocity(role.board_id, now)
        .map_err(plainly)?
        .map(|v| VelocityDto {
            opened_recently: v.opened_recently,
            still_open: v.still_open,
            median_days_to_close: v.median_days_to_close,
            caveat: format!(
                "Observed, not reported. Perch has only been watching this board for {}.",
                perch_core::span(now - v.watching_since)
            ),
        });

    let applied = store
        .applications()
        .map_err(plainly)?
        .into_iter()
        .find(|a| a.role_id == role.id)
        .map(|a| a.state.heading().to_string());

    Ok(DetailDto {
        role: role_dto(&role, None, now),
        blocks,
        description_missing: text.is_none(),
        history,
        velocity,
        applied,
    })
}

#[tauri::command]
fn dismiss(state: State<'_, App>, reference: String) -> Answer<()> {
    let store = state.store.lock().map_err(plainly)?;
    let role = store.role_by_reference(&reference).map_err(plainly)?;
    store
        .dismiss(role.id, OffsetDateTime::now_utc())
        .map_err(plainly)
}

#[tauri::command]
fn restore(state: State<'_, App>, reference: String) -> Answer<()> {
    let store = state.store.lock().map_err(plainly)?;
    let role = store.role_by_reference(&reference).map_err(plainly)?;
    store.undismiss(role.id).map_err(plainly)
}

#[tauri::command]
fn sync(state: State<'_, App>) -> Answer<SyncDto> {
    let now = OffsetDateTime::now_utc();
    let mut store = state.store.lock().map_err(plainly)?;
    let outcome = sync_all(&mut store, &state.http, now).map_err(plainly)?;

    let mut lines = Vec::new();
    for report in &outcome.reports {
        if report.quiet() {
            continue;
        }
        let mut parts = Vec::new();
        for (n, word) in [
            (report.first_seen, "new"),
            (report.reposted, "reposted"),
            (report.retitled, "retitled"),
            (report.relocated, "moved"),
            (report.reopened, "reopened"),
            (report.closed, "came down"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {word}"));
            }
        }
        lines.push(format!("{}: {}", report.company, parts.join(", ")));
    }

    Ok(SyncDto {
        lines,
        failures: outcome
            .failures
            .iter()
            .map(|(name, why)| format!("{name} could not be reached: {why}"))
            .collect(),
        quiet: outcome.quiet(),
        boards_read: outcome.reports.len(),
    })
}

/// When Perch last read the boards, for the line under the rail.
///
/// Asked of the store rather than remembered in the interface, because a fresh
/// launch has nothing remembered and said so: the rail claimed nothing had ever
/// synced while the watchlist beside it said each board was read minutes ago.
#[tauri::command]
fn last_read(state: State<'_, App>) -> Answer<String> {
    let now = OffsetDateTime::now_utc();
    let store = state.store.lock().map_err(plainly)?;
    let freshest = store
        .boards()
        .map_err(plainly)?
        .into_iter()
        .filter_map(|board| board.last_checked_at)
        .max();
    Ok(match freshest {
        Some(at) => perch_core::verb_signal("read", at, now),
        // No board has been read, which is also true of a Perch with no boards.
        None => "not read yet".to_string(),
    })
}

#[tauri::command]
fn watchlist(state: State<'_, App>) -> Answer<Vec<BoardDto>> {
    let now = OffsetDateTime::now_utc();
    let store = state.store.lock().map_err(plainly)?;
    store
        .boards()
        .map_err(plainly)?
        .into_iter()
        .map(|board| {
            let (seen, open) = store.board_tally(board.id).map_err(plainly)?;
            Ok(BoardDto {
                key: board.token.clone(),
                company: board.company_name.clone(),
                ats: board.ats.label().to_string(),
                url: board.url.clone(),
                checked: match board.last_checked_at {
                    Some(at) => perch_core::verb_signal("checked", at, now),
                    None => "not read yet".to_string(),
                },
                fill_supported: board.fill_supported,
                history: if seen == 0 {
                    "Nothing seen here yet.".to_string()
                } else {
                    format!(
                        "{seen} {} seen since Perch started watching, {open} still open.",
                        if seen == 1 { "role" } else { "roles" }
                    )
                },
            })
        })
        .collect()
}

#[tauri::command]
fn watch_add(state: State<'_, App>, input: String) -> Answer<String> {
    let now = OffsetDateTime::now_utc();
    let found = detect_board(&input, &state.http)
        .map_err(plainly)?
        .ok_or_else(|| format!("No public board found for {input}."))?;
    let mut store = state.store.lock().map_err(plainly)?;
    let board = store.watch(&found, now).map_err(plainly)?;
    Ok(board.company_name)
}

#[tauri::command]
fn watch_remove(state: State<'_, App>, key: String) -> Answer<bool> {
    let store = state.store.lock().map_err(plainly)?;
    store
        .unwatch(&key, OffsetDateTime::now_utc())
        .map_err(plainly)
}

#[tauri::command]
fn applications(state: State<'_, App>) -> Answer<Vec<ApplicationDto>> {
    let now = OffsetDateTime::now_utc();
    let store = state.store.lock().map_err(plainly)?;
    // Applications with no reply in thirty days are archived before anything
    // is shown.
    store.archive_quiet_applications(now).map_err(plainly)?;
    Ok(store
        .applications()
        .map_err(plainly)?
        .into_iter()
        .map(|a| {
            let at = a.note_at.unwrap_or(a.applied_at);
            ApplicationDto {
                reference: a.reference,
                company: a.company_name,
                title: a.title,
                state: a.state.as_str().to_string(),
                heading: a.state.heading().to_string(),
                applied: perch_core::verb_signal("applied", a.applied_at, now),
                note: a.note,
                archived_quietly: a.archived_quietly,
                freshness: perch_core::Freshness::at(at, now).class().to_string(),
            }
        })
        .collect())
}

#[tauri::command]
fn mark_application(
    state: State<'_, App>,
    reference: String,
    app_state: String,
    note: Option<String>,
) -> Answer<()> {
    let store = state.store.lock().map_err(plainly)?;
    let role = store.role_by_reference(&reference).map_err(plainly)?;
    let parsed = ApplicationState::parse(&app_state).map_err(plainly)?;
    store
        .mark_application(role.id, parsed, note.as_deref(), OffsetDateTime::now_utc())
        .map_err(plainly)
}

/// Where the model stands, for a screen that shows it.
///
/// `on_this_mac` is what Ollama here reports, and `None` means Ollama is not
/// running. Nothing in this answer comes from the keychain. Saying whether a
/// key is held would mean reading one, and reading one raises a password
/// prompt on a screen nobody opened to be prompted on.
fn model_dto(model: &perch_llm::Model, on_this_mac: Option<Vec<String>>) -> ModelDto {
    let permission = model.may_send_document();
    ModelDto {
        configured: model.configured(),
        name: model.model.clone(),
        endpoint: model.endpoint.clone(),
        consequence: model.consequence(),
        local: matches!(permission, Permission::Local),
        refused: matches!(permission, Permission::Refused { .. }),
        ollama_running: on_this_mac.is_some(),
        on_this_mac: on_this_mac.unwrap_or_default(),
    }
}

#[tauri::command]
fn settings(state: State<'_, App>) -> Answer<SettingsDto> {
    let profile = Profile::load(&state.paths.profile()).map_err(plainly)?;
    let (rules, rules_error) = match Rules::load(&state.paths.rules()) {
        Ok(rules) => (
            rules
                .rules
                .iter()
                .map(|r| RuleDto {
                    name: r.name.trim().to_string(),
                    conditions: [
                        ("title contains", &r.title, false),
                        ("title excludes", &r.title_excludes, true),
                        ("location contains", &r.location, false),
                        ("location excludes", &r.location_excludes, true),
                        ("company is", &r.company, false),
                    ]
                    .iter()
                    .filter(|(_, items, _)| !items.is_empty())
                    .map(|(label, items, negative)| {
                        let joined = if *negative {
                            items.join(", ")
                        } else {
                            match items.as_slice() {
                                [one] => one.clone(),
                                [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
                                [] => String::new(),
                            }
                        };
                        format!("{label} {joined}")
                    })
                    .collect(),
                })
                .collect(),
            None,
        ),
        Err(err) => (Vec::new(), Some(plainly(err))),
    };

    let configured = perch_llm::Model::load(&state.paths.model()).map_err(plainly)?;
    let ollama = perch_llm::Client::new().ok().and_then(|c| c.probe_ollama());

    Ok(SettingsDto {
        model: model_dto(
            &configured,
            ollama.map(|models| models.into_iter().map(|m| m.name).collect()),
        ),
        model_path: state.paths.model().display().to_string(),
        preferred_resume: profile.preferred_resume().map(|d| d.path.clone()),
        profile,
        profile_path: state.paths.profile().display().to_string(),
        rules_path: state.paths.rules().display().to_string(),
        database_path: state.paths.db().display().to_string(),
        rules,
        rules_error,
    })
}

// ---- the key for a remote endpoint ---------------------------------------
//
// macOS scopes a keychain item's access control list to the program that
// created it. A key stored by `perch model key` belongs to the command line
// binary, and this app reading that item puts a system password prompt in
// front of someone who only asked to read a résumé. Stored from here, the app
// is on that list from the start and there is no prompt.
//
// The key travels one way: the field on the profile screen, into one of these
// commands, into the keychain. Nothing hands it back, no DTO carries it, and
// it reaches neither model.toml nor an error message.

/// Put the key for the configured endpoint in this Mac's keychain.
///
/// What comes back names the host and says where the key went. It says nothing
/// about whether the endpoint accepts the key, because nothing has been asked
/// of the endpoint.
fn key_set(
    model: &perch_llm::Model,
    key: &str,
    into_keychain: impl Fn(&str, &str) -> Result<(), String>,
) -> Answer<String> {
    let Some(host) = model.host() else {
        return Err("No endpoint is configured, so there is nothing to hold a key for.".into());
    };
    // A model on this Mac is sent no key, so storing one would be a secret
    // kept for nothing, and a screen saying it did something it did not do.
    if model.is_local() {
        return Err(format!("{host} is on this Mac and wants no key."));
    }
    let key = key.trim();
    if key.is_empty() {
        return Err("Nothing was typed, so nothing was stored.".into());
    }
    into_keychain(&host, key)?;
    Ok(format!("Stored in this Mac's keychain, for {host}."))
}

/// Take it out again. A host holding no key is the state being asked for, not
/// a failure.
fn key_forget(
    model: &perch_llm::Model,
    from_keychain: impl Fn(&str) -> Result<(), String>,
) -> Answer<String> {
    let Some(host) = model.host() else {
        return Err("No endpoint is configured, so there is nothing to hold a key for.".into());
    };
    from_keychain(&host)?;
    Ok(format!("Perch is holding no key for {host}."))
}

#[tauri::command]
fn model_key_set(state: State<'_, App>, key: String) -> Answer<String> {
    let model = perch_llm::Model::load(&state.paths.model()).map_err(plainly)?;
    key_set(&model, &key, perch_llm::client::secret::set)
}

#[tauri::command]
fn model_key_forget(state: State<'_, App>) -> Answer<String> {
    let model = perch_llm::Model::load(&state.paths.model()).map_err(plainly)?;
    key_forget(&model, perch_llm::client::secret::forget)
}

// ---- reading a résumé into proposed fields --------------------------------
//
// The order here is the command line's order, for the same reason: the consent
// gate is settled before the document is read, so a résumé that may not be
// sent is not opened either. Every value the model returns is checked back
// against the document by `perch-llm`, and one that anchors nowhere reaches
// this screen with no way to accept it.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReadyDto {
    pub configured: bool,
    /// local · remote · refused · none
    pub verdict: String,
    /// The host a résumé would be sent to, when it would leave this Mac.
    pub host: Option<String>,
    /// What reading a résumé would do, said before a file is chosen.
    pub sentence: String,
    /// Where a person changes that answer.
    pub remedy: Option<String>,
    pub profile_path: String,
}

/// One proposed field, with the evidence for it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalDto {
    /// Its place in the read, which is how a decision names it later.
    pub id: usize,
    pub label: String,
    pub value: String,
    /// What the profile holds today, if anything.
    pub current: Option<String>,
    /// The document line the value was found on, quoted whole.
    pub quote: Option<String>,
    /// The matched span within `quote`, counted in UTF-16 code units.
    pub highlight: Option<(usize, usize)>,
    pub line: Option<usize>,
    /// False means there is no Accept control for this one at all.
    pub offerable: bool,
    /// True only for a quotation. Anything read rather than quoted waits.
    pub accepted: bool,
    /// Why a loose match wants a second look.
    pub note: Option<String>,
    /// Why this one is not offered.
    pub refusal: Option<String>,
    /// The values this row writes, when it writes more than one. Editing the
    /// row edits all of them, so the screen has to name them.
    pub parts: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDto {
    /// The file's own name, for the lede.
    pub document: String,
    /// Where the reading happened, and whether anything left this Mac.
    pub provenance: String,
    pub proposals: Vec<ProposalDto>,
    /// Set when nothing the model returned is supported by the document.
    pub nothing_anchored: Option<String>,
    pub profile_path: String,
}

/// What a person decided about one row.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionDto {
    pub id: usize,
    pub accepted: bool,
    /// The person's own words, when they typed over the proposed value.
    pub value: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportWrittenDto {
    pub written: usize,
    pub profile_path: String,
}

/// The verdict, the host it is about, and where a person changes it.
///
/// The sentence itself comes from `perch-llm`, so the app and the command line
/// say the same thing about the same endpoint.
fn stance(model: &perch_llm::Model) -> (String, Option<String>, Option<String>) {
    // The remedy names the command that changes the answer. Perch has no
    // settings screen, and pointing at one that is not there is worse than
    // saying nothing: a person goes looking for it.
    match model.may_send_document() {
        Permission::NoModel => (
            "none".into(),
            None,
            Some("perch model list  shows what this Mac can run.".into()),
        ),
        Permission::Local => ("local".into(), None, None),
        Permission::ConsentedRemote { host } => ("remote".into(), Some(host), None),
        Permission::Refused { host } => (
            "refused".into(),
            Some(host),
            Some(format!(
                "perch model endpoint {} --allow-resume  says it may.",
                model.endpoint.trim()
            )),
        ),
    }
}

/// The consent gate, and then the document, in that order.
///
/// The screen having shown the same verdict is not a gate. A résumé that may
/// not be sent is not read off the disk here either.
fn document_for(model: &perch_llm::Model, path: &Path) -> Result<perch_llm::Document, String> {
    match model.may_send_document() {
        Permission::Local | Permission::ConsentedRemote { .. } => {
            llm_document::read(path).map_err(plainly)
        }
        Permission::NoModel | Permission::Refused { .. } => Err(model.consequence()),
    }
}

/// Where the reading happened, for the line above the proposals.
fn provenance(model: &perch_llm::Model) -> String {
    match model.may_send_document() {
        Permission::Local => format!(
            "Read on this Mac by {}. Nothing left this Mac.",
            model.model
        ),
        Permission::ConsentedRemote { host } => format!(
            "Read by {} at {host}. Your résumé left this Mac.",
            model.model
        ),
        // Not reached: nothing is read until the gate above has passed.
        Permission::NoModel | Permission::Refused { .. } => model.consequence(),
    }
}

/// The API key for this endpoint, if one is wanted at all.
///
/// A local endpoint is never asked about. Consulting the keychain for a model
/// running on this Mac puts a system password prompt in front of someone whose
/// résumé is not going anywhere.
fn key_for(
    model: &perch_llm::Model,
    from_keychain: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    model
        .host()
        .filter(|_| !model.is_local())
        .and_then(|host| from_keychain(&host))
}

/// A span in `quote`, moved from bytes to UTF-16 code units.
///
/// The webview counts string offsets the way JavaScript does. A span measured
/// in bytes highlights the wrong characters as soon as the line holds an em
/// dash or an accent, which a résumé line usually does.
fn utf16_span(quote: &str, (from, to): (usize, usize)) -> Option<(usize, usize)> {
    let at = |byte: usize| Some(quote.get(..byte)?.encode_utf16().count());
    Some((at(from)?, at(to)?))
}

fn proposal_dto(id: usize, proposal: &Proposal, document: &str) -> ProposalDto {
    let quote = proposal.quote.clone();
    ProposalDto {
        id,
        label: proposal.field.label(),
        value: proposal.value.clone(),
        current: proposal.current.clone(),
        highlight: quote
            .as_deref()
            .zip(proposal.highlight)
            .and_then(|(quote, span)| utf16_span(quote, span)),
        quote,
        line: proposal.line,
        offerable: proposal.offerable(),
        accepted: proposal.accepted && proposal.offerable(),
        note: proposal.caution(),
        refusal: proposal.refusal(document),
        parts: proposal.field.parts().map(str::to_string),
    }
}

/// Fold a person's decisions into the proposals Perch verified.
///
/// A decision names a row. It cannot bring evidence of its own, so a row Perch
/// did not offer stays unaccepted whatever the decision says, and cannot be
/// typed over either.
fn decide(proposals: &mut [Proposal], decisions: &[DecisionDto]) {
    for proposal in proposals.iter_mut() {
        proposal.accepted = false;
    }
    for decision in decisions {
        let Some(proposal) = proposals.get_mut(decision.id) else {
            continue;
        };
        if !proposal.offerable() {
            continue;
        }
        if let Some(value) = decision
            .value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            proposal.edit(value);
        }
        proposal.accepted = decision.accepted;
    }
}

/// What choosing a file would do, before one is chosen.
#[tauri::command]
fn import_ready(state: State<'_, App>) -> Answer<ImportReadyDto> {
    let model = perch_llm::Model::load(&state.paths.model()).map_err(plainly)?;
    let (verdict, host, remedy) = stance(&model);
    Ok(ImportReadyDto {
        configured: model.configured(),
        verdict,
        host,
        sentence: model.consequence(),
        remedy,
        profile_path: state.paths.profile().display().to_string(),
    })
}

/// Read a résumé into proposed fields. Nothing is written here.
#[tauri::command]
fn import_read(state: State<'_, App>, path: String) -> Answer<ImportDto> {
    let model = perch_llm::Model::load(&state.paths.model()).map_err(plainly)?;
    let document = document_for(&model, Path::new(&path))?;

    let profile_path = state.paths.profile();
    let profile = Profile::load(&profile_path).map_err(plainly)?;

    let key = key_for(&model, perch_llm::client::secret::get);
    let client = perch_llm::Client::new().map_err(plainly)?;
    let proposals =
        llm_extract::run(&client, &model, key.as_deref(), &document, &profile).map_err(plainly)?;

    let shown = proposals
        .iter()
        .enumerate()
        .map(|(id, proposal)| proposal_dto(id, proposal, &document.name))
        .collect();
    let nothing_anchored = proposals.is_empty().then(|| {
        format!(
            "Nothing in {} anchored to text Perch could find in it.",
            document.name
        )
    });
    *state.import.lock().map_err(plainly)? = proposals;

    Ok(ImportDto {
        document: document.name,
        provenance: provenance(&model),
        proposals: shown,
        nothing_anchored,
        profile_path: profile_path.display().to_string(),
    })
}

/// Write the accepted fields, and only those.
#[tauri::command]
fn import_write(state: State<'_, App>, decisions: Vec<DecisionDto>) -> Answer<ImportWrittenDto> {
    let mut proposals = state.import.lock().map_err(plainly)?;
    if proposals.is_empty() {
        return Err("There is nothing to write. No résumé has been read.".into());
    }
    decide(&mut proposals, &decisions);

    let profile_path = state.paths.profile();
    let mut profile = Profile::load(&profile_path).map_err(plainly)?;
    let written = llm_extract::apply(&proposals, &mut profile);
    if written > 0 {
        profile.save(&profile_path).map_err(plainly)?;
        // The review is over, and the résumé's text has no further use here.
        proposals.clear();
    }

    Ok(ImportWrittenDto {
        written,
        profile_path: profile_path.display().to_string(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FillPlanDto {
    pub plan: serde_json::Value,
    /// The sentence stating exactly what will happen, and where it stops.
    pub what_happens_next: String,
    /// Some boards run their own file picker and will not take a file from it.
    pub attachment_caveat: Option<String>,
    pub company: String,
    pub title: String,
    /// Absent when this board's forms are not one Perch can fill.
    pub fillable: bool,
}

/// What Perch would type into this role's form, for a person to read first.
///
/// Building a plan touches nothing: no browser, no network, no file.
#[tauri::command]
fn fill_plan(
    state: State<'_, App>,
    reference: String,
    resume: Option<String>,
) -> Answer<FillPlanDto> {
    let store = state.store.lock().map_err(plainly)?;
    let role = store.role_by_reference(&reference).map_err(plainly)?;
    let profile = Profile::load(&state.paths.profile()).map_err(plainly)?;

    let Some(plan) = perch_fill::plan::build(role.ats, &role.url, &profile, resume.as_deref())
    else {
        return Ok(FillPlanDto {
            plan: serde_json::Value::Null,
            what_happens_next: format!(
                "Perch cannot fill {}'s forms, so this one opens in your browser and you fill it there.",
                role.ats.label()
            ),
            attachment_caveat: None,
            company: role.company_name,
            title: role.title,
            fillable: false,
        });
    };

    Ok(FillPlanDto {
        what_happens_next: plan.what_happens_next(&role.company_name),
        attachment_caveat: plan.attachment_caveat(),
        plan: serde_json::to_value(&plan).map_err(plainly)?,
        company: role.company_name,
        title: role.title,
        fillable: true,
    })
}

/// The file's type, from its name. Asserting a type the file does not have
/// would tell the employer's page something untrue about it.
fn mime_for(path: &str) -> String {
    match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("doc") => "application/msword",
        Some("rtf") => "application/rtf",
        Some("txt") => "text/plain",
        Some("md" | "markdown") => "text/markdown",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// What the fill said it managed to do, written for a person to read.
///
/// Perch decides nothing from this. The script that writes it runs in a page
/// Perch does not control, and that page can put anything on `window` that the
/// script can, so a report is something to show and never something to act on.
/// Every guard on where the typing happens is settled before this exists, and
/// none of them is revisited afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillReportDto {
    /// The role this is about, so one window's account is not read as another's.
    /// Filled in here; the page has no say in it.
    #[serde(default)]
    pub reference: String,
    /// The mark the fill leaves on its own account. It is compared here and
    /// nowhere else, so it is not part of what the sheet is handed.
    #[serde(default, skip_serializing)]
    pub stamp: String,
    /// Whether the form's window was still open when this was read. Stamped
    /// here from the app's own list of windows, because the page cannot be
    /// asked and a sentence about an open form should not describe a closed
    /// one.
    #[serde(default)]
    pub window_open: bool,
    /// Planned values sitting in their boxes on the form.
    pub filled: usize,
    pub planned: usize,
    /// The labels of the boxes that were not on the form.
    pub missing: Vec<String>,
    /// The labels of the boxes that are on the form and hold something other
    /// than what Perch typed.
    #[serde(default)]
    pub changed: Vec<String>,
    /// A file was planned, whether Perch got it onto a box, and whether the
    /// form printed its name.
    pub file: bool,
    #[serde(default)]
    pub file_given: bool,
    pub file_named: bool,
    /// Whether the fill had finished when this was read.
    #[serde(default)]
    pub settled: bool,
    /// Why nothing happened, when something stopped it.
    pub error: Option<String>,
}

impl FillReportDto {
    /// A fill that never started, said in the same shape as one that did.
    fn stopped(reference: &str, why: String) -> Self {
        Self {
            reference: reference.to_string(),
            stamp: String::new(),
            window_open: false,
            filled: 0,
            planned: 0,
            missing: Vec::new(),
            changed: Vec::new(),
            file: false,
            file_given: false,
            file_named: false,
            settled: true,
            error: Some(why),
        }
    }

    /// Cut to a size a sentence can hold. The words come from the page, and a
    /// page is free to answer with a megabyte.
    fn trimmed(mut self) -> Self {
        self.missing.truncate(24);
        self.changed.truncate(24);
        for label in self.missing.iter_mut().chain(self.changed.iter_mut()) {
            *label = shortened(label, 120);
        }
        self.error = self.error.map(|why| shortened(&why, 200));
        self
    }
}

/// The first characters of something the page wrote.
///
/// Counted in characters rather than bytes: the words are the page's, a page
/// is free to answer in any script it likes, and a cut between the bytes of
/// one character is a panic in the middle of a callback that has no one to
/// catch it.
fn shortened(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// Asks the page for the account the fill left on it.
const FILL_REPORT_READER: &str = "(function () { try { \
     return JSON.stringify(window.__perchFill || null); \
     } catch (err) { return 'null'; } })();";

/// The event the sheet is waiting on.
const FILL_REPORT_EVENT: &str = "fill-report";

/// Reads a report out of whatever the evaluator handed back.
///
/// The evaluated value arrives serialised as JSON, so a script that returned a
/// string arrives quoted. That layer is unwrapped when it is there.
fn read_report(raw: &str, reference: &str) -> Option<FillReportDto> {
    let text = serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_string());
    let mut report: FillReportDto = serde_json::from_str(&text).ok()?;
    report.reference = reference.to_string();
    Some(report.trimmed())
}

/// What is left to say when the page has no account of the fill on it.
const NOT_THAT_PAGE: &str = "The window is not showing the form Perch filled any more, \
     so there is nothing left to read back from it.";

/// Hands a report to the window the person is looking at.
fn tell(app: &tauri::AppHandle, of_window: &str, mut report: FillReportDto) {
    use tauri::Emitter;
    report.window_open = app.get_webview_window(of_window).is_some();
    // Nothing waits on this. A report that cannot be delivered is one the
    // sheet hears nothing about, and saying so is what the sheet does then.
    let _ = app.emit_to("main", FILL_REPORT_EVENT, report);
}

/// What pressing Open and fill came to.
///
/// Opening a window is not a fill, and what went into the form is said later
/// on the `fill-report` event. This is only what happened in the moment.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Started {
    /// A window opened and the fill runs in it. A report follows.
    Filling,
    /// This role's form was already open, so it was brought forward and
    /// nothing was typed again. No report follows.
    AlreadyOpen,
    /// There is nothing on this board Perch can fill, so the page went to the
    /// browser. No report follows.
    Browser,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FillStartedDto {
    pub started: Started,
    /// A sentence to show beside the outcome, when there is one.
    pub caveat: Option<String>,
}

/// Open the form and type the plan into it.
///
/// This is where Perch stops. The window is left open, filled in, with the
/// person in front of it. There is no follow-up command, and `perch-fill` has
/// no action that could activate a control.
///
/// Two things guard *where* the typing happens, because the URL comes out of a
/// board's own JSON and is therefore remote data:
///
/// * the destination must be a page the ATS actually serves forms from, and
/// * the script runs once, on that page, and never again. A posting that
///   redirects, or a link clicked inside the window, must not be handed
///   someone's profile and résumé.
///
/// Returning here means the window opened, not that anything was typed. The
/// fill runs in the page afterwards and says how it went in a [`FillReportDto`]
/// on the `fill-report` event. `Some(caveat)` is a sentence to show beside it.
///
/// The report is the only thing Perch asks this page for, and it comes back
/// through `eval_with_callback`, whose callback runs here in Rust. Perch adds
/// nothing to the page to carry it: no initialisation script, no channel of
/// its own.
///
/// Tauri puts `__TAURI_INTERNALS__` and its invoke bridge into every webview it
/// builds, this one included, so the board's page can see that bridge and the
/// key that goes with it. What keeps it from reaching a command is the ACL:
/// `capabilities/main.json` names the `main` window and nothing else, and a
/// request from a remote origin is refused unless a capability allows it. A
/// capability written later with a `remote` scope, or with `"windows": ["*"]`,
/// would hand a job board Perch's commands, which would be a far worse thing
/// than a form nobody heard back about.
#[tauri::command]
fn open_and_fill(
    app: tauri::AppHandle,
    state: State<'_, App>,
    reference: String,
    resume: Option<String>,
) -> Answer<FillStartedDto> {
    use base64::Engine;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let (url, script, label, ats, skipped) = {
        let store = state.store.lock().map_err(plainly)?;
        let role = store.role_by_reference(&reference).map_err(plainly)?;
        let profile = Profile::load(&state.paths.profile()).map_err(plainly)?;

        let Some(plan) = perch_fill::plan::build(role.ats, &role.url, &profile, resume.as_deref())
        else {
            // Nothing to fill here; hand them the page and stop.
            return open_in_browser(app, role.url).map(|()| FillStartedDto {
                started: Started::Browser,
                caveat: None,
            });
        };

        // The board said where this form lives. Taking that on trust would mean
        // typing a résumé wherever a stranger's JSON pointed.
        if !perch_fill::flavor::may_fill(role.ats, &role.url) {
            return Err(format!(
                "{} is not an address {} serves application forms from. Perch will not type anything into it.",
                role.url,
                role.ats.label()
            ));
        }

        // Read the chosen document so the page can be given a real file. A
        // document that has moved since the profile named it costs the
        // attachment, not the whole application.
        let mut files = std::collections::BTreeMap::new();
        let mut skipped = None;
        if let Some(path) = resume.as_deref() {
            for entry in &plan.entries {
                if let perch_fill::Action::AttachFile { selector, .. } = &entry.action {
                    match std::fs::read(path) {
                        Ok(bytes) => {
                            files.insert(
                                selector.clone(),
                                perch_fill::script::Attachment {
                                    name: std::path::Path::new(path)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("resume")
                                        .to_string(),
                                    mime: mime_for(path),
                                    base64: base64::engine::general_purpose::STANDARD
                                        .encode(&bytes),
                                },
                            );
                        }
                        Err(_) => skipped = Some(path.to_string()),
                    }
                }
            }
        }

        (
            role.url.clone(),
            perch_fill::to_script(&plan, &files),
            // A label per role, so a form left open does not stop the next
            // application from opening.
            format!(
                "fill-{}",
                reference
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
            ),
            role.ats,
            skipped,
        )
    };

    let parsed: tauri::Url = url
        .parse()
        .map_err(|_| format!("{url} is not a web address Perch can open"))?;

    // If this role's window is already open, bring it forward rather than
    // failing: the person may simply have come back to it. Nothing is
    // dispatched, so nothing will answer, and the sheet is told that here
    // rather than waiting out a settle window that never started.
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.set_focus();
        return Ok(FillStartedDto {
            started: Started::AlreadyOpen,
            caveat: None,
        });
    }

    let intended = url.clone();
    let allowed_ats = ats;
    let navigation_ats = ats;
    let fired = Arc::new(AtomicBool::new(false));
    let handle = app.clone();
    let of_role = reference.clone();
    let of_window = label.clone();
    // The first pass, kept for the case where the settle window ends with
    // nobody left to ask: a window the person has already closed still had
    // something to say about what it managed to type.
    let first_pass: Arc<Mutex<Option<FillReportDto>>> = Arc::new(Mutex::new(None));

    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::External(parsed))
        .title("Fill: you submit this yourself")
        .inner_size(1080.0, 900.0)
        .on_navigation(move |next| {
            // The window may follow the board's own redirects within the ATS,
            // and nowhere else. It is holding a filled-in form.
            perch_fill::flavor::may_fill(navigation_ats, next.as_str())
        })
        .on_page_load(move |webview, payload| {
            if !matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                return;
            }
            // `on_page_load` fires for every navigation, not once. Without
            // both of these, clicking a link in the filled window (or a closed
            // posting redirecting to a careers index) would type the profile
            // and the résumé into a page nobody reviewed.
            if !perch_fill::flavor::is_the_reviewed_page(
                allowed_ats,
                payload.url().as_str(),
                &intended,
            ) {
                return;
            }
            if fired.swap(true, Ordering::SeqCst) {
                return;
            }

            let keeping = first_pass.clone();
            let of_first = of_role.clone();
            let dispatched = webview.eval_with_callback(&script, move |raw| {
                if let Some(said) = read_report(&raw, &of_first) {
                    if let Ok(mut kept) = keeping.lock() {
                        *kept = Some(said);
                    }
                }
            });
            if let Err(err) = dispatched {
                // The script never reached the page. Perch failing to ask is
                // not the page failing to answer, and it is the one thing
                // knowable from this side of the window.
                tell(
                    &handle,
                    &of_window,
                    FillReportDto::stopped(
                        &of_role,
                        format!(
                            "The fill could not be started in the window Perch opened. {}",
                            plainly(err)
                        ),
                    ),
                );
                return;
            }

            // The fill puts its values back for a few seconds, so the account
            // worth reading is the one the page gives once that has stopped.
            // This waits the whole window out, and a moment past it, on a
            // thread of its own: the one it is called on draws the interface.
            let asking = webview.clone();
            let kept = first_pass.clone();
            let told = handle.clone();
            let of_last = of_role.clone();
            let window = of_window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(7_500));
                let first = kept.lock().ok().and_then(|k| k.clone());
                let spare = first.clone();
                let elsewhere = told.clone();
                let of_spare = window.clone();
                // One account per fill, whichever arm reaches it first.
                let once = Arc::new(AtomicBool::new(false));
                let answered = once.clone();
                let _ = asking.eval_with_callback(FILL_REPORT_READER, move |raw| {
                    if answered.swap(true, Ordering::SeqCst) {
                        return;
                    }
                    // Which account goes out. A page answering with Perch's own
                    // mark on it is answering about the fill Perch ran, and
                    // that reading is the one worth showing. An answer without
                    // the mark was written by the page, which is free to write
                    // to `window`; the first pass came back through Perch's own
                    // callback, so that is the one Perch has grounds for. No
                    // account at all, where one was left, means this is not the
                    // document that was filled any more and neither reading
                    // describes what is in the window now.
                    let said = match (read_report(&raw, &of_last), first.clone()) {
                        (Some(fresh), Some(mine)) if fresh.stamp == mine.stamp => Some(fresh),
                        (Some(_), Some(mine)) => Some(mine),
                        (None, Some(_)) => {
                            Some(FillReportDto::stopped(&of_last, NOT_THAT_PAGE.to_string()))
                        }
                        // Nothing came back through the callback either, so
                        // Perch has no account of this fill and says none.
                        (_, None) => None,
                    };
                    if let Some(said) = said {
                        tell(&told, &window, said);
                    }
                });
                // A window the person has closed answers nothing, and asking it
                // still succeeds: the message is delivered to where the webview
                // used to be and dropped there. So the wait for an answer ends,
                // and what the first pass saw is still true of what was typed.
                std::thread::sleep(std::time::Duration::from_millis(2_000));
                if !once.swap(true, Ordering::SeqCst) {
                    if let Some(said) = spare {
                        tell(&elsewhere, &of_spare, said);
                    }
                }
            });
        })
        .build()
        .map_err(plainly)?;

    Ok(FillStartedDto {
        started: Started::Filling,
        caveat: skipped.map(|path| {
            format!("{path} could not be read, so the file box is left for you to fill yourself.")
        }),
    })
}

/// Roles Perch can fill open in the app; ones it cannot open in the browser.
/// Either way the person is the one who submits, so both end here.
#[tauri::command]
fn open_in_browser(app: tauri::AppHandle, url: String) -> Answer<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(url, None::<&str>).map_err(plainly)
}

/// Adapters declare fill support separately from monitoring, so the interface
/// can say "opens in browser" truthfully rather than guessing from the ATS.
#[tauri::command]
fn fill_supported(ats: String) -> bool {
    ["greenhouse", "lever", "ashby"]
        .iter()
        .find(|a| **a == ats.to_ascii_lowercase())
        .and_then(|a| perch_core::Ats::parse(a).ok())
        .and_then(adapter_for)
        .map(|a| a.fill_supported())
        .unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // The file picker, which is how a résumé gets in. It hands back a path
        // a person chose, and the reading is decided on this side of the wall.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = Paths::discover()?;
            paths.ensure()?;
            let store = Store::open(&paths.db())?;
            app.manage(App {
                store: Mutex::new(store),
                paths,
                http: Http::new()?,
                import: Mutex::new(Vec::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            feed,
            detail,
            dismiss,
            restore,
            sync,
            watchlist,
            last_read,
            watch_add,
            watch_remove,
            applications,
            mark_application,
            settings,
            model_key_set,
            model_key_forget,
            import_ready,
            import_read,
            import_write,
            open_in_browser,
            fill_supported,
            fill_plan,
            open_and_fill,
        ])
        .run(tauri::generate_context!())
        .expect("Perch could not start");
}

#[cfg(test)]
mod tests {
    use super::{
        decide, document_for, key_for, key_forget, key_set, model_dto, proposal_dto, provenance,
        read_report, stance, DecisionDto, FillPlanDto, FillReportDto, ImportReadyDto, ProposalDto,
        SettingsDto, FILL_REPORT_READER,
    };
    use perch_core::{profile, Ats, Profile};
    use perch_fill::flavor::is_the_reviewed_page as reviewed;
    use perch_llm::{document::Kind, extract, Consent, Document, Model, Proposal};
    use serde_json::json;
    use std::path::Path;

    /// What a fill leaves on the page, as one came back from a browser.
    const SEEN: &str = r#"{"filled":6,"planned":7,"missing":["LinkedIn"],"file":true,"fileNamed":true,"error":null}"#;

    #[test]
    fn the_reader_asks_the_page_for_the_account_and_for_nothing_else() {
        // Run against a filled form in a browser, where it gave back the
        // account as a string and the word null when there was none.
        assert_eq!(
            FILL_REPORT_READER,
            "(function () { try { return JSON.stringify(window.__perchFill || null); } \
             catch (err) { return 'null'; } })();"
        );
        // And it is a question, not a door. Nothing here reaches a command.
        for reach in ["__TAURI__", "invoke", "ipc", "postMessage"] {
            assert!(!FILL_REPORT_READER.contains(reach));
        }
    }

    #[test]
    fn a_report_survives_the_evaluator_quoting_it() {
        // The evaluated value arrives serialised, so a script that returned a
        // string arrives wrapped in one more layer of JSON than it left with.
        let quoted = serde_json::to_string(SEEN).unwrap();
        let said = read_report(&quoted, "acme-1").expect("the quoted form is unreadable");
        assert_eq!(said.filled, 6);
        assert_eq!(said.planned, 7);
        assert_eq!(said.missing, vec!["LinkedIn".to_string()]);
        assert!(said.file && said.file_named);
        assert_eq!(said.error, None);
        // And unquoted, for a platform that hands the value over as it is.
        assert_eq!(read_report(SEEN, "acme-1").unwrap().filled, 6);
    }

    #[test]
    fn a_report_is_about_the_role_perch_asked_after() {
        // The page is free to write to `window`, so it can name any role it
        // likes. The name it is filed under is the one Perch opened.
        let claimed = r#"{"reference":"someone-else","filled":1,"planned":1,"missing":[],
            "file":false,"fileNamed":false,"error":null}"#;
        assert_eq!(read_report(claimed, "acme-1").unwrap().reference, "acme-1");
    }

    #[test]
    fn a_page_that_left_no_account_is_not_read_as_one() {
        // The sheet says it heard nothing. Nothing is not a fill that worked.
        for nothing in ["null", "\"null\"", "", "undefined", "{\"filled\":-1}"] {
            assert!(
                read_report(nothing, "acme-1").is_none(),
                "{nothing:?} was read as a report"
            );
        }
    }

    #[test]
    fn nothing_the_page_writes_reaches_the_interface_unbounded() {
        // A report is information, and the page writes it. A page answering
        // with a megabyte is answering, not deciding anything.
        let said = FillReportDto {
            reference: "acme-1".into(),
            stamp: String::new(),
            window_open: true,
            filled: 0,
            planned: 400,
            missing: (0..400).map(|_| "x".repeat(4000)).collect(),
            changed: (0..400).map(|_| "z".repeat(4000)).collect(),
            file: false,
            file_given: false,
            file_named: false,
            settled: true,
            error: Some("y".repeat(9000)),
        }
        .trimmed();
        assert_eq!(said.changed.len(), 24);
        assert!(said.changed.iter().all(|label| label.len() <= 120));
        assert_eq!(said.missing.len(), 24);
        assert!(said.missing.iter().all(|label| label.len() <= 120));
        assert_eq!(said.error.unwrap().len(), 200);
    }

    #[test]
    fn a_board_redirect_still_counts_as_the_form() {
        // Every boards.greenhouse.io posting 301s to job-boards.greenhouse.io.
        // Comparing hosts literally would skip the fill on all of them.
        let intended = "https://boards.greenhouse.io/figma/jobs/6145639004?gh_jid=6145639004";
        assert!(reviewed(
            Ats::Greenhouse,
            "https://job-boards.greenhouse.io/figma/jobs/6145639004",
            intended
        ));
        assert!(reviewed(
            Ats::Greenhouse,
            "https://boards.greenhouse.io/figma/jobs/6145639004",
            intended
        ));
    }

    #[test]
    fn anywhere_else_gets_nothing() {
        let intended = "https://job-boards.greenhouse.io/gitlab/jobs/8749950002";
        for elsewhere in [
            // A closed posting redirecting to the company's index.
            "https://job-boards.greenhouse.io/gitlab",
            // A different posting on the same board.
            "https://job-boards.greenhouse.io/gitlab/jobs/1111111111",
            // A link clicked inside the filled window.
            "https://about.gitlab.com/jobs/",
            // A host that only looks right.
            "https://job-boards.greenhouse.io.evil.example/gitlab/jobs/8749950002",
            "https://job-boards.greenhouse.io@evil.example/gitlab/jobs/8749950002",
            "about:blank",
            "",
        ] {
            assert!(
                !reviewed(Ats::Greenhouse, elsewhere, intended),
                "{elsewhere} passed as the reviewed form"
            );
        }
        // And one ATS never fills another's form, however the path lines up.
        assert!(!reviewed(
            Ats::Lever,
            "https://job-boards.greenhouse.io/gitlab/jobs/8749950002",
            intended
        ));
    }

    // ---- reading a résumé into proposed fields ----------------------------

    const RESUME: &str = "\
DANA FERREIRA
dana@dferreira.dev · +1 (415) 555-0148 · github.com/dferreira
SF Bay Area · open to remote

EXPERIENCE

Cloudflare — Senior Software Engineer, Storage
March 2023 – February 2026

SKILLS
Rust, Go, Tokio, gRPC, PostgreSQL";

    fn resume() -> Document {
        Document {
            text: RESUME.to_string(),
            kind: Kind::Text,
            name: "resume-systems.pdf".into(),
        }
    }

    fn model(endpoint: &str) -> Model {
        Model {
            endpoint: endpoint.into(),
            model: "llama3.1:8b".into(),
            consent: Consent::default(),
        }
    }

    /// One reply, verified and shaped the way a read hands it to the screen.
    fn read(reply: serde_json::Value, profile: &Profile) -> (Vec<Proposal>, Vec<ProposalDto>) {
        let document = resume();
        let proposals = extract::proposals_from(&reply, &document, profile);
        let shown = proposals
            .iter()
            .enumerate()
            .map(|(id, proposal)| proposal_dto(id, proposal, &document.name))
            .collect();
        (proposals, shown)
    }

    fn row<'a>(shown: &'a [ProposalDto], label: &str) -> &'a ProposalDto {
        shown
            .iter()
            .find(|proposal| proposal.label == label)
            .unwrap_or_else(|| panic!("no row for {label}"))
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("perch-import-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_resume_is_not_read_off_the_disk_until_consent_is_settled() {
        let mut remote = model("https://api.openrouter.ai/v1");
        let dir = scratch("gate");
        let path = dir.join("cv.txt");
        std::fs::write(&path, RESUME).unwrap();

        let refusal = document_for(&remote, &path).unwrap_err();
        assert!(refusal.contains("api.openrouter.ai"), "{refusal}");
        assert!(refusal.contains("will not send"), "{refusal}");

        // The same answer for a file that is not there, which is how we know
        // the gate was settled before the path was touched at all.
        let missing = document_for(&remote, Path::new("/nowhere/at/all/cv.txt")).unwrap_err();
        assert_eq!(missing, refusal);

        remote.consent.resume_import_may_leave_this_mac = true;
        assert_eq!(document_for(&remote, &path).unwrap().name, "cv.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_model_configured_there_is_nothing_to_read_a_resume_with() {
        let none = Model::default();
        let (verdict, host, remedy) = stance(&none);
        assert_eq!(verdict, "none");
        assert_eq!(host, None);
        assert!(remedy.unwrap().contains("perch model list"));
        assert!(document_for(&none, Path::new("/nowhere/at/all/cv.txt")).is_err());
    }

    #[test]
    fn the_screen_is_told_the_host_when_a_resume_would_leave_this_mac() {
        let local = model("http://localhost:11434/v1");
        assert_eq!(stance(&local), ("local".into(), None, None));
        assert!(provenance(&local).contains("Nothing left this Mac"));

        let refused = model("https://api.openrouter.ai/v1");
        let (verdict, host, remedy) = stance(&refused);
        assert_eq!(verdict, "refused");
        assert_eq!(host.as_deref(), Some("api.openrouter.ai"));
        assert!(remedy.is_some());

        let mut allowed = refused.clone();
        allowed.consent.resume_import_may_leave_this_mac = true;
        let (verdict, host, remedy) = stance(&allowed);
        assert_eq!(verdict, "remote");
        assert_eq!(host.as_deref(), Some("api.openrouter.ai"));
        assert_eq!(remedy, None);
        assert!(provenance(&allowed).contains("api.openrouter.ai"));
        assert!(provenance(&allowed).contains("Your résumé left this Mac"));
    }

    #[test]
    fn a_value_the_document_does_not_state_arrives_with_no_way_to_accept_it() {
        let reply = json!({
            "name": "DANA FERREIRA", "email": "dana@gmail.com", "phone": "",
            "location": "", "github": "", "website": "", "skills": "",
            "experience": []
        });
        let (mut proposals, shown) = read(reply, &Profile::default());

        // It is still sent to the screen, which shows it and counts it.
        assert_eq!(shown.len(), 2);
        let email = row(&shown, "Email");
        assert!(!email.offerable);
        assert!(!email.accepted);
        assert!(email.quote.is_none());
        let refusal = email.refusal.as_deref().unwrap();
        assert!(refusal.contains("resume-systems.pdf"), "{refusal}");
        assert!(refusal.contains("not offering it"), "{refusal}");

        // A caller marking it accepted, with words of its own, changes nothing
        // about where it may go.
        decide(
            &mut proposals,
            &[DecisionDto {
                id: email.id,
                accepted: true,
                value: Some("dana@gmail.com".into()),
            }],
        );
        let mut profile = Profile::default();
        assert_eq!(extract::apply(&proposals, &mut profile), 0);
        assert_eq!(profile.email, "");
    }

    #[test]
    fn only_what_a_person_accepted_is_written_and_nothing_else_is_touched() {
        let mut profile = Profile {
            email: "dana@hey.com".into(),
            work_authorisation: "US citizen".into(),
            links: profile::Links {
                linkedin: "in/dferreira".into(),
                ..Default::default()
            },
            documents: vec![profile::Document {
                name: "résumé".into(),
                path: "/Users/dana/cv.pdf".into(),
                kind: "résumé".into(),
            }],
            ..Profile::default()
        };
        let reply = json!({
            "name": "DANA FERREIRA", "email": "dana@dferreira.dev",
            "phone": "+1 (415) 555-0148", "github": "github.com/dferreira",
            "location": "", "website": "", "skills": "", "experience": []
        });
        let (mut proposals, shown) = read(reply, &profile);
        assert_eq!(
            row(&shown, "Email").current.as_deref(),
            Some("dana@hey.com")
        );
        assert!(
            shown.iter().all(|row| row.accepted),
            "all four are quoted, so all four arrive accepted"
        );

        decide(
            &mut proposals,
            &[
                DecisionDto {
                    id: row(&shown, "Name").id,
                    accepted: true,
                    value: None,
                },
                DecisionDto {
                    id: row(&shown, "Email").id,
                    accepted: false,
                    value: None,
                },
                DecisionDto {
                    id: row(&shown, "Phone").id,
                    accepted: true,
                    value: Some("+1 415 555 0148".into()),
                },
            ],
        );

        assert_eq!(extract::apply(&proposals, &mut profile), 2);
        assert_eq!(profile.name, "DANA FERREIRA");
        assert_eq!(
            profile.phone, "+1 415 555 0148",
            "an edited value is written as the person typed it"
        );
        assert_eq!(
            profile.email, "dana@hey.com",
            "a skipped row writes nothing"
        );
        assert_eq!(
            profile.links.github, "",
            "a row nobody decided about writes nothing either"
        );
        assert_eq!(profile.work_authorisation, "US citizen");
        assert_eq!(profile.links.linkedin, "in/dferreira");
        assert_eq!(profile.documents.len(), 1);
        assert!(profile.skills.is_empty());
        assert!(profile.experience.is_empty());
    }

    #[test]
    fn the_keychain_is_asked_about_a_remote_endpoint_and_never_a_local_one() {
        let asked = std::cell::RefCell::new(Vec::new());
        let keychain = |host: &str| {
            asked.borrow_mut().push(host.to_string());
            Some("a key".to_string())
        };

        assert_eq!(key_for(&model("http://localhost:11434/v1"), keychain), None);
        assert!(
            asked.borrow().is_empty(),
            "a model on this Mac needs no key, and asking puts a password prompt in the way"
        );

        let mut remote = model("https://api.openrouter.ai/v1");
        remote.consent.resume_import_may_leave_this_mac = true;
        assert_eq!(key_for(&remote, keychain).as_deref(), Some("a key"));
        assert_eq!(asked.borrow().as_slice(), ["api.openrouter.ai"]);
    }

    #[test]
    fn the_highlight_is_counted_the_way_the_webview_counts_it() {
        let reply = json!({
            "name": "", "email": "", "phone": "", "location": "",
            "github": "github.com/dferreira", "website": "", "skills": "",
            "experience": []
        });
        let (_, shown) = read(reply, &Profile::default());
        let github = row(&shown, "GitHub");
        let quote = github.quote.as_deref().unwrap();
        let (from, to) = github.highlight.unwrap();

        let units: Vec<u16> = quote.encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&units[from..to]).unwrap(),
            "github.com/dferreira"
        );
        // Two middle dots sit before the match on that line, so the same span
        // in bytes would highlight two characters late.
        assert_ne!(from, quote.find("github.com/dferreira").unwrap());
        assert_eq!(github.line, Some(2));
    }

    #[test]
    fn a_row_that_writes_three_values_says_which_three() {
        // Editing an employer row edits its company, title and dates at once.
        // The screen can only say so if the row tells it what it writes.
        let reply = json!({
            "name": "DANA FERREIRA", "email": "", "phone": "", "location": "",
            "github": "", "website": "", "skills": "",
            "experience": [
                { "company": "Cloudflare", "title": "Senior Software Engineer, Storage", "dates": "March 2023 – February 2026" }
            ]
        });
        let (_, shown) = read(reply, &Profile::default());
        assert_eq!(
            row(&shown, "Employer: most recent").parts.as_deref(),
            Some("company · title · dates")
        );
        assert_eq!(row(&shown, "Name").parts, None);
    }

    #[test]
    fn a_row_nobody_touched_is_not_reported_as_skipped() {
        // "Skipped" is the x action, and the hint bar offers u to undo it. The
        // sentence after the write counts the marks the rows carry: reading
        // the count off the accepted total instead folded the undecided rows
        // in with them and named an act the person never performed.
        let view = include_str!("../../src/views/Import.tsx");
        assert!(
            view.contains("function heldBack(rows: Row[]): string | null {"),
            "the summary is handed a count rather than the rows"
        );
        let body = view
            .split("function heldBack(")
            .nth(1)
            .expect("the screen still says what it did not write")
            .split("\n}")
            .next()
            .expect("a function body");
        assert!(body.contains(r#"row.mark === "skipped""#), "{body}");
    }

    #[test]
    fn the_app_declares_every_field_the_profile_keeps() {
        // Reading a résumé can write skills and positions. A field the
        // interface does not declare is one the app cannot show back to the
        // person it has just told it wrote.
        let api = include_str!("../../src/lib/api.ts");
        let declared = api
            .split("export interface Profile {")
            .nth(1)
            .expect("the app still describes the profile")
            .split("\n}")
            .next()
            .expect("an interface body");
        let profile = serde_json::to_value(Profile::default()).unwrap();
        for field in profile.as_object().expect("an object").keys() {
            assert!(declared.contains(field), "the app does not declare {field}");
        }

        let view = include_str!("../../src/views/Profile.tsx");
        for field in ["skills", "experience"] {
            assert!(
                view.contains(field),
                "the profile screen never reads {field}"
            );
        }
    }

    // ---- the key for a remote endpoint ------------------------------------

    /// Stands in for a real key, and is odd enough that finding these
    /// characters anywhere is finding this one.
    const KEY: &str = "sk-perch-Zx9Qv7Lm2Kd4Rt6Wp8Nb";

    /// A host this test run made up, so nothing here reads or writes the item a
    /// person actually keeps. The run that stores one deletes it again.
    fn nobodys_host(what: &str) -> String {
        format!("{what}-{}.perch.invalid", std::process::id())
    }

    #[test]
    fn a_key_that_was_stored_is_in_nothing_the_interface_receives() {
        let host = nobodys_host("stored");
        let mut remote = model(&format!("https://{host}/v1"));
        remote.consent.resume_import_may_leave_this_mac = true;
        let stored = key_set(&remote, KEY, perch_llm::client::secret::set);

        // Everything the app hands the webview about a model, an import and a
        // fill, shaped by the same functions the commands shape it with.
        let profile = Profile {
            name: "Dana Ferreira".into(),
            email: "dana@dferreira.dev".into(),
            ..Profile::default()
        };
        let (verdict, verdict_host, remedy) = stance(&remote);
        let plan = perch_fill::plan::build(
            Ats::Greenhouse,
            "https://job-boards.greenhouse.io/acme/jobs/1",
            &profile,
            None,
        )
        .expect("Greenhouse forms are ones Perch can fill");
        let payloads = [
            (
                "settings",
                serde_json::to_string(&SettingsDto {
                    model: model_dto(&remote, Some(vec!["llama3.1:8b".into()])),
                    model_path: "/Users/dana/.perch/model.toml".into(),
                    preferred_resume: None,
                    profile: profile.clone(),
                    profile_path: "/Users/dana/.perch/profile.toml".into(),
                    rules_path: "/Users/dana/.perch/rules.toml".into(),
                    database_path: "/Users/dana/.perch/perch.db".into(),
                    rules: Vec::new(),
                    rules_error: None,
                })
                .unwrap(),
            ),
            (
                "import_ready",
                serde_json::to_string(&ImportReadyDto {
                    configured: remote.configured(),
                    verdict,
                    host: verdict_host,
                    sentence: remote.consequence(),
                    remedy,
                    profile_path: "/Users/dana/.perch/profile.toml".into(),
                })
                .unwrap(),
            ),
            (
                "fill_plan",
                serde_json::to_string(&FillPlanDto {
                    what_happens_next: plan.what_happens_next("Acme"),
                    attachment_caveat: plan.attachment_caveat(),
                    plan: serde_json::to_value(&plan).unwrap(),
                    company: "Acme".into(),
                    title: "Systems Engineer".into(),
                    fillable: true,
                })
                .unwrap(),
            ),
        ];

        // Out of the keychain before anything below can fail and leave it
        // there, and asked after once to be sure it went. Reading is the one
        // thing the app itself never does, and a test that puts something in a
        // person's keychain owes them proof it took it back out.
        let cleared = perch_llm::client::secret::forget(&host);
        let left_behind = perch_llm::client::secret::get(&host);

        assert_eq!(
            stored,
            Ok(format!("Stored in this Mac's keychain, for {host}."))
        );
        assert_eq!(cleared, Ok(()));
        assert_eq!(left_behind, None, "the test left a key in the keychain");
        for (which, payload) in payloads {
            assert!(!payload.contains(KEY), "the key is in the {which} payload");
        }
    }

    #[test]
    fn storing_a_key_writes_nothing_to_model_toml() {
        // The file is one a person can open, copy and paste. A credential in it
        // travels with it.
        let dir = scratch("model-key");
        let path = dir.join("model.toml");
        let mut remote = model("https://api.openrouter.ai/v1");
        remote.consent.resume_import_may_leave_this_mac = true;
        remote.save(&path).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        let kept = std::cell::RefCell::new(Vec::new());
        let keychain = |host: &str, key: &str| {
            kept.borrow_mut().push((host.to_string(), key.to_string()));
            Ok(())
        };
        let said = key_set(&Model::load(&path).unwrap(), KEY, keychain);

        let after = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert!(said.is_ok(), "{said:?}");
        assert_eq!(after, before);
        assert!(!after.contains(KEY));
        assert_eq!(
            kept.into_inner().as_slice(),
            [("api.openrouter.ai".to_string(), KEY.to_string())]
        );
    }

    #[test]
    fn an_endpoint_on_this_mac_is_refused_a_key_and_told_why() {
        let asked = std::cell::RefCell::new(0);
        let keychain = |_: &str, _: &str| {
            *asked.borrow_mut() += 1;
            Ok(())
        };
        for endpoint in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:11434/v1",
            // Spellings of this Mac that a person actually types. Storing a key
            // for one of these hands it to a process on this machine.
            "http://0.0.0.0:11434/v1",
            "http://localhost.:11434/v1",
            "http://[::ffff:127.0.0.1]:11434/v1",
            "http://ollama.localhost:11434/v1",
        ] {
            let here = model(endpoint);
            let host = here.host().expect("an endpoint with a host");
            assert_eq!(
                key_set(&here, KEY, keychain).unwrap_err(),
                format!("{host} is on this Mac and wants no key.")
            );
        }
        assert_eq!(*asked.borrow(), 0, "a key nobody would send was stored");
    }

    #[test]
    fn with_no_endpoint_configured_there_is_nothing_to_hold_a_key_for() {
        let writing = |_: &str, _: &str| -> Result<(), String> {
            panic!("the keychain was written to with no endpoint configured")
        };
        let deleting = |_: &str| -> Result<(), String> {
            panic!("the keychain was reached with no endpoint configured")
        };
        for endpoint in ["", "   ", "not a url", "http://", "ftp://example.com"] {
            let none = model(endpoint);
            let nothing = "No endpoint is configured, so there is nothing to hold a key for.";
            assert_eq!(key_set(&none, KEY, writing).unwrap_err(), nothing);
            assert_eq!(key_forget(&none, deleting).unwrap_err(), nothing);
        }
    }

    #[test]
    fn forgetting_a_key_nobody_stored_is_not_an_error() {
        // Perch cannot say whether it holds one without reading it, so the
        // control is offered either way and pressing it settles the question.
        let host = nobodys_host("never-stored");
        let said = key_forget(
            &model(&format!("https://{host}/v1")),
            perch_llm::client::secret::forget,
        );
        assert_eq!(said, Ok(format!("Perch is holding no key for {host}.")));
    }

    #[test]
    fn the_screen_sends_the_key_one_way_and_keeps_none_of_it() {
        let view = include_str!("../../src/views/Profile.tsx");
        let control = view
            .split("function ModelKey(")
            .nth(1)
            .expect("the profile screen still offers somewhere to put a key")
            .split("\n}")
            .next()
            .expect("a function body");
        assert!(control.contains(r#"type="password""#), "{control}");
        assert!(control.contains(r#"autoComplete="off""#), "{control}");
        assert!(
            control.contains(r#"setKey("")"#),
            "the field is not emptied when the command resolves"
        );
        assert!(!view.contains("localStorage"), "the key is put in storage");

        // And the app reaches both commands by the names Rust registers.
        let api = include_str!("../../src/lib/api.ts");
        assert!(api.contains(r#""model_key_set""#));
        assert!(api.contains(r#""model_key_forget""#));
    }

    #[test]
    fn the_main_window_gained_a_file_picker_and_nothing_else() {
        // The picker's plugin brings a filesystem plugin along as a dependency.
        // What decides whether any of it is reachable is this list, and the one
        // window it names: the window a form opens in is not that window.
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/main.json")).unwrap();
        assert_eq!(capability["windows"], json!(["main"]));
        assert_eq!(
            capability["permissions"],
            json!([
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "dialog:allow-open"
            ])
        );
    }
}

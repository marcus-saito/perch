//! The `perch` command line. Its vocabulary is the interface's vocabulary:
//! the desktop command palette offers these same words, verbatim.

mod render;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use perch_core::{
    ats, detect_board, ensure_description, html, rules::TEMPLATE, sync::sync_all, ApplicationState,
    FeedFilter, Http, Paths, Rules, Store,
};
use perch_llm::client::{secret as llm_secret, Client as LlmClient, OLLAMA};
use perch_llm::config::{Model as LlmModel, Permission as LlmPermission};
use perch_llm::{document as llm_document, extract as llm_extract};
use render::Style;
use time::OffsetDateTime;

#[derive(Parser)]
#[command(
    name = "perch",
    version,
    about = "A local-first job search companion",
    long_about = "Watches company job boards and helps fill applications.\n\
                  Everything stays on this machine. Perch never submits anything."
)]
struct Cli {
    /// Keep everything under this directory instead of the usual place.
    #[arg(long, global = true, value_name = "DIR")]
    home: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Companies whose boards Perch is watching
    #[command(subcommand)]
    Watch(WatchCommand),

    /// Poll every watched board now
    Sync,

    /// Matched roles, newest first
    Feed {
        /// Only what went up in the last day
        #[arg(long)]
        fresh: bool,
        /// One company only
        #[arg(long, value_name = "NAME")]
        company: Option<String>,
        /// Every open role, whether or not a rule fires on it
        #[arg(long)]
        all: bool,
    },

    /// The rules that decide what reaches the feed
    #[command(subcommand)]
    Rules(RulesCommand),

    /// One role in full: what it says, what the board has done with it
    Open {
        /// The short reference the feed printed
        reference: String,
    },

    /// Review what Perch would fill, attach a document, open it in your browser
    Apply {
        /// The short reference the feed printed
        reference: String,
        /// The document to attach, instead of the résumé in your profile
        #[arg(long, value_name = "PATH")]
        resume: Option<String>,
    },

    /// Where the model lives, and whether your résumé may go there
    #[command(subcommand)]
    Model(ModelCommand),

    /// Your profile, and reading a résumé into it
    #[command(subcommand)]
    Profile(ProfileCommand),

    /// Applications you have sent, and what came back
    Apps {
        #[command(subcommand)]
        command: Option<AppsCommand>,
    },

    /// Take a role out of the feed. Nothing is deleted; `restore` puts it back
    Dismiss {
        /// The short reference the feed printed
        reference: String,
    },

    /// Put a dismissed role back in the feed
    Restore { reference: String },
}

#[derive(Subcommand)]
enum ModelCommand {
    /// Models Perch can reach, here and at the configured endpoint
    List,
    /// Choose the model that reads résumés
    Set {
        /// A model name from `perch model list`
        name: String,
        /// An OpenAI-compatible base URL. Defaults to Ollama on this Mac.
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Point at a remote endpoint, and say whether your résumé may go there
    Endpoint {
        /// An OpenAI-compatible base URL
        url: String,
        /// Allow résumé import to send your résumé to it
        #[arg(long)]
        allow_resume: bool,
    },
    /// Give Perch the key a remote endpoint wants, or take it back
    Key {
        /// Forget the key held for the configured endpoint
        #[arg(long)]
        forget: bool,
        /// Forget the key held for this host rather than the configured one
        #[arg(long, value_name = "HOST", requires = "forget")]
        host: Option<String>,
    },
    /// Run with no model at all. Everything except résumé import still works.
    Off,
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// Show the profile as Perch reads it
    Show,
    /// Propose profile fields from a résumé, for you to accept one at a time
    Import {
        /// A .pdf, .txt or .md file
        file: String,
        /// Accept every proposal the document quotes exactly, without asking
        #[arg(long)]
        accept_quoted: bool,
    },
}

#[derive(Subcommand)]
enum AppsCommand {
    /// In flight, responded, archived
    List,
    /// Record where an application stands
    Mark {
        /// The short reference the feed printed
        reference: String,
        /// in-flight, responded, or archived
        state: String,
        /// What happened, in your own words
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Subcommand)]
enum RulesCommand {
    /// Open the rules file, creating a commented starting point if there is none
    Edit,
    /// The rules as Perch reads them
    List,
    /// Which rule fires on one role, and why
    Test {
        /// The short reference the feed printed
        reference: String,
    },
}

#[derive(Subcommand)]
enum WatchCommand {
    /// Find a company's board and start watching it
    Add {
        /// A company name, a board URL, or a careers page
        company: String,
    },
    /// Companies and their boards
    List,
    /// Stop watching a company
    Rm { company: String },
}

/// `perch feed | head` must end quietly. Rust masks SIGPIPE at startup, which
/// turns a closed pipe into a panic three screens into the output; put the
/// default disposition back so the process stops.
#[cfg(unix)]
fn end_quietly_on_closed_pipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn end_quietly_on_closed_pipe() {}

/// Terminal echo turned off for as long as this is held, so a key typed at the
/// prompt is not left on the screen and in the scrollback.
///
/// The previous terminal settings go back on drop, so they are restored even
/// when the read between fails. A stdin that is not a terminal has no echo to
/// turn off and gets `None`: reading a key from a pipe is still supported.
#[cfg(unix)]
struct EchoOff {
    fd: i32,
    restore: libc::termios,
}

#[cfg(unix)]
impl EchoOff {
    fn new() -> Option<Self> {
        let fd = libc::STDIN_FILENO;
        // SAFETY: `termios` is plain data, and `tcgetattr` either fills the
        // whole of it or reports that it did not.
        unsafe {
            let mut current = std::mem::MaybeUninit::<libc::termios>::uninit();
            if libc::tcgetattr(fd, current.as_mut_ptr()) != 0 {
                return None;
            }
            let restore = current.assume_init();
            let mut quiet = restore;
            quiet.c_lflag &= !libc::ECHO;
            if libc::tcsetattr(fd, libc::TCSAFLUSH, &quiet) != 0 {
                return None;
            }
            Some(Self { fd, restore })
        }
    }
}

#[cfg(unix)]
impl Drop for EchoOff {
    fn drop(&mut self) {
        // SAFETY: `restore` is what `tcgetattr` gave back for this same fd.
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.restore);
        }
    }
}

#[cfg(not(unix))]
struct EchoOff;

#[cfg(not(unix))]
impl EchoOff {
    fn new() -> Option<Self> {
        None
    }
}

fn main() {
    end_quietly_on_closed_pipe();
    if let Err(err) = run() {
        // Plain, lowercase, no stack of context.
        eprintln!("{err}");
        for cause in err.chain().skip(1) {
            eprintln!("  {cause}");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = match &cli.home {
        Some(dir) => Paths::at(dir),
        None => Paths::discover().context("could not work out where to keep Perch's files")?,
    };
    paths.ensure()?;

    let mut store = Store::open(&paths.db())?;
    let style = Style::detect();
    let now = OffsetDateTime::now_utc();

    match cli.command {
        Command::Watch(WatchCommand::Add { company }) => watch_add(&mut store, &company, now),
        Command::Watch(WatchCommand::List) => watch_list(&store, &style, now),
        Command::Watch(WatchCommand::Rm { company }) => watch_rm(&store, &company, now),
        Command::Sync => sync(&mut store, &style, now),
        Command::Feed {
            fresh,
            company,
            all,
        } => feed(&store, &paths, &style, fresh, company, all, now),
        Command::Rules(RulesCommand::Edit) => rules_edit(&paths),
        Command::Rules(RulesCommand::List) => rules_list(&paths, &style),
        Command::Rules(RulesCommand::Test { reference }) => {
            rules_test(&store, &paths, &style, &reference)
        }
        Command::Open { reference } => open_role(&store, &style, &reference, now),
        Command::Apply { reference, resume } => {
            apply(&store, &paths, &style, &reference, resume.as_deref())
        }
        Command::Model(ModelCommand::List) => model_list(&paths, &style),
        Command::Model(ModelCommand::Set { name, endpoint }) => {
            model_set(&paths, &style, &name, endpoint.as_deref())
        }
        Command::Model(ModelCommand::Endpoint { url, allow_resume }) => {
            model_endpoint(&paths, &style, &url, allow_resume)
        }
        Command::Model(ModelCommand::Key { forget, host }) => {
            model_key(&paths, &style, forget, host.as_deref())
        }
        Command::Model(ModelCommand::Off) => model_off(&paths, &style),
        Command::Profile(ProfileCommand::Show) => profile_show(&paths, &style),
        Command::Profile(ProfileCommand::Import {
            file,
            accept_quoted,
        }) => profile_import(&paths, &style, &file, accept_quoted),
        Command::Apps { command } => match command {
            None | Some(AppsCommand::List) => apps_list(&store, &style, now),
            Some(AppsCommand::Mark {
                reference,
                state,
                note,
            }) => apps_mark(&store, &reference, &state, note.as_deref(), now),
        },
        Command::Dismiss { reference } => dismiss(&store, &reference, now),
        Command::Restore { reference } => restore(&store, &reference),
    }
}

fn watch_add(store: &mut Store, input: &str, now: OffsetDateTime) -> Result<()> {
    let http = Http::new()?;
    println!("Looking for a board for {input}…");

    let Some(found) = detect_board(input, &http)? else {
        println!();
        println!("No public board found for {input}.");
        println!("Perch can watch {} boards so far.", ats::watchable());
        println!("If you have the board URL, pass that instead.");
        return Ok(());
    };

    let board = store.watch(&found, now)?;
    println!();
    println!(
        "Found {} board for {}.",
        found.ats.with_article(),
        board.company_name
    );
    println!("  {}", board.url);
    println!(
        "  {}",
        if board.fill_supported {
            "Forms filled here, from your profile."
        } else {
            "Perch can read this board but not fill it; roles open in the browser."
        }
    );
    println!();
    println!("Run `perch sync` to read it for the first time. Anything already open");
    println!("there arrives in the feed at its real age, not as new.");
    Ok(())
}

fn watch_list(store: &Store, style: &Style, now: OffsetDateTime) -> Result<()> {
    let boards = store.boards()?;
    if boards.is_empty() {
        println!();
        println!("  Nothing on the watchlist yet.");
        println!();
        println!(
            "  {}",
            style.dim("perch watch add <company>  finds a board and starts watching it")
        );
        println!();
        return Ok(());
    }

    println!();
    for board in &boards {
        let (seen, open) = store.board_tally(board.id)?;
        let checked = match board.last_checked_at {
            Some(at) => perch_core::verb_signal("checked", at, now),
            None => "not read yet".to_string(),
        };

        println!("  {}", board.company_name);
        println!(
            "  {}  {}  {}",
            style.dim(board.ats.label()),
            style.dim(&board.url),
            style.dim(&checked)
        );

        let history = if seen == 0 {
            "Nothing seen here yet.".to_string()
        } else {
            format!(
                "{seen} {} seen since Perch started watching, {open} still open.",
                if seen == 1 { "role" } else { "roles" }
            )
        };
        let fill = if board.fill_supported {
            "Forms filled here."
        } else {
            "Opens in browser."
        };
        println!("  {}", style.dim(&format!("{history} {fill}")));
        println!();
    }
    Ok(())
}

fn watch_rm(store: &Store, company: &str, now: OffsetDateTime) -> Result<()> {
    if store.unwatch(company, now)? {
        println!("Stopped watching {company}.");
        println!("Nothing is deleted. Adding it again restores what was here.");
    } else {
        println!("{company} is not on the watchlist. `perch watch list` shows what is.");
    }
    Ok(())
}

fn sync(store: &mut Store, style: &Style, now: OffsetDateTime) -> Result<()> {
    let boards = store.boards()?;
    if boards.is_empty() {
        println!();
        println!("  Nothing to sync. No boards on the watchlist yet.");
        println!();
        println!(
            "  {}",
            style.dim("perch watch add <company>  finds a board and starts watching it")
        );
        println!();
        return Ok(());
    }

    let http = Http::new()?;
    let outcome = sync_all(store, &http, now)?;

    println!();
    for report in &outcome.reports {
        if report.quiet() {
            continue;
        }
        let mut parts = Vec::new();
        if report.first_seen > 0 {
            parts.push(format!("{} new", report.first_seen));
        }
        if report.reposted > 0 {
            parts.push(format!("{} reposted", report.reposted));
        }
        if report.retitled > 0 {
            parts.push(format!("{} retitled", report.retitled));
        }
        if report.relocated > 0 {
            parts.push(format!("{} moved", report.relocated));
        }
        if report.reopened > 0 {
            parts.push(format!("{} reopened", report.reopened));
        }
        if report.closed > 0 {
            parts.push(format!("{} came down", report.closed));
        }
        println!("  {}: {}", report.company, parts.join(", "));
    }

    for name in &outcome.unsupported {
        println!(
            "  {}",
            style.dim(&format!(
                "{name}: this board's ATS is not one this build can read"
            ))
        );
    }
    for (name, why) in &outcome.failures {
        println!(
            "  {}",
            style.dim(&format!("{name} could not be reached: {why}"))
        );
    }

    if outcome.quiet() {
        // Count what was actually read, not what was attempted.
        let n = outcome.reports.len();
        println!(
            "  Read {n} {}. Nothing new.",
            if n == 1 { "board" } else { "boards" }
        );
        println!("  {}", style.dim("Boards post in bursts. This is normal."));
    }
    println!();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn feed(
    store: &Store,
    paths: &Paths,
    style: &Style,
    fresh: bool,
    company: Option<String>,
    all: bool,
    now: OffsetDateTime,
) -> Result<()> {
    // Resolve before filtering so "not watched" and "watched but quiet" are
    // told apart. They need different sentences.
    let resolved = match company.as_deref() {
        Some(name) => Some((name, store.watched_company(name)?)),
        None => None,
    };
    let filter = FeedFilter {
        company: resolved
            .as_ref()
            .and_then(|(_, found)| found.as_ref())
            .map(|c| c.slug.clone()),
        fresh_only: fresh,
    };
    if let Some((name, None)) = &resolved {
        println!();
        println!("  {name} is not on the watchlist, so there is nothing to show.");
        println!();
        println!(
            "  {}",
            style.dim(&format!(
                "perch watch add {name}  finds its board and starts watching"
            ))
        );
        println!();
        return Ok(());
    }

    let open = store.feed(&filter, now)?;
    let open_count = open.len();

    // Read fresh every time: editing the rules file changes the next feed,
    // with nothing to re-sync and no cached verdict to go stale.
    //
    // A typo must not take the whole tool away. If the file will not parse,
    // say so and then show everything: the feed is what the person opened
    // this for, and a mistyped field name is not reason enough to withhold it.
    let mut rules_broke = None;
    let rules = if all {
        Rules::default()
    } else {
        match Rules::load(&paths.rules()) {
            Ok(rules) => rules,
            Err(err) => {
                rules_broke = Some(anyhow::Error::new(err));
                Rules::default()
            }
        }
    };
    if let Some(err) = &rules_broke {
        println!();
        println!("  Your rules did not parse, so this is every open role.");
        for line in describe(err) {
            println!("  {}", style.dim(&line));
        }
        println!(
            "  {}",
            style.dim(
                "perch rules edit  opens the file  ·  matching resumes once it reads as rules"
            )
        );
    }
    let roles = rules.apply(open);

    if roles.is_empty() {
        println!();
        if store.boards()?.is_empty() {
            println!("  No boards yet, so nothing to show.");
            println!();
            println!(
                "  {}",
                style.dim("perch watch add <company>  finds a board and starts watching it")
            );
        } else if open_count > 0 {
            // The roles are there; the rules are holding them back. Both the
            // count and the suggested command have to describe the same set the
            // person is looking at, filters included.
            let (roles, are) = if open_count == 1 {
                ("role", "is")
            } else {
                ("roles", "are")
            };
            let (what, escape) = match (&resolved, fresh) {
                (Some((name, _)), true) => (
                    format!("went up at {name} in the last day"),
                    format!("perch feed --company {name} --fresh --all"),
                ),
                (Some((name, _)), false) => (
                    format!("{are} open at {name}"),
                    format!("perch feed --company {name} --all"),
                ),
                (None, true) => (
                    "went up in the last day".to_string(),
                    "perch feed --fresh --all".to_string(),
                ),
                (None, false) => (
                    format!("{are} open on the boards you watch"),
                    "perch feed --all".to_string(),
                ),
            };
            println!("  Nothing matched your rules. {open_count} {roles} {what}.");
            println!(
                "  {}",
                style.dim(&format!(
                    "{escape}  shows them  ·  perch rules edit  changes what matches"
                ))
            );
        } else if fresh {
            println!("  Nothing went up in the last day.");
            println!(
                "  {}",
                style.dim("Boards post in bursts. `perch feed` shows everything open.")
            );
        } else if let Some((name, _)) = &resolved {
            println!("  Nothing open at {name} right now.");
            println!("  {}", style.dim("Perch will keep watching."));
        } else {
            println!("  Nothing open on the boards you're watching.");
            println!("  {}", style.dim("Boards post in bursts. This is normal."));
        }
        println!();
        return Ok(());
    }

    render::print_feed(&roles, style, now);
    Ok(())
}

/// Every line of an error, the way `main` prints one. A parse failure then
/// shows the line number and the offending field, not just a file path.
fn describe(err: &anyhow::Error) -> Vec<String> {
    std::iter::once(err.to_string())
        .chain(err.chain().skip(1).map(|c| c.to_string()))
        .flat_map(|line| {
            line.lines()
                .map(str::trim_end)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn rules_edit(paths: &Paths) -> Result<()> {
    let path = paths.rules();
    if !path.exists() {
        std::fs::write(&path, TEMPLATE)?;
        println!("Wrote a starting point to {}.", path.display());
        println!("Every rule in it is commented out, so nothing is matched away yet.");
    }

    // $VISUAL wins over $EDITOR by long convention, and either may carry flags
    // ("code --wait", "emacsclient -nw"). Split it the way git does.
    let configured = std::env::var_os("VISUAL")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("EDITOR").filter(|v| !v.is_empty()));

    let Some(configured) = configured else {
        println!("{}", path.display());
        println!("Set $EDITOR and Perch will open it for you.");
        return Ok(());
    };

    let spelled = configured.to_string_lossy().to_string();
    let mut words = spelled.split_whitespace();
    let Some(program) = words.next() else {
        println!("{}", path.display());
        println!("$EDITOR is set to nothing, so open it yourself.");
        return Ok(());
    };
    let args: Vec<&str> = words.collect();

    let status = std::process::Command::new(program)
        .args(&args)
        .arg(&path)
        .status();

    match status {
        Err(err) => {
            println!("Could not start {spelled}: {err}");
            println!("The file is at {}.", path.display());
            return Ok(());
        }
        Ok(status) if !status.success() => {
            println!("{spelled} closed without saving.");
            return Ok(());
        }
        Ok(_) => {}
    }

    // Read it back so a mistake is caught here, while the person is still
    // holding the file, rather than at the next feed.
    match Rules::load(&path) {
        Ok(rules) if rules.is_empty() => {
            println!("No rules are switched on, so the feed shows everything open.");
            Ok(())
        }
        Ok(rules) => {
            println!(
                "{} {} in place.",
                rules.rules.len(),
                if rules.rules.len() == 1 {
                    "rule"
                } else {
                    "rules"
                }
            );
            Ok(())
        }
        Err(err) => {
            let err = anyhow::Error::new(err);
            println!("That file does not read as rules yet:");
            for line in describe(&err) {
                println!("  {line}");
            }
            println!("Until it does, `perch feed` shows every open role and matches nothing away.");
            Ok(())
        }
    }
}

fn rules_list(paths: &Paths, style: &Style) -> Result<()> {
    let rules = Rules::load(&paths.rules())?;
    println!();
    if rules.is_empty() {
        println!("  No rules, so the feed shows every open role.");
        println!();
        println!(
            "  {}",
            style.dim("perch rules edit  writes a commented starting point")
        );
        println!();
        return Ok(());
    }

    println!(
        "  {}",
        style.dim("Tried top to bottom. The first to fire is the one the feed names.")
    );
    println!();
    for rule in &rules.rules {
        println!("  {}", style.rule(rule.name.trim()));
        for (label, items, negative) in [
            ("title contains", &rule.title, false),
            ("title excludes", &rule.title_excludes, true),
            ("location contains", &rule.location, false),
            ("location excludes", &rule.location_excludes, true),
            ("company is", &rule.company, false),
        ] {
            if items.is_empty() {
                continue;
            }
            // A positive condition fires on any one of its entries; an
            // exclusion needs all of them absent. Same list, opposite meaning,
            // so they must not be punctuated identically.
            let joined = if negative {
                items.join(", ")
            } else {
                match items.as_slice() {
                    [one] => one.clone(),
                    [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
                    [] => unreachable!(),
                }
            };
            let line = format!("{label} {joined}");
            println!("  {}", if negative { style.dim(&line) } else { line });
        }
        println!();
    }
    Ok(())
}

fn rules_test(store: &Store, paths: &Paths, style: &Style, reference: &str) -> Result<()> {
    let role = store.role_by_reference(reference)?;
    let rules = Rules::load(&paths.rules())?;

    println!();
    println!("  {}", style.dim(&role.company_name));
    println!("  {}", role.title);
    println!("  {}", style.dim(&role.location));
    println!();

    // A rule firing is not the same as a role reaching the feed. Something else
    // may already be keeping it out, and saying "this is the one the feed
    // names" over the top of that would be false.
    let held_back = if role.dismissed_at.is_some() {
        Some(format!(
            "you set this aside, so it is out of the feed either way. `perch restore {}` puts it back",
            role.reference
        ))
    } else if role.closed_at.is_some() {
        Some("this came down off the board, so it is out of the feed either way".to_string())
    } else if store.watched_company(&role.company_name)?.is_none() {
        Some(format!(
            "you stopped watching {}, so it is out of the feed either way",
            role.company_name
        ))
    } else {
        None
    };

    if rules.is_empty() {
        match &held_back {
            None => println!("  No rules are written, so every open role reaches the feed."),
            Some(why) => {
                println!("  No rules are written, but {why}.");
            }
        }
        println!();
        return Ok(());
    }

    let verdicts = rules.explain(&role);
    let mut named = false;
    for (name, why) in &verdicts {
        match why {
            Some(why) if !named => {
                println!("  {}: {}", style.rule(name), why.because);
                if held_back.is_none() {
                    println!("  {}", style.dim("this is the one the feed names"));
                }
                named = true;
            }
            Some(why) => println!(
                "  {}{}",
                style.rule(name),
                style.dim(&format!(": {} (also fires)", why.because))
            ),
            None => println!("  {}  {}", style.dim(name), style.dim("does not fire")),
        }
    }

    println!();
    match (&held_back, named) {
        (Some(why), true) => {
            println!("  A rule fires, but {why}.");
            println!();
        }
        (Some(why), false) => {
            println!("  Nothing fires, and {why}.");
            println!();
        }
        (None, false) => {
            println!("  Nothing fires, so this role does not reach the feed.");
            println!(
                "  {}",
                style.dim("perch feed --all  shows it anyway  ·  perch rules edit  changes that")
            );
            println!();
        }
        (None, true) => {}
    }
    Ok(())
}

// ---- the model, and whether the résumé may be sent to it ------------------

fn model_list(paths: &Paths, style: &Style) -> Result<()> {
    let configured = LlmModel::load(&paths.model())?;
    let client = LlmClient::new()?;

    println!();
    match client.probe_ollama() {
        Some(models) if !models.is_empty() => {
            println!("  {}", style.heading("On this Mac"));
            println!();
            println!("  {}", style.dim("Ollama is running on localhost:11434."));
            println!();
            for model in &models {
                let chosen = configured.model == model.name;
                let size = model
                    .size
                    .map(|b| format!("{:.1} GB on disk", b as f64 / 1e9))
                    .unwrap_or_default();
                println!(
                    "  {}  {}",
                    if chosen {
                        style.accent(&model.name)
                    } else {
                        model.name.clone()
                    },
                    style.dim(&size)
                );
                println!(
                    "  {}",
                    style.dim(&if chosen {
                        "in use".to_string()
                    } else {
                        format!("perch model set {} switches to it", model.name)
                    })
                );
            }
            println!();
            println!(
                "  {}",
                style.dim("Reading a résumé is a small job. A 3B model does it well enough.")
            );
        }
        _ => {
            println!("  {}", style.heading("On this Mac"));
            println!();
            println!("  Nothing is listening on localhost:11434, so Ollama is not running here.");
            println!(
                "  {}",
                style.dim("perch model endpoint <url>  points at any OpenAI-compatible server")
            );
        }
    }

    println!();
    println!("  {}", style.heading("Configured"));
    println!();
    if configured.configured() {
        println!(
            "  {} at {}",
            configured.model,
            style.dim(&configured.endpoint)
        );
        // Says whether the resume leaves this Mac. Load-bearing.
        println!("  {}", style.dim(&configured.consequence()));
    } else {
        println!("  No model.");
    }
    println!();
    println!(
        "  {}",
        style.dim("Watching, matching, filling and tracking need no model.")
    );
    println!();
    Ok(())
}

fn model_set(paths: &Paths, style: &Style, name: &str, endpoint: Option<&str>) -> Result<()> {
    let mut model = LlmModel::load(&paths.model())?;
    let was = model.host().filter(|_| !model.is_local());
    model.model = name.to_string();
    if let Some(endpoint) = endpoint {
        usable_endpoint(endpoint)?;
        model.endpoint = endpoint.to_string();
        // Pointing somewhere new withdraws consent given for somewhere else.
        model.consent.resume_import_may_leave_this_mac = false;
    } else if model.endpoint.trim().is_empty() {
        model.endpoint = format!("{OLLAMA}/v1");
    }
    model.save(&paths.model())?;
    let left_behind = was.filter(|old| Some(old.as_str()) != model.host().as_deref());

    println!("{name} will read résumés.");
    println!("{}", model.consequence());
    if matches!(model.may_send_document(), LlmPermission::Refused { .. }) {
        println!();
        println!("`perch model endpoint <url> --allow-resume` says it may.");
    }
    say_a_key_may_be_left_behind(style, left_behind.as_deref());
    Ok(())
}

/// An endpoint has to be an address Perch could call before it is written
/// down. The consent gate reads the host off the same parse, so a string it
/// cannot parse would be stored as "no model" and never asked about.
fn usable_endpoint(url: &str) -> Result<()> {
    let probe = LlmModel {
        endpoint: url.to_string(),
        ..LlmModel::default()
    };
    if probe.host().is_none() {
        anyhow::bail!(
            "{url} is not an address Perch can ask. An OpenAI-compatible base URL starts with http:// or https://, like https://api.example.com/v1"
        );
    }
    Ok(())
}

fn model_endpoint(paths: &Paths, style: &Style, url: &str, allow_resume: bool) -> Result<()> {
    usable_endpoint(url)?;
    let mut model = LlmModel::load(&paths.model())?;
    let was = model.host().filter(|_| !model.is_local());
    model.endpoint = url.to_string();
    model.consent.resume_import_may_leave_this_mac = allow_resume;
    model.save(&paths.model())?;
    let left_behind = was.filter(|old| Some(old.as_str()) != model.host().as_deref());

    println!("Perch will ask {url}.");
    println!("{}", model.consequence());
    if !model.is_local() && allow_resume {
        println!();
        println!("Your résumé will be sent there when you run `perch profile import`.");
        println!("Nothing else Perch does sends anything anywhere.");
    }
    if model.model.trim().is_empty() {
        println!();
        println!("`perch model list` shows what it can run.");
    }
    say_a_key_may_be_left_behind(style, left_behind.as_deref());
    Ok(())
}

/// Put the key for the configured endpoint in the system keychain, or take it
/// out again.
///
/// The key is read from the terminal rather than taken as an argument, because
/// an argument is in the shell's history and in the output of `ps` for anyone
/// on the machine to read. It is never written to model.toml: that file is
/// meant to be one a person can open, copy and paste without handing over a
/// credential by accident.
fn model_key(paths: &Paths, style: &Style, forget: bool, named: Option<&str>) -> Result<()> {
    use std::io::{BufRead, Write};

    // A host named here is forgotten whatever the endpoint says. Pointing the
    // endpoint somewhere else leaves the old host's key in the keychain, and
    // this is how it is reached afterwards.
    if let Some(host) = named {
        llm_secret::forget(host).map_err(perch_core::Error::msg)?;
        println!();
        println!("  Perch is holding no key for {host}.");
        println!();
        return Ok(());
    }

    let model = LlmModel::load(&paths.model())?;
    let Some(host) = model.host() else {
        println!();
        println!("  No endpoint is configured, so there is nothing to hold a key for.");
        println!(
            "  {}",
            style.dim("perch model endpoint <url>  points at one")
        );
        println!();
        return Ok(());
    };

    if forget {
        llm_secret::forget(&host).map_err(perch_core::Error::msg)?;
        println!();
        println!("  Perch is holding no key for {host}.");
        println!();
        return Ok(());
    }

    if model.is_local() {
        println!();
        println!("  {host} is on this Mac and wants no key.");
        println!();
        return Ok(());
    }

    print!("  Key for {host} (it is not shown as you type it): ");
    std::io::stdout().flush()?;
    let mut typed = String::new();
    let read = {
        let _quiet = EchoOff::new();
        std::io::stdin().lock().read_line(&mut typed)?
    };
    // Nothing was echoed, the Return included, so the cursor is still sitting
    // at the end of the prompt. Put it on the next line before anything prints.
    println!();
    if read == 0 {
        return Ok(());
    }
    let key = typed.trim();
    if key.is_empty() {
        println!();
        println!("  Nothing was typed, so nothing was stored.");
        println!();
        return Ok(());
    }

    llm_secret::set(&host, key).map_err(perch_core::Error::msg)?;
    println!();
    println!("  Stored in this Mac's keychain, for {host}.");
    println!(
        "  {}",
        style.dim("perch model key --forget  takes it back out")
    );
    println!();
    Ok(())
}

fn model_off(paths: &Paths, style: &Style) -> Result<()> {
    // The file is about to be replaced, so one Perch cannot parse is not a
    // reason to refuse. It only costs the sentence about a key left behind.
    let left_behind = LlmModel::load(&paths.model())
        .ok()
        .and_then(|model| model.host().filter(|_| !model.is_local()));
    LlmModel::default().save(&paths.model())?;
    println!("No model configured.");
    println!("Watching, matching, filling and tracking work as before.");
    println!("Reading a résumé into proposed fields is the only thing that needs one.");
    say_a_key_may_be_left_behind(style, left_behind.as_deref());
    Ok(())
}

/// Say that a key for the host just pointed away from is still in the keychain.
///
/// Whether one is really held cannot be checked here. Reading a key asks macOS
/// for permission, and asking that every time an endpoint changes teaches
/// people to approve it without looking. So this is said as a condition, and
/// the command that settles it is named.
fn say_a_key_may_be_left_behind(style: &Style, left_behind: Option<&str>) {
    let Some(host) = left_behind else { return };
    println!();
    println!("If you gave Perch a key for {host}, it is still in the keychain.");
    println!(
        "{}",
        style.dim(&format!(
            "perch model key --forget --host {host}  takes it out"
        ))
    );
}

// ---- the profile, and reading a résumé into it -----------------------------

fn profile_show(paths: &Paths, style: &Style) -> Result<()> {
    let profile = perch_core::Profile::load(&paths.profile())?;
    println!();
    println!("  {}", style.dim(&paths.profile().display().to_string()));
    println!();
    for (label, value) in [
        ("Name", profile.name.clone()),
        ("Email", profile.email.clone()),
        ("Phone", profile.phone.clone()),
        ("Location", profile.location.clone()),
        ("Work authorisation", profile.work_authorisation.clone()),
        ("GitHub", profile.links.github.clone()),
        ("Website", profile.links.website.clone()),
        ("Skills", profile.skills.join(", ")),
    ] {
        println!("  {}", style.dim(label));
        println!(
            "  {}",
            if value.trim().is_empty() {
                style.dim("not set")
            } else {
                value
            }
        );
        println!();
    }
    for (n, role) in profile.experience.iter().enumerate() {
        println!("  {}", style.dim(&format!("Position {}", n + 1)));
        println!("  {} · {}", role.company, role.title);
        println!("  {}", style.dim(&role.dates));
        println!();
    }
    println!(
        "  {}",
        style.dim("Perch never fills demographic questions. This file has nowhere to keep an answer to one.")
    );
    println!();
    Ok(())
}

fn profile_import(paths: &Paths, style: &Style, file: &str, accept_quoted: bool) -> Result<()> {
    let model = LlmModel::load(&paths.model())?;
    if !model.configured() {
        println!();
        println!("  No model is configured, so there is nothing to read the résumé with.");
        println!(
            "  {}",
            style.dim("perch model list  shows what this Mac can run")
        );
        println!();
        println!(
            "  {}",
            style.dim("Watching, matching, filling and tracking need no model.")
        );
        println!();
        return Ok(());
    }

    // The consent gate, stated before anything is read, not after.
    if let LlmPermission::Refused { host } = model.may_send_document() {
        println!();
        println!("  {host} is not on this Mac.");
        println!("  Perch will not send your résumé there until you say it may.");
        println!();
        println!(
            "  {}",
            style.dim(&format!(
                "perch model endpoint {} --allow-resume  says it may",
                model.endpoint
            ))
        );
        println!();
        return Ok(());
    }

    let document = llm_document::read(std::path::Path::new(file))?;
    let profile_path = paths.profile();
    let mut profile = perch_core::Profile::load(&profile_path)?;

    println!();
    println!("  Reading {}…", document.name);
    println!("  {}", style.dim(&model.consequence()));

    let key = model
        .host()
        .filter(|_| !model.is_local())
        .and_then(|host| llm_secret::get(&host));
    let client = LlmClient::new()?;
    let mut proposals = match llm_extract::run(&client, &model, key.as_deref(), &document, &profile)
    {
        Ok(proposals) => proposals,
        // Said with the thing to do about it, the way every other stop in this
        // program is. A status code on its own leaves a person guessing.
        Err(perch_llm::Error::Unauthorized { host }) => {
            println!();
            println!("  {host} would not take the request without a key it accepts.");
            println!(
                "  {}",
                style.dim("perch model key  stores one in this Mac's keychain")
            );
            println!();
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };

    if proposals.is_empty() {
        println!();
        println!(
            "  Nothing in {} anchored to text Perch could find in it.",
            document.name
        );
        println!();
        return Ok(());
    }

    println!();
    println!(
        "  {}",
        style.dim(&format!(
            "Nothing is written to {} until you accept it here.",
            profile_path.display()
        ))
    );
    println!();

    let mut offered = 0usize;
    let mut refused = 0usize;
    for proposal in proposals.iter_mut() {
        println!("  {}", style.heading(&proposal.field.label()));
        println!(
            "  {}",
            style.dim(&match &proposal.current {
                Some(current) => format!("in profile  {current}"),
                None => "in profile  not set".to_string(),
            })
        );
        println!("  {}", proposal.value);

        match (&proposal.quote, proposal.refusal(&document.name)) {
            (_, Some(refusal)) => {
                // Not offered at all: no accept, no prompt.
                println!("  {}", style.dim(&refusal));
                refused += 1;
                proposal.accepted = false;
            }
            (Some(quote), None) => {
                offered += 1;
                let line = proposal
                    .line
                    .map(|n| format!("line {n}"))
                    .unwrap_or_default();
                println!("  {}  {}", style.dim(quote.trim()), style.dim(&line));
                if let Some(caution) = proposal.caution() {
                    println!("  {}", style.dim(&caution));
                }
                proposal.accepted = ask(proposal.accepted, accept_quoted, &proposal.anchor)?;
            }
            (None, None) => {
                refused += 1;
                proposal.accepted = false;
            }
        }
        println!();
    }

    let accepted = proposals.iter().filter(|p| p.accepted).count();
    if accepted == 0 {
        println!("  Nothing accepted, so nothing was written.");
        println!();
        return Ok(());
    }

    let written = llm_extract::apply(&proposals, &mut profile);
    profile.save(&profile_path)?;

    println!(
        "  Wrote {written} {} to {}.",
        if written == 1 { "field" } else { "fields" },
        profile_path.display()
    );
    let skipped = offered.saturating_sub(accepted);
    if skipped > 0 || refused > 0 {
        let mut parts = Vec::new();
        if skipped > 0 {
            parts.push(format!("{skipped} skipped"));
        }
        if refused > 0 {
            parts.push(format!("{refused} Perch is not offering"));
        }
        println!("  {}", style.dim(&parts.join(", ")));
    }
    println!();
    Ok(())
}

/// Ask about one proposal. A quotation may be taken as read with
/// `--accept-quoted`; anything the model rewrote is always asked about.
fn ask(pre_accepted: bool, accept_quoted: bool, anchor: &perch_llm::Anchor) -> Result<bool> {
    use std::io::{BufRead, Write};

    if accept_quoted && matches!(anchor, perch_llm::Anchor::Exact { .. }) {
        println!("  accepted");
        return Ok(true);
    }

    let prompt = if pre_accepted {
        "  accept? [Y/n] "
    } else {
        "  accept? [y/N] "
    };
    print!("{prompt}");
    std::io::stdout().flush()?;

    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        // No one is there to answer, so nothing is accepted.
        println!();
        return Ok(false);
    }
    Ok(match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => pre_accepted,
        _ => false,
    })
}

fn open_role(store: &Store, style: &Style, reference: &str, now: OffsetDateTime) -> Result<()> {
    let role = store.role_by_reference(reference)?;
    let signal = role.signal(now);

    println!();
    println!("  {}", style.dim(&role.company_name));
    println!("  {}", role.title);
    let mut meta = vec![role.location.clone()];
    meta.push(style.by_freshness(signal.freshness(now), &signal.text));
    meta.push(style.dim(role.ats.label()));
    if !role.fill_supported {
        meta.push(style.dim("opens in browser"));
    }
    println!("  {}", meta.join(style.dim(" · ").as_str()));
    println!("  {}", style.dim(&role.url));
    println!();

    // The posting's own words, fetched now if this is the first time.
    let http = Http::new()?;
    match ensure_description(store, &role, &http, now) {
        Ok(Some(text)) => {
            for block in html::to_blocks(&text) {
                match block {
                    html::Block::Heading(t) => {
                        println!("  {}", style.heading(&t));
                        println!();
                    }
                    html::Block::Bullet(t) => println!("  {}", wrap(&format!("• {t}"), 4)),
                    html::Block::Paragraph(t) => {
                        println!("  {}", wrap(&t, 2));
                        println!();
                    }
                }
            }
        }
        Ok(None) => println!(
            "  {}",
            style.dim("The board no longer has this posting's text.")
        ),
        Err(err) => println!(
            "  {}",
            style.dim(&format!("Could not read the posting's text: {err}"))
        ),
    }

    // Board history: only what Perch saw itself.
    let events = store.events(role.id)?;
    if !events.is_empty() {
        println!();
        println!("  {}", style.heading("Board history"));
        println!();
        for event in &events {
            let when = perch_core::verb_signal("", event.at, now)
                .trim()
                .to_string();
            let what = match (event.kind, event.detail.as_deref()) {
                (perch_core::EventKind::FirstSeen, _) => "Perch first saw it".to_string(),
                (perch_core::EventKind::Reposted, _) => "the board reposted it".to_string(),
                (perch_core::EventKind::Retitled, Some(d)) => format!("retitled: {d}"),
                (perch_core::EventKind::Retitled, None) => "retitled".to_string(),
                (perch_core::EventKind::Relocated, Some(d)) => format!("moved: {d}"),
                (perch_core::EventKind::Relocated, None) => "moved".to_string(),
                (perch_core::EventKind::Closed, _) => "it came down".to_string(),
                (perch_core::EventKind::Reopened, _) => "it went back up".to_string(),
            };
            println!("  {}  {}", style.dim(&when), what);
        }
        println!();
        println!(
            "  {}",
            style.dim("Perch only knows what it has seen since it started watching this board.")
        );
    }

    // Hiring velocity, if there is enough of it to stand behind.
    if let Some(v) = store.velocity(role.board_id, now)? {
        println!();
        println!("  {}", style.heading("Hiring velocity"));
        println!();
        println!(
            "  {} {} opened in the last 90 days, {} still open.",
            v.opened_recently,
            if v.opened_recently == 1 {
                "role"
            } else {
                "roles"
            },
            v.still_open
        );
        if let Some(days) = v.median_days_to_close {
            println!("  Half came down within {days} days.");
        }
        println!(
            "  {}",
            style.dim(&format!(
                "Observed, not reported. Perch has only been watching this board {}.",
                match perch_core::span(now - v.watching_since).as_str() {
                    "a few minutes" => "for a few minutes".to_string(),
                    other => format!("for {other}"),
                }
            ))
        );
    }

    println!();
    println!(
        "  {}",
        style.dim(&format!(
            "perch apps mark {} in-flight   once you have applied",
            role.reference
        ))
    );
    println!();
    Ok(())
}

/// Soft-wrap prose to something readable in a terminal.
fn wrap(text: &str, indent: usize) -> String {
    const WIDTH: usize = 76;
    let pad = " ".repeat(indent);
    let mut out = String::new();
    let mut column = 0;
    for word in text.split_whitespace() {
        if column > 0 && column + 1 + word.chars().count() > WIDTH {
            out.push('\n');
            out.push_str(&pad);
            column = 0;
        } else if column > 0 {
            out.push(' ');
            column += 1;
        }
        out.push_str(word);
        column += word.chars().count();
    }
    out
}

// ---- applying: review, attach, open in the browser -------------------------

/// The same three steps as the interface's apply sheet, in the terminal:
/// review what would be typed, say which document goes with it, then open the
/// form. It ends there. Perch submits nothing.
fn apply(
    store: &Store,
    paths: &Paths,
    style: &Style,
    reference: &str,
    resume: Option<&str>,
) -> Result<()> {
    let role = store.role_by_reference(reference)?;
    let profile = perch_core::Profile::load(&paths.profile())?;

    // Whatever was asked for, or the first document the profile calls a
    // résumé. Nothing is guessed at beyond that.
    let attached: Option<(String, String)> = match resume {
        Some(path) => Some((file_name(path), path.to_string())),
        None => profile
            .documents
            .iter()
            .find(|d| d.kind.to_lowercase().contains("sum"))
            .map(|d| {
                let name = if d.name.trim().is_empty() {
                    file_name(&d.path)
                } else {
                    d.name.clone()
                };
                (name, d.path.clone())
            }),
    };

    let Some(plan) = perch_fill::plan::build(
        role.ats,
        &role.url,
        &profile,
        attached.as_ref().map(|(_, path)| path.as_str()),
    ) else {
        // Watched, readable, but not fillable. Perch does not know this
        // board's form.
        println!();
        println!(
            "  Perch reads {} boards but does not know their forms, so there is",
            role.ats.label()
        );
        println!("  nothing for it to fill in here.");
        println!();
        println!("  {}", role.url);
        println!();
        println!("  {}", style.dim("Open that and fill it in yourself."));
        println!();
        return Ok(());
    };

    if profile.is_empty() {
        println!();
        println!("  Your profile is empty, so there is nothing to fill from.");
        println!();
        println!(
            "  {}",
            style.dim("perch profile import <file>  reads a résumé into it, a field at a time")
        );
        println!(
            "  {}",
            style.dim("perch profile show           shows what is there")
        );
        println!();
        return Ok(());
    }

    println!();
    println!("  {}", style.dim(&role.company_name));
    println!("  {}", role.title);
    let mut meta = Vec::new();
    if !role.location.is_empty() {
        meta.push(role.location.clone());
    }
    meta.push(style.dim(role.ats.label()));
    println!("  {}", meta.join(style.dim(" · ").as_str()));
    println!("  {}", style.dim(&role.url));
    println!();

    // Step one: what Perch would type, and where each value came from.
    if !plan.entries.is_empty() {
        println!("  {}", style.heading("From your profile"));
        println!();
        for entry in &plan.entries {
            println!("  {}", style.dim(&entry.label));
            println!(
                "  {}  {}",
                entry.action.shown_value(),
                style.dim(&entry.provenance.label())
            );
            println!();
        }
    }

    // Deliberate blanks, each named rather than left to be noticed. An empty
    // box Perch chose is not the same as one it missed.
    if !plan.flagged.is_empty() {
        println!("  {}", style.heading("Perch is not guessing at these"));
        println!();
        for flagged in &plan.flagged {
            println!("  {}", flagged.label);
            println!("  {}", style.dim(&wrap(&flagged.why, 2)));
            println!();
        }
    }

    if !plan.left_to_you.is_empty() {
        println!("  {}", style.heading("Left to you"));
        println!();
        for free in &plan.left_to_you {
            println!("  {}", free.label);
            println!("  {}", style.dim(&wrap(&free.why, 2)));
            println!();
        }
    }

    if !plan.never.is_empty() {
        println!("  {}", style.heading("Never answered"));
        println!();
        // Every question is named and so is every reason, but a reason shared
        // by several questions is stated once.
        let mut grouped: Vec<(&str, Vec<&str>)> = Vec::new();
        for refused in &plan.never {
            match grouped.iter_mut().find(|(why, _)| *why == refused.why) {
                Some((_, labels)) => labels.push(&refused.label),
                None => grouped.push((&refused.why, vec![&refused.label])),
            }
        }
        for (why, labels) in grouped {
            println!("  {}", style.dim(&labels.join(" · ")));
            println!("  {}", style.dim(&wrap(why, 2)));
            println!();
        }
    }

    // Step two: the one file, named.
    println!("  {}", style.heading("Attached"));
    println!();
    match &attached {
        Some((name, path)) => {
            println!("  {name}");
            println!("  {}", style.dim(path));
            println!(
                "  {}",
                style.dim(&wrap("Perch attaches that file as it stands.", 2))
            );
        }
        None => {
            println!("  No document, so the file box stays empty.");
            println!(
                "  {}",
                style.dim(&format!(
                    "perch apply {} --resume <path>  attaches one",
                    role.reference
                ))
            );
        }
    }
    println!();

    // Step three: where this stops, in the plan's own words.
    println!("  {}", style.heading("What happens next"));
    println!();
    println!("  {}", wrap(&plan.what_happens_next(&role.company_name), 2));
    println!();
    println!(
        "  {}",
        style.dim("The desktop app does the typing. From here Perch opens the page.")
    );
    println!();

    if !ask_to_open(&format!(
        "  open {}'s form in your browser? [y/N] ",
        role.company_name
    ))? {
        println!();
        println!("  {}", style.dim("Nothing was opened."));
        println!();
        return Ok(());
    }

    println!();
    // `open` on a Mac, `xdg-open` everywhere else. Either way it hands the URL
    // to the desktop's own handler.
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match std::process::Command::new(opener).arg(&role.url).status() {
        Ok(status) if status.success() => {
            println!("  The form is open in your browser.");
        }
        Ok(_) => {
            println!("  {opener} could not open a window here, so the address is:");
            println!("  {}", role.url);
            println!("  {}", style.dim("Open it yourself."));
        }
        Err(err) => {
            println!("  Could not run {opener} here: {err}");
            println!("  {}", role.url);
            println!("  {}", style.dim("Open it yourself."));
        }
    }
    println!();
    println!(
        "  {}",
        wrap(
            "That is everything Perch does. It sends nothing. You read the form over and \
             submit it yourself.",
            2
        )
    );
    println!(
        "  {}",
        style.dim(&format!(
            "perch apps mark {} in-flight   once you have sent it",
            role.reference
        ))
    );
    println!();
    Ok(())
}

/// A file's own name, for saying which document is attached.
fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Ask before opening a browser window, in the shape `profile import` asks in.
/// Nobody at the other end of stdin means nothing is opened.
fn ask_to_open(prompt: &str) -> Result<bool> {
    use std::io::{BufRead, Write};

    print!("{prompt}");
    std::io::stdout().flush()?;

    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        println!();
        return Ok(false);
    }
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn apps_list(store: &Store, style: &Style, now: OffsetDateTime) -> Result<()> {
    // Silence past thirty days is filed away before anything is shown, so the
    // list is the same however it is reached.
    store.archive_quiet_applications(now)?;
    let apps = store.applications()?;

    println!();
    if apps.is_empty() {
        println!("  No applications yet.");
        println!(
            "  {}",
            style.dim("perch apps mark <ref> in-flight  records one once you have sent it")
        );
        println!();
        return Ok(());
    }

    for state in [
        ApplicationState::InFlight,
        ApplicationState::Responded,
        ApplicationState::Archived,
    ] {
        let group: Vec<_> = apps.iter().filter(|a| a.state == state).collect();
        if group.is_empty() {
            continue;
        }
        println!("  {}", style.heading(state.heading()));
        println!();
        if state == ApplicationState::Archived {
            println!(
                "  {}",
                style.dim(
                    "After 30 days without a reply Perch moves an application here on its own. Nothing is deleted."
                )
            );
            println!();
        }
        for app in group {
            let at = app.note_at.unwrap_or(app.applied_at);
            let freshness = perch_core::Freshness::at(at, now);
            let sent = perch_core::verb_signal("applied", app.applied_at, now);
            let mut line = vec![style.by_freshness(freshness, &sent)];
            match (&app.note, app.archived_quietly) {
                (Some(note), _) => line.push(note.clone()),
                (None, true) => line.push(style.dim("no reply in 30 days")),
                (None, false) => line.push(style.dim("nothing since")),
            }
            println!("  {}", style.dim(&app.company_name));
            println!(
                "  {}  {}",
                style.title_by_freshness(freshness, &app.title),
                style.dim(&app.reference)
            );
            println!("  {}", line.join(style.dim(" · ").as_str()));
            println!();
        }
    }
    Ok(())
}

fn apps_mark(
    store: &Store,
    reference: &str,
    state: &str,
    note: Option<&str>,
    now: OffsetDateTime,
) -> Result<()> {
    let role = store.role_by_reference(reference)?;
    let state = ApplicationState::parse(state)?;
    store.mark_application(role.id, state, note, now)?;

    let where_it_is = format!("{} at {}", role.title, role.company_name);
    match state {
        ApplicationState::InFlight => {
            println!("Recorded: you applied to {where_it_is}.");
            println!("Perch had no part in sending it, and will not chase it.");
        }
        ApplicationState::Responded => println!("Recorded a reply on {where_it_is}."),
        ApplicationState::Archived => {
            println!("Filed {where_it_is} away. Nothing is deleted.");
        }
    }
    Ok(())
}

fn dismiss(store: &Store, reference: &str, now: OffsetDateTime) -> Result<()> {
    let role = store.role_by_reference(reference)?;
    let where_it_is = format!("{} at {}", role.title, role.company_name);

    if role.closed_at.is_some() {
        println!("{where_it_is} already came down off the board, so it is not in the feed.");
        return Ok(());
    }
    if role.dismissed_at.is_some() {
        println!("{where_it_is} was already set aside. Nothing changed.");
        println!("`perch restore {}` puts it back.", role.reference);
        return Ok(());
    }

    store.dismiss(role.id, now)?;
    println!("Set aside {where_it_is}.");
    println!(
        "Nothing is deleted. `perch restore {}` puts it back.",
        role.reference
    );
    Ok(())
}

fn restore(store: &Store, reference: &str) -> Result<()> {
    let role = store.role_by_reference(reference)?;
    let where_it_is = format!("{} at {}", role.title, role.company_name);

    if role.dismissed_at.is_none() {
        println!("{where_it_is} was not set aside. Nothing changed.");
        return Ok(());
    }

    store.undismiss(role.id)?;
    if role.closed_at.is_some() {
        // Saying a role is "back in the feed" when the board took it down
        // would be false.
        println!("{where_it_is} is no longer set aside.");
        println!("It came down off the board, though, so the feed will not list it.");
    } else {
        println!("{where_it_is} is back in the feed.");
    }
    Ok(())
}

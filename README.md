# Perch

A local-first job search companion. It watches company job boards, tells you
plainly how old everything is, and helps fill application forms from a profile
you wrote yourself.

No account, no backend, no telemetry. Everything stays on your machine.

## What it will not do

**Perch never submits an application.** It fills a form from a profile you have
approved, and then it stops, with the form open and you in front of it.

This is not a setting. [`perch_fill::Action`](crates/fill/src/lib.rs) has three
variants (set a value, choose an option, attach a file), and none of them
activates a control. A fill plan cannot express a click or a submit, so no
caller can ask for one. Two tests read the generated JavaScript and assert it
contains no `.submit(`, `requestSubmit`, `.click(`, `fetch(` or `sendBeacon`,
which checks the promise against what actually runs rather than against an
intention.

Where it types is guarded too. The form's address comes out of a board's own
JSON, so Perch will only type into a page the ATS genuinely serves forms from,
only on the exact page you reviewed, and only once. A closed posting that
redirects, or a link clicked inside the filled window, gets nothing.

Three kinds of box are left empty on purpose, each saying so: ones Perch would
have to guess at (a start date), ones that are yours to write (a cover letter),
and demographic questions, which are refused by kind *and* independently by the
label the form uses, so a question no list anticipated still cannot be answered
on your behalf.

It also never fills or stores EEO and demographic questions, and anything an
LLM extracts from your résumé arrives as a proposal in a review diff, traced
back to the text it came from, never as a direct write.

## Where it is

The design is settled; see [`design/`](design/), starting at `design/index.html`.

Built so far: `crates/core`, `crates/cli` and the Tauri app in `app/`,
synchronous, Greenhouse only, plus `crates/llm` and `crates/fill`. Watching,
syncing, matching, the feed and detail pane, application tracking, the
watchlist, résumé import and form filling all work end to end against real
boards.

Everything in the build order is in.

## Using it

```
cargo run -p perch-cli -- watch add figma
cargo run -p perch-cli -- sync
cargo run -p perch-cli -- feed
```

| command | what it does |
|---|---|
| `perch watch add <company>` | finds a board from a name, a board URL or a careers page, and starts watching |
| `perch watch list` | companies, their boards, and when each was last read |
| `perch watch rm <company>` | stops watching, keeping everything it has seen |
| `perch sync` | reads every watched board now |
| `perch feed` | open roles, newest first |
| `perch feed --fresh` | only what went up in the last day |
| `perch feed --company <name>` | one company only |
| `perch feed --all` | every open role, whether or not a rule fires |
| `perch rules edit` | opens the rules file, writing a commented starting point if there is none |
| `perch rules list` | the rules as Perch reads them, in the order they are tried |
| `perch rules test <ref>` | which rule fires on one role, and why |
| `perch open <ref>` | one role in full: its text, board history, hiring velocity |
| `perch apps` | applications: in flight, responded, archived |
| `perch apps mark <ref> <state>` | record where one stands, with `--note` |
| `perch model list` | models this Mac can run, and what is configured |
| `perch model set <name>` | choose the model that reads résumés |
| `perch model endpoint <url>` | point at a remote server, with `--allow-resume` |
| `perch model off` | run with no model at all |
| `perch profile show` | the profile as Perch reads it |
| `perch profile import <file>` | propose fields from a résumé, one at a time |
| `perch apply <ref>` | review the fill plan, attach a document, open the form |
| `perch dismiss <ref>` | takes a role out of the feed |
| `perch restore <ref>` | puts it back |

The command palette in the desktop interface offers these same words, verbatim.

`--home <dir>` keeps everything under a directory of your choosing, which is
handy for trying it out without touching your real data.

## Rules

A role reaches the feed if any rule in `~/.config/perch/rules.toml` fires on it.
A rule fires when every line in it holds. Rules are tried top to bottom and the
first to fire is the one the feed names.

```toml
[[rule]]
name = "rust-in-title"
title = ["rust"]

[[rule]]
name = "not-staff-plus"
title = ["engineer"]
title_excludes = ["staff", "principal"]
```

Nothing is scored or weighted. A rule fires or it does not. What Perch can do
honestly is say which rule fired and what set it off, so every row explains
itself in one line:

```
Astral
Rust Engineer, uv  4a2a53a5
Remote (US or EU) · posted 9 hours ago
matched rust-in-title: 'Rust' in the title
```

Matching is case-insensitive and works on whole words, so a rule looking for
`rust` fires on "Rust Engineer" but not on "Trust & Safety". A row that cannot
explain itself honestly is worse than a row that never appeared. Several words
in one entry match as a phrase.

**With no rules, the feed shows everything.** Rules narrow it; they are not a
gate you have to satisfy before Perch is useful. Rules are read fresh on every
command, so editing the file changes the next `feed` with nothing to re-sync.

A rules file that will not parse does not take the feed away either: `perch
feed` says what went wrong, points at the line, and then shows every open role
until the file reads as rules again.

Rules see the title, location and company that the board puts in its listing.
Descriptions are not read yet.

## Reading a résumé

A model is used for exactly one thing: proposing profile fields from a résumé.
Watching, matching, filling and tracking need no model at all, and running
without one is a complete way to use Perch rather than a reduced one.

There is no inference in this process. One OpenAI-compatible client covers
Ollama, LM Studio, llama.cpp's server, OpenRouter and Fireworks; Perch probes
`localhost:11434` and offers whatever is already installed.

**Nothing a model says is taken on trust.** Every proposed value is checked back
against the document it supposedly came from, and there are three outcomes:

| | |
|---|---|
| the document says it | offered, and arrives accepted, with the line quoted |
| the document says something it was read *from* | offered, never pre-accepted, with the source shown |
| the document does not say it | **not offered at all**: there is no Accept to press |

Schema-constrained output makes replies easier to parse. It is not the
guarantee; verification is, and it holds whatever is on the other end of the
endpoint. A model that invents an employer wastes a moment of your time and
cannot do more than that.

Text that appears only *inside* something larger, such as `dferreira.dev`
sitting in `dana@dferreira.dev`, is treated as read rather than quoted,
because the document contains those characters without stating them as a
value of its own.

Nothing is written to `profile.toml` until you accept it, field by field.

### If the model is not on this Mac

Pointing Perch at a remote endpoint means your résumé leaves your machine. That
needs saying yes to, for that feature, before anything is sent:

```bash
perch model endpoint https://api.example.com/v1 --allow-resume
```

Without it Perch refuses, and the refusal happens before any network call. The
check parses addresses rather than matching prefixes, so a host like
`127.0.0.1.evil.example` is what it actually is. An API key for a remote
endpoint lives in the system keychain; there is nowhere in `model.toml` for one.

## How it thinks about time

Perch has no opinion about how good a match is. There are no scores anywhere.
It has a precise opinion about how old something is, and says it in plain
language: *posted 6 hours ago*, *reposted 3 weeks ago*, *open 143 days*.

Age is the only hierarchy. Fresh roles read bright, tired ones read quiet, and
the ordering always agrees with the words, because both come from one decision
in `timesignal::signal`.

Everything Perch says about a posting's history is something Perch *observed*.
It records what it saw and when it first saw it, so it never claims to know
what a board was doing before you started watching.

## Files

| what | where |
|---|---|
| your profile | `~/.config/perch/profile.toml`: plain text, edit it by hand |
| your rules | `~/.config/perch/rules.toml`: plain text |
| model settings | `~/.config/perch/model.toml`: no secrets in it |
| the database | `~/.local/share/perch/perch.db`: one SQLite file |

Nothing is ever deleted. Dismissing a role, a posting coming down off a board,
and stopping watching a company are all dates rather than deletions. The rows
stay, which is why each is undoable and why board history survives them. Adding
a company back picks up exactly where it left off, dismissals included.

Perch also refuses to conclude things it did not observe. A board that will not
answer is reported as unreachable and left visibly stale; only a board that
actually replies with an empty listing can close a role.

## Layout

```
crates/core   boards, store, time signals, rules, profile
crates/cli    the command line
crates/llm    one OpenAI-compatible client, and span verification
crates/fill   fill plans per ATS: selector, value, provenance, and no submit
app/          the desktop app: Tauri 2, Vite, React
design/       the mockups the interface is built from
```

## The desktop app

```bash
cd app && npm install && npm run tauri dev
```

`npm run dev` alone opens the same interface in a browser against sample data,
which is useful for working on the design without rebuilding the Rust side. The
preview is dev-only and is dropped from a real build.

Two properties of the app are structural rather than matters of care. There is
no command that submits anything, because there is no such primitive in
`perch-core` for one to call. And a posting's text crosses into the webview as a
list of paragraphs, bullets and headings rather than as markup, so a job board
cannot put a script tag into an app that holds someone's profile.

Adding an ATS is one file in `crates/core/src/ats/` and one line in
`adapters()`. Whether Perch can *fill* that ATS's forms is declared separately
from whether it can *watch* them: Perch monitors broadly and fills narrowly.

## Licence

MIT.

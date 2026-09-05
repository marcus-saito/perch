# Perch

A local-first job search companion for macOS. Perch watches company job
boards, says plainly how old every posting is, and fills application forms
from a profile you wrote yourself. You review the form and you submit it.

No account, no backend, no telemetry. Everything stays on your machine, in
files you can open.

## What Perch refuses to do

**It never submits an application.** It fills a form from your profile and
then stops, with the form open and you in front of it. This is not a setting.
[`perch_fill::Action`](crates/fill/src/lib.rs) has three variants (set a value,
choose an option, attach a file) and none of them activates a control, so no
caller can ask for a click or a submit. Two tests read the JavaScript Perch
emits and assert it contains no `.submit(`, `requestSubmit`, `.click(`,
`fetch(` or `sendBeacon`.

**It never answers demographic questions.** EEO fields are refused by kind and,
independently, by the label the form uses, so a question no list anticipated
still cannot be answered on your behalf. There is nowhere in `profile.toml` to
store such an answer.

**It never takes a model's word for anything.** A model is used for one thing,
proposing profile fields from a résumé. Every proposal is checked back against
the document. A value the document states arrives with its line quoted. A
value the document only implies is offered, never pre-accepted. A value the
document does not contain is not offered at all. Nothing is written until you
accept it, field by field.

**It never sends your résumé anywhere without asking.** A remote model
endpoint means the résumé leaves your machine, and that needs a yes, for that
feature, before the document is read. The check parses the address rather than
matching a prefix, so `127.0.0.1.evil.example` is what it actually is.

## Install

Perch is built and tested on macOS. It is not signed or notarized and there is
no download, so you build it. A first build takes a few minutes.

You need a stable Rust toolchain, Node.js 22 or newer (20 also works), and
the Xcode command line tools.

The command line:

```bash
cargo install --path crates/cli
```

That puts `perch` on your path.

The desktop app:

```bash
cd app
npm install
npm run tauri build
```

The app is written to `target/release/bundle/macos/Perch.app`. Open it where
it is or move it to Applications.

The two share one set of files. A company you watch from the terminal is in
the app's feed, and the other way round.

## First run

```bash
perch watch add figma
perch sync
perch feed
```

`watch add` takes a company name, a board URL or a careers page, and works out
which board it is. `sync` reads every watched board. `feed` lists open roles,
newest first, each with one line saying why it is there.

```
  Figma
  Support Engineer, AI Infrastructure & Tooling  48f3258b
  San Francisco, CA • New York, NY • United States · reposted yesterday
  matched not-staff-plus: 'Engineer' in the title
```

The short reference at the end of the title line is how you name a role to the
other commands: `perch open 48f3258b`, `perch apply 48f3258b`.

With no rules and no profile, the feed shows every open role. Rules narrow it
and a profile lets Perch fill forms. Neither is needed to start.

`--home <dir>` keeps everything under a directory of your choosing, which is
useful for trying Perch without touching your real data.

## Your profile

`~/.config/perch/profile.toml` is a plain text file you own. Perch fills forms
from it and from nothing else.

```toml
name = "Dana Ferreira"
email = "dana@example.com"
phone = "+1 503 555 0100"
location = "Portland, OR"
work_authorisation = "US citizen"
skills = ["Rust", "distributed systems", "PostgreSQL"]

[links]
github = "https://github.com/dferreira"
website = "https://dferreira.dev"

[[experience]]
company = "Honeycomb"
title = "Senior Software Engineer"
dates = "2022 to present"

[[documents]]
name = "Résumé"
path = "/Users/dana/Documents/resume.pdf"
kind = "résumé"
```

Scalars and arrays go before the first `[table]`. An unknown key is an error,
which is what makes it true that there is nowhere to store a demographic
answer.

`perch profile import <file>` proposes these fields from a résumé, one at a
time, for you to accept, edit or skip. See "Reading a résumé" below.

## Applying

```bash
perch apply 48f3258b
```

The command line shows the plan: which boxes Perch would fill, with what, and
where each value came from; which document it would attach; and where it
stops. Then it opens the form in your browser.

The desktop app does the typing. It opens the form in its own window, types
the plan into the exact page you reviewed, once, and hands you the window with
a title that says you submit this yourself. The window may follow the board's
own redirects within the ATS and nowhere else. A closed posting that redirects
to a careers index, or a link clicked inside the filled window, gets nothing.

Three kinds of box are left empty on purpose, each saying so: ones Perch would
have to guess at (a start date), ones that are yours to write (a cover
letter), and demographic questions.

## The desktop app

Feed, role detail, applications, watchlist, profile and résumé import, on one
screen, driven from the keyboard. `j` and `k` move, `Enter` opens, `x` sets
aside and `u` brings back, `⌘K` opens a command palette whose words are the
CLI's words, verbatim.

There are no scores, no badges and no counts. Age is the only hierarchy: fresh
roles read bright, tired ones read quiet, and the ordering always agrees with
the words.

## Commands

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
| `perch open <ref>` | one role in full: its text, board history, hiring velocity |
| `perch apply <ref>` | review the fill plan, attach a document, open the form |
| `perch apply <ref> --resume <path>` | attach this document instead of the profile's résumé |
| `perch dismiss <ref>` | takes a role out of the feed |
| `perch restore <ref>` | puts it back |
| `perch rules edit` | opens the rules file, writing a commented starting point if there is none |
| `perch rules list` | the rules as Perch reads them, in the order they are tried |
| `perch rules test <ref>` | which rule fires on one role, and why |
| `perch apps` | applications: in flight, responded, archived |
| `perch apps mark <ref> <state>` | record where one stands, with `--note` |
| `perch profile show` | the profile as Perch reads it |
| `perch profile import <file>` | propose fields from a résumé, one at a time |
| `perch model list` | models this Mac can run, and what is configured |
| `perch model set <name>` | choose the model that reads résumés |
| `perch model endpoint <url>` | point at a remote server, with `--allow-resume` |
| `perch model key` | give Perch the key a remote endpoint wants, or `--forget` it |
| `perch model off` | run with no model at all |

Every command takes `--home <dir>`.

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

Nothing is scored or weighted. A rule fires or it does not, and every row says
which rule fired and what set it off. Matching is case-insensitive and works on
whole words, so a rule looking for `rust` fires on "Rust Engineer" but not on
"Trust & Safety". Several words in one entry match as a phrase.

Rules are read fresh on every command, so editing the file changes the next
`feed` with nothing to re-sync. A rules file that will not parse does not take
the feed away: `perch feed` says what went wrong, points at the line, and shows
every open role until the file reads as rules again.

Rules see the title, location and company that the board puts in its listing.
Descriptions are not read.

## Reading a résumé

Watching, matching, filling and tracking need no model. Running without one is
a complete way to use Perch, not a reduced one.

One OpenAI-compatible client covers Ollama, LM Studio, llama.cpp's server,
OpenRouter and Fireworks. Perch probes `localhost:11434` and offers whatever is
installed there.

```bash
perch model list
perch model set llama3.1
perch profile import ~/Documents/resume.pdf
```

Each proposed value is checked against the document and there are three
outcomes:

| | |
|---|---|
| the document says it | offered, and arrives accepted, with the line quoted |
| the document says something it was read *from* | offered, never pre-accepted, with the source shown |
| the document does not say it | not offered at all: there is no Accept to press |

Text that appears only inside something larger, such as `dferreira.dev` sitting
in `dana@dferreira.dev`, is treated as read rather than quoted.

Schema-constrained output makes replies easier to parse. It is not the
guarantee; verification is, and it holds whatever is on the other end of the
endpoint. A model that invents an employer wastes a moment of your time and
cannot do more than that.

### If the model is not on this Mac

```bash
perch model endpoint https://api.example.com/v1 --allow-resume
perch model key
```

Without `--allow-resume` Perch refuses, and the refusal happens before the
document is read. The API key is read from the terminal without echo and kept
in the system keychain. There is nowhere in `model.toml` for one, and the
desktop app offers the key control only when the endpoint is not this machine.

## How it thinks about time

Perch has no opinion about how good a match is. It has a precise opinion about
how old something is, and says it in plain language: *posted 6 hours ago*,
*reposted 3 weeks ago*, *open 143 days*.

Everything Perch says about a posting's history is something it observed. It
records what it saw and when it first saw it, so it never claims to know what a
board was doing before you started watching. A board that will not answer is
reported as unreachable and left visibly stale; only a board that replies with
an empty listing can close a role.

## Boards

| ATS | watch | fill |
|---|---|---|
| Greenhouse | yes | yes |
| Lever | yes | yes |
| Ashby | yes | yes |
| any page with schema.org `JobPosting` JSON-LD | yes | no, the role opens in your browser |

Whether Perch can fill a board is declared separately from whether it can
watch it. Perch watches broadly and fills narrowly.

## Files

| what | where |
|---|---|
| your profile | `~/.config/perch/profile.toml` |
| your rules | `~/.config/perch/rules.toml` |
| model settings | `~/.config/perch/model.toml`, with no secrets in it |
| the database | `~/.local/share/perch/perch.db`, one SQLite file |

Nothing is ever deleted. Dismissing a role, a posting coming down off a board,
and stopping watching a company are dates rather than deletions. The rows stay,
which is why each is undoable and why board history survives them.

## Development

```
crates/core   boards, store, time signals, rules, profile
crates/cli    the command line
crates/llm    one OpenAI-compatible client, and span verification
crates/fill   fill plans per ATS: selector, value, provenance, and no submit
app/          the desktop app: Tauri 2, Vite, React
design/       the mockups the interface is built from, and the design rules
```

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cd app && npx tsc --noEmit
```

The same four run on every pull request. `cd app && npm run tauri dev` runs the
app against the Rust side with live reload; `npm run dev` alone opens the
interface in a browser against sample data, for working on the design.

Adding an ATS is one file in `crates/core/src/ats/` and one line in
`adapters()`. A posting's text crosses into the webview as paragraphs, bullets
and headings rather than as markup, so a job board cannot put a script into an
app that holds someone's profile.

[`design/DESIGN.md`](design/DESIGN.md) holds the rules the interface and this
file are written to.

## Licence

MIT.

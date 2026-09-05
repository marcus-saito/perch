# Perch: design language for the mockups

Static HTML, Tailwind Play CDN for layout utilities, `perch.css` for the visual
language, `perch.js` for the keyboard layer. No build step. Open any file directly.

## The person this is for

Someone opening this on a bad Tuesday, four months in, after a rejection. Every
decision follows from that. The app is a quiet queue, not a dashboard. It never
congratulates, never gamifies, never counts. It tells the truth about time,
including the uncomfortable truth that a role has been open 143 days, and then
gets out of the way.

## Hard rules (a mockup violating any of these is wrong)

1. **No match scores.** Not a percentage, not stars, not "strong match".
2. **No badges. No count badges. No pills of any kind.** Nowhere in the app.
   Not on the rail, not on section headings, not next to a company name.
   `.tag` exists only for ATS names and file types: a bordered monospace label,
   never a number, never a status.
3. **No submit button anywhere.** The application flow ends at "Open in browser".
   There is no primitive in this app that submits a form.
4. **No EEO/demographic fields.** They do not appear even as skipped rows.
5. **Nothing an LLM produced is presented as fact.** It is a proposal in a diff,
   with the source text it anchors to, and it needs an explicit accept.
6. **Empty states read as fine.** No "0", no sad face, no "Get started!" energy.
   State what is true, say it's normal, name the one command that would change it.

## Tokens

Use CSS variables from `perch.css`. Never hardcode a hex value.

Surfaces `--paper` `--paper-raised` `--paper-sunk` `--rule` `--rule-strong`
Ink (4 steps) `--ink` `--ink-2` `--ink-3` `--ink-4`
Accent (one, terracotta) `--accent` `--accent-ink` `--accent-soft`
Mute caution `--caution` `--caution-soft`: used for *flagged, needs-you* only

Fonts: `--font-text` (Newsreader: role titles, pane titles, empty-state titles,
big numbers only), `--font-ui` (Inter: everything else), `--font-mono`
(JetBrains Mono: commands, rule names, selectors, provenance, timestamps in
board history).

Type scale: 11 · 12 · 13 · 15 · 17 · 21 · 27 · 34.

**Accent budget: at most three uses of `--accent` per screen.** Typically: the
fresh time signal, the one primary button, the selection bar. Spend it anywhere
else and cut something.

## Freshness: the only visual hierarchy in the feed

Warm and full-strength = new. Cool, light, and slightly transparent = old.
Expressed purely in type colour, weight, and opacity. Never a dot, bar, or chip.

| age | row class | time-signal class | reads as |
|---|---|---|---|
| < 24h | `row-fresh` | `t-fresh` | terracotta, medium weight, bright |
| 1–7d | `row-recent` | `t-recent` | full-ink title, grey signal |
| 7–30d | `row-settled` | `t-settled` | title steps back |
| 30–90d | `row-stale` | `t-stale` | quiet, 88% opacity |
| 90d+ | `row-tired` | `t-tired` | italic signal, 74% opacity, tired |

Time signals are plain language, always: "posted 6 hours ago", "posted
yesterday", "open 143 days", "reposted 3 weeks ago", "first seen in April".
Never a date alone, never "6h", never a relative-time badge.

## Page skeleton: copy this exactly

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Perch: VIEW NAME</title>
<script src="https://cdn.tailwindcss.com"></script>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;450;500;600&family=Newsreader:opsz,wght@6..72,400;6..72,500&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<link rel="stylesheet" href="perch.css">
</head>
<body>
<div class="app">
  <div data-rail></div>
  <div class="stage"> ... </div>
</div>
<div class="hint-bar"> ... </div>
<script src="perch.js"></script>
<script>perchBoot({ active: "Feed" });</script>
</body>
</html>
```

`perchBoot({active, queue:{onOpen, onSelect, onDismiss, selectFirst}})` wires
the rail, the ⌘K palette, and j/k/Enter/x over any `[data-queue]` containing
`[data-row]` elements. Every page gets a working palette and a hint bar.

## Component vocabulary in `perch.css`

`.app .rail .stage .pane .pane-head .pane-title .pane-sub .pane-body`
`.section-head`: uppercase 11px with a hairline rule trailing off
`.queue .row .row-company .row-title .row-meta .row-why .row-why .rule-name`
`.btn .btn-primary .btn-ghost .btn-sm`: one primary per screen, maximum
`.kbd .hint-bar .hint`
`.card .field .field-label .field-value .input .textarea .select`
`.textarea.by-design`: hatched and dashed, conspicuously empty *on purpose*
`.flagged`: the muted-gold "this one needs you" treatment
`.prov` / `.prov.from-profile`: provenance marker, 11px mono with a small dot
`.empty .empty-title .empty-body`
`.timeline .timeline-item .timeline-when .timeline-what`: board history
`.stat .stat .n`: hiring velocity
`.tag`: ATS name or file type only
`.palette-*`: supplied by perch.js, do not rebuild

## Keyboard: one contract, every screen

A key means the same category of thing everywhere. No screen overrides a
shared key, and no screen has to defend itself against one.

| key | means | where |
|---|---|---|
| `j` / `k` | move through the selection | every `[data-queue]` |
| `Enter` | open the selection | everywhere |
| `x` | set aside, never delete | queues marked `[data-clearable]` |
| `u` | undo the last `x` | anywhere `x` works |
| `Esc` | close the pane, sheet, or palette | anywhere one is open |
| `⌘K` | commands | everywhere, labelled exactly "commands" |

`x` is the load-bearing one. Perch deletes nothing, so `x` collapses the row in
place, offers `u` in the hint bar for six seconds, and lets each queue say where
the row went. The verb comes from the queue: `data-clearable="dismissed"` on the
feed, `"archived"` on Applications, `"skipped"` on Import, `"no longer watched"`
on the Watchlist. A queue holding nothing clearable (profile fields, documents)
simply omits the attribute, and `x` does nothing there.

Screen-specific verbs must not collide with the table above: Applications uses
`r` to record a reply, Import uses `a` accept and `e` edit, the apply sheet uses
`⌥←`/`⌥→` between steps and `⌘↵` for the step's primary action.

The hint bar shows only the keys that do something on that screen, in this
order: movement, open, screen-specific verbs, `x`, `Esc`, `⌘K`. Keys are
`<span class="kbd">j</span>`.

The palette's vocabulary is the CLI's vocabulary verbatim: `watch add`, `sync`,
`feed --fresh`, `apply`, `profile import`, `rules test`, `model set`, `model off`.
It is defined once in `perch.js`; do not redefine it per page.

## Voice

Plain, factual, unhurried. State what is true and stop.

**No em dashes.** Use a full stop, a comma, a colon, or brackets. Two short
sentences are almost always better than one long one joined by a dash.

**No embellishment.** Say the fact once. Do not restate it in a second clause,
do not justify it with a rhetorical aside, and do not personify Perch beyond
naming it as the thing that acted.

| instead of | write |
|---|---|
| "Perch has no idea when you could actually start. A plausible date typed here is worse than an empty box, so it stays empty and you answer it on the page." | "Perch does not know your start date. This is left blank." |
| "Perch does not write these. The whole point of them is that you did." | "Perch does not write cover letters." |
| "Nothing is deleted — adding it again picks up where this left off." | "Nothing is deleted. Adding it again restores what was here." |
| "Status: Active · Last sync: 11m" | "Ashby board. Checked 11 minutes ago." |
| "No results found." | "Nothing new since Tuesday. Boards post in bursts." |

Numbers belong in sentences, not as chrome. Counts under thirteen read as
words. Time is plain language: "posted 6 hours ago", "open 143 days".

Never an exclamation mark. Never "Congratulations", "Oops", "Error", "Failed",
"Sorry". Never an emoji. Never address the reader's emotional state.

Empty states, rejections, archived applications and unsupported boards get the
same ordinary treatment as anything else. Say what is true, say what would
change it, stop.

This applies to text a person reads in the app or the terminal. Text that comes
from a job board or a résumé is that source's own words and is never rewritten.

## Fixture data: use these companies across every view, consistently

The postings, descriptions and board histories in the mockups are invented for
the mockups. The company names are real and are used as names only. Nothing a
mockup shows is a real listing, and the README uses captures of real data
instead.

Sourcegraph (Greenhouse) · Oxide Computer (Lever) · Val Town (Ashby) ·
Fly.io (Ashby) · Warp (Greenhouse) · Tigris Data (Lever) · Modal (Ashby) ·
Ramp (Greenhouse) · Recurse Center (JSON-LD, no fill support) ·
Astral (Ashby) · Cursor (Greenhouse) · Zed Industries (Lever)

The user is a backend/systems engineer, Rust and distributed systems, looking
for remote or Bay Area, mid-to-senior, wanting a small company.

Her past employers are Cloudflare, Honeycomb and Datadog. They are deliberately
NOT drawn from the watched-company list above, because she cannot be applying to
a company she already works at. Keep those two sets disjoint.

Match rules that fire (name them in `.row-why`, mono, exactly these):
`rust-in-title` · `remote-ok` · `systems-keywords` · `small-team` ·
`watched-company` · `seniority-band` · `not-staff-plus` · `bay-area`

Example `.row-why` lines:
- "matched `rust-in-title`: 'Rust' in the title"
- "matched `systems-keywords`: description mentions distributed systems, storage"
- "matched `watched-company`: you added Oxide in March"
- "matched `remote-ok`: location says Remote (US)"

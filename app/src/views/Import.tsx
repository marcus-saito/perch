import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  type ImportProposal,
  type ImportRead,
  type ImportReady,
} from "../lib/api";
import { HintBar } from "../components/HintBar";
import { useQueue, type SetAside } from "../lib/keys";
import { count as word } from "../lib/words";

/**
 * Review import: what a model read out of a résumé, and what a person makes
 * of it one field at a time.
 *
 * Every row arrives having been checked back against the document by
 * `perch-llm`. A proposal that anchors nowhere is shown with its reason and no
 * Accept control at all, and neither the buttons nor the keyboard can put one
 * on it. A loose match arrives undecided, so accepting one is always a
 * person's own act.
 *
 * The consent verdict is on screen before a file is chosen, which is a
 * courtesy rather than a gate: `import_read` asks the same question again in
 * Rust, before the résumé is read off the disk.
 */

/** Where a row stands. `eye` is a loose match nobody has decided on yet. */
type Mark = "accepted" | "skipped" | "eye" | "open";

interface Row {
  /** The proposal's place in the read, as a string for the shared queue. */
  reference: string;
  proposal: ImportProposal;
  mark: Mark;
  /** The person's own words, when they typed over the proposed value. */
  edited: string | null;
}

/**
 * How a row starts.
 *
 * Only a quotation arrives accepted. A loose match waits for a person, and a
 * proposal Perch is not offering has nothing to accept.
 */
function firstMark(proposal: ImportProposal): Mark {
  if (!proposal.offerable) return "open";
  if (proposal.accepted) return "accepted";
  return proposal.note ? "eye" : "open";
}

/**
 * What a row shows.
 *
 * A row that writes more than one value reads "company · title · dates". A
 * part the person did not type keeps what it had, which is how `perch-llm`
 * reads the line back, so the row goes on showing every value it would write.
 */
function shown(row: Row): string {
  const { proposal, edited } = row;
  if (edited === null) return proposal.value;
  if (proposal.parts === null) return edited;
  const was = proposal.value.split("·").map((part) => part.trim());
  const typed = edited.split("·").map((part) => part.trim());
  const filled =
    typed.length >= was.length ? typed : typed.concat(was.slice(typed.length));
  return filled.filter((part) => part !== "").join(" · ");
}

const upper = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** A sentence, not a score: what will be written, and what will not. */
function tally(rows: Row[]): string {
  const count = (mark: Mark) => rows.filter((row) => row.mark === mark).length;
  const accepted = count("accepted");
  const skipped = count("skipped");
  const eye = count("eye");
  const held = rows.filter(
    (row) => !row.proposal.offerable && row.mark !== "skipped",
  ).length;
  const waiting = rows.length - accepted - skipped - eye - held;

  const head = accepted
    ? `${upper(word(accepted))} ${accepted === 1 ? "field to write." : "fields to write."}`
    : "Nothing to write yet.";
  const parts: string[] = [];
  if (skipped) parts.push(`${word(skipped)} skipped`);
  if (eye)
    parts.push(
      `${word(eye)} ${eye === 1 ? "needs your eye" : "need your eye"}`,
    );
  if (waiting) parts.push(`${word(waiting)} still waiting`);
  if (held) parts.push(`${word(held)} Perch is not offering`);
  return head + (parts.length ? ` ${upper(parts.join(", "))}.` : "");
}

/**
 * Where in the document the quote sits.
 *
 * A line number, and no more than that. The document Perch reads is flattened
 * text with no pages in it, so this does not name one.
 */
function reference(quote: string, line: number | null): string | null {
  if (line === null) return null;
  const lines = quote.split("\n").length;
  return lines > 1 ? `lines ${line}–${line + lines - 1}` : `line ${line}`;
}

/** One line of the quote, with the matched span wrapped in `.hl`. */
function marked(text: string, span: [number, number] | null): ReactNode {
  if (!span) return text;
  const from = Math.max(0, Math.min(span[0], text.length));
  const to = Math.max(from, Math.min(span[1], text.length));
  if (from === to) return text;
  return (
    <>
      {text.slice(0, from)}
      <span className="hl">{text.slice(from, to)}</span>
      {text.slice(to)}
    </>
  );
}

/**
 * The quoted line, or lines when the match crosses one.
 *
 * The span is counted in UTF-16 code units, which is what `slice` counts, and
 * it is moved line by line so a match spanning a break stays under the text it
 * was found in.
 */
function Anchor({ proposal }: { proposal: ImportProposal }) {
  const quote = proposal.quote;
  if (!quote) return null;
  const ref = reference(quote, proposal.line);
  let at = 0;
  return (
    <div className="anchor">
      {quote.split("\n").map((line, i) => {
        const span = proposal.highlight;
        const here: [number, number] | null = span
          ? [span[0] - at, span[1] - at]
          : null;
        at += line.length + 1;
        return <div key={i}>{marked(line, here)}</div>;
      })}
      {ref && <span className="ref">{ref}</span>}
    </div>
  );
}

/** Whatever came back, said plainly, without the machinery around it. */
function plainly(err: unknown): string {
  const text = err instanceof Error ? err.message : String(err);
  return text.replace(/^Error:\s*/i, "").trim() || "no reason came back";
}

/** What was written, in the sentence the command line uses for it. */
function wroteLine(written: number, path: string): string {
  if (written === 0) return `Nothing was written to ${path}.`;
  return `Wrote ${word(written)} ${written === 1 ? "field" : "fields"} to ${path}.`;
}

/**
 * What was not written, counted the way the footer counts it.
 *
 * "Skipped" is the x action, which the hint bar offers u to undo. A row nobody
 * decided on is not one of those, so it is counted and named separately rather
 * than being reported as an act the person did not perform.
 */
function heldBack(rows: Row[]): string | null {
  const count = (of: (row: Row) => boolean) => rows.filter(of).length;
  const skipped = count((row) => row.mark === "skipped");
  const undecided = count(
    (row) =>
      row.proposal.offerable &&
      row.mark !== "skipped" &&
      row.mark !== "accepted",
  );
  const refused = count(
    (row) => !row.proposal.offerable && row.mark !== "skipped",
  );
  const parts: string[] = [];
  if (skipped) parts.push(`${word(skipped)} skipped`);
  if (undecided) parts.push(`${word(undecided)} left undecided`);
  if (refused) parts.push(`${word(refused)} Perch is not offering`);
  return parts.length ? `${upper(parts.join(", "))}.` : null;
}

export function ImportView({ onWritten }: { onWritten: () => void }) {
  const [ready, setReady] = useState<ImportReady | null>(null);
  const [read, setRead] = useState<ImportRead | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [reading, setReading] = useState(false);
  const [writing, setWriting] = useState(false);
  const [written, setWritten] = useState<{
    sentence: string;
    aside: string | null;
  } | null>(null);
  const [trouble, setTrouble] = useState<string | null>(null);

  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  // Escape puts the value back. If the webview sends a blur on the way out,
  // the commit that would follow it is not the person's doing.
  const cancelling = useRef(false);

  // Reading a résumé and writing the profile are both round trips that can
  // outlive the view. Nothing below writes into a screen that has gone away.
  const onScreen = useRef(true);
  useEffect(() => {
    onScreen.current = true;
    return () => {
      onScreen.current = false;
    };
  }, []);

  useEffect(() => {
    api.importReady().then(
      (result) => {
        if (onScreen.current) setReady(result);
      },
      (err) => {
        if (onScreen.current)
          setTrouble(
            `Perch could not read its model settings. ${plainly(err)}`,
          );
      },
    );
  }, []);

  const reviewing = read !== null && rows.length > 0 && written === null;

  const setMark = useCallback((at: string, mark: Mark) => {
    setRows((rows) =>
      rows.map((row) => {
        if (row.reference !== at) return row;
        // The one rule this screen exists for. A proposal Perch is not
        // offering cannot be accepted from here, whatever asks.
        if (mark === "accepted" && !row.proposal.offerable) return row;
        return { ...row, mark };
      }),
    );
  }, []);

  const beginEdit = useCallback((row: Row) => {
    if (!row.proposal.offerable) return;
    cancelling.current = false;
    // The box opens on what the row shows, so a second edit starts from every
    // value the row would write rather than from the words typed last time.
    setDraft(shown(row));
    setEditing(row.reference);
  }, []);

  const commitEdit = useCallback(
    (at: string) => {
      if (cancelling.current) {
        cancelling.current = false;
        return;
      }
      const value = draft.trim();
      setRows((rows) =>
        rows.map((row) => {
          if (row.reference !== at || value === "") return row;
          // Typing the proposal back is not an edit. The value is still the
          // document's, so the screen goes on saying so.
          return {
            ...row,
            edited: value === row.proposal.value ? null : value,
          };
        }),
      );
      setEditing(null);
    },
    [draft],
  );

  const cancelEdit = useCallback(() => {
    cancelling.current = true;
    setEditing(null);
  }, []);

  // x sets a row aside as skipped and hands back the mark it had. The row
  // stays where it is, still legible, so you can see what you turned down.
  const setAside: SetAside = {
    verb: "skipped",
    run: async (at) => {
      const was = rows.find((row) => row.reference === at)?.mark ?? "open";
      setMark(at, "skipped");
      return async () => setMark(at, was);
    },
  };

  const { selected, setSelected, undoNote } = useQueue(rows, {
    setAside,
    enabled: reviewing && editing === null,
  });

  // a accepts, e edits. Neither reaches a row Perch is not offering.
  useEffect(() => {
    if (!reviewing || editing !== null) return;
    const onKey = (e: KeyboardEvent) => {
      const el = document.activeElement;
      if (
        el instanceof HTMLElement &&
        (el.isContentEditable || /INPUT|TEXTAREA|SELECT/.test(el.tagName))
      )
        return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key !== "a" && e.key !== "e") return;
      const row = rows.find((r) => r.reference === selected);
      if (!row || !row.proposal.offerable) return;
      e.preventDefault();
      if (e.key === "a") setMark(row.reference, "accepted");
      else beginEdit(row);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [reviewing, editing, rows, selected, setMark, beginEdit]);

  const choose = useCallback(async () => {
    setTrouble(null);
    let path: string | null = null;
    try {
      path = await open({
        title: "Choose a résumé",
        multiple: false,
        directory: false,
        filters: [{ name: "Résumé", extensions: ["pdf", "txt", "md"] }],
      });
    } catch (err) {
      if (onScreen.current)
        setTrouble(`The file picker did not open. ${plainly(err)}`);
      return;
    }
    if (path === null) return;

    setReading(true);
    try {
      const next = await api.importRead(path);
      if (!onScreen.current) return;
      setRead(next);
      setRows(
        next.proposals.map((proposal) => ({
          reference: String(proposal.id),
          proposal,
          mark: firstMark(proposal),
          edited: null,
        })),
      );
    } catch (err) {
      if (onScreen.current)
        setTrouble(`${plainly(err)} Nothing was written to your profile.`);
    } finally {
      if (onScreen.current) setReading(false);
    }
  }, []);

  const startOver = useCallback(() => {
    setRead(null);
    setRows([]);
    setWritten(null);
    setEditing(null);
    setTrouble(null);
  }, []);

  const accepted = useMemo(
    () => rows.filter((row) => row.mark === "accepted").length,
    [rows],
  );

  const write = useCallback(async () => {
    if (writing || accepted === 0) return;
    setWriting(true);
    setTrouble(null);
    try {
      const done = await api.importWrite(
        rows.map((row) => ({
          id: row.proposal.id,
          accepted: row.mark === "accepted",
          value: row.edited,
        })),
      );
      if (!onScreen.current) return;
      setWritten({
        sentence: wroteLine(done.written, done.profilePath),
        aside: heldBack(rows),
      });
      setRows([]);
      onWritten();
    } catch (err) {
      if (onScreen.current)
        setTrouble(`${plainly(err)} Your profile is as it was.`);
    } finally {
      if (onScreen.current) setWriting(false);
    }
  }, [writing, accepted, rows, onWritten]);

  const lede = reviewing ? (
    <>
      Read from <span className="mono">{read?.document}</span>. Nothing is saved
      to your profile until it is accepted here.
    </>
  ) : (
    <>
      Perch proposes fields from a résumé. Nothing is saved to your profile
      until it is accepted here.
    </>
  );

  return (
    <div className="import">
      <div className="stage">
        <section className="pane">
          <header className="pane-head">
            <div>
              <h1 className="pane-title">Review import</h1>
              <p className="pane-sub lede">{lede}</p>
            </div>
          </header>

          <div className="pane-body pad-top">
            <div style={column}>
              {trouble && (
                <div className="notice" style={rawWords}>
                  {trouble}
                </div>
              )}

              {/* ── Before a file is chosen ─────────────────── */}
              {!read && !written && (
                <Chooser
                  ready={ready}
                  reading={reading}
                  onChoose={() => void choose()}
                />
              )}

              {/* ── A read that anchored nothing ───────────── */}
              {read && rows.length === 0 && !written && (
                <>
                  <div className="prov">{read.provenance}</div>
                  <p className="lede" style={{ marginTop: 18 }}>
                    {read.nothingAnchored ??
                      `Nothing in ${read.document} anchored to text Perch could find in it.`}
                  </p>
                  <p
                    className="note"
                    style={{ marginTop: 10, maxWidth: "62ch" }}
                  >
                    Perch offers a field only when it can point at the line the
                    value came from. Your profile is untouched.
                  </p>
                  <button
                    type="button"
                    className="btn"
                    style={{ marginTop: 16 }}
                    onClick={startOver}
                  >
                    Choose another file
                  </button>
                </>
              )}

              {/* ── The proposals ──────────────────────────── */}
              {reviewing && read && (
                <>
                  <div className="prov">{read.provenance}</div>

                  <div className="section-head">Proposed fields</div>

                  <div className="queue">
                    {rows.map((row) => (
                      <Proposal
                        key={row.reference}
                        row={row}
                        selected={selected === row.reference}
                        editing={editing === row.reference}
                        draft={draft}
                        onSelect={() => setSelected(row.reference)}
                        onDraft={setDraft}
                        onAccept={() => setMark(row.reference, "accepted")}
                        onSkip={() => setMark(row.reference, "skipped")}
                        onEdit={() => beginEdit(row)}
                        onCommit={() => commitEdit(row.reference)}
                        onCancel={cancelEdit}
                      />
                    ))}
                  </div>

                  <p className="d-note" style={{ marginTop: 18 }}>
                    Anything left unaccepted stays out of the file. You can run{" "}
                    <span className="mono">profile import {read.document}</span>{" "}
                    again whenever you like.
                  </p>
                </>
              )}

              {/* ── After the write ────────────────────────── */}
              {written && (
                <>
                  <p className="lede" style={{ marginTop: 18, ...rawWords }}>
                    {written.sentence}
                  </p>
                  {written.aside && (
                    <p className="note" style={{ marginTop: 8 }}>
                      {written.aside}
                    </p>
                  )}
                  <p
                    className="note"
                    style={{ marginTop: 10, maxWidth: "62ch" }}
                  >
                    The file is plain text and yours to edit. Perch reads it
                    back each time it runs.
                  </p>
                  <button
                    type="button"
                    className="btn"
                    style={{ marginTop: 16 }}
                    onClick={startOver}
                  >
                    Read another file
                  </button>
                </>
              )}
            </div>
          </div>
        </section>
      </div>

      {reviewing && read && (
        <div className="footbar">
          <div style={footRow}>
            <div style={{ minWidth: 0 }}>
              <div className="foot-line">{tally(rows)}</div>
              <div className="foot-path mono" style={rawWords}>
                writes to {read.profilePath}
              </div>
            </div>
            <div style={footActs}>
              <button
                type="button"
                className="btn btn-ghost"
                onClick={startOver}
              >
                Discard
              </button>
              <button
                type="button"
                className="btn btn-primary"
                disabled={accepted === 0 || writing}
                onClick={() => void write()}
              >
                {writing ? "Writing" : "Write to profile.toml"}
              </button>
            </div>
          </div>
        </div>
      )}

      <HintBar
        hints={
          reviewing
            ? [
                { keys: ["j", "k"], label: "move" },
                { keys: ["a"], label: "accept" },
                { keys: ["e"], label: "edit" },
                { keys: ["x"], label: "skip" },
              ]
            : []
        }
        undoNote={reviewing ? undoNote : null}
      />
    </div>
  );
}

/**
 * What happens if a file is chosen, said before one is.
 *
 * A model that is not configured, and an endpoint a résumé may not be sent to,
 * both stop here: there is no picker to press, so nothing is read.
 */
function Chooser({
  ready,
  reading,
  onChoose,
}: {
  ready: ImportReady | null;
  reading: boolean;
  onChoose: () => void;
}) {
  if (reading) {
    return (
      <p className="lede" style={{ marginTop: 18 }}>
        Reading the résumé.
      </p>
    );
  }
  if (!ready) {
    return (
      <p className="note" style={{ marginTop: 18 }}>
        Reading what a model would do with a résumé.
      </p>
    );
  }

  const stopped = !ready.configured || ready.verdict === "refused";
  if (stopped) {
    return (
      <div className="notice" style={{ marginTop: 14 }}>
        <div>{ready.sentence}</div>
        {ready.remedy && (
          <div style={{ color: "var(--ink-3)", marginTop: 6 }}>
            {ready.remedy}
          </div>
        )}
        <div style={{ color: "var(--ink-3)", marginTop: 6 }}>
          No file is asked for until that changes.
        </div>
      </div>
    );
  }

  return (
    <>
      <p className="lede" style={{ marginTop: 18, maxWidth: "62ch" }}>
        {ready.sentence}
      </p>
      <div style={{ marginTop: 18 }}>
        <button type="button" className="btn btn-primary" onClick={onChoose}>
          Choose a résumé
        </button>
      </div>
      <p className="note" style={{ marginTop: 14, maxWidth: "62ch" }}>
        A .pdf, .txt or .md file. Perch reads it into proposed fields and shows
        you the line each one came from.
      </p>
      <p className="note" style={{ marginTop: 8, ...rawWords }}>
        Accepted fields are written to{" "}
        <span className="mono">{ready.profilePath}</span>.
      </p>
    </>
  );
}

/** One proposal: the diff, the evidence, and what may be done about it. */
function Proposal({
  row,
  selected,
  editing,
  draft,
  onSelect,
  onDraft,
  onAccept,
  onSkip,
  onEdit,
  onCommit,
  onCancel,
}: {
  row: Row;
  selected: boolean;
  editing: boolean;
  draft: string;
  onSelect: () => void;
  onDraft: (value: string) => void;
  onAccept: () => void;
  onSkip: () => void;
  onEdit: () => void;
  onCommit: () => void;
  onCancel: () => void;
}) {
  const { proposal } = row;
  const value = shown(row);
  return (
    <article
      data-row={row.reference}
      data-state={row.mark}
      className={`row${selected ? " is-selected" : ""}`}
      onClick={onSelect}
    >
      <div style={rowGrid}>
        <div style={{ minWidth: 0 }}>
          <div className="field-label">{proposal.label}</div>

          <div className="d-was">
            in profile{" "}
            {proposal.current === null ? (
              "not set"
            ) : row.mark === "accepted" ? (
              <span className="old">{proposal.current}</span>
            ) : (
              proposal.current
            )}
          </div>

          <div className={proposal.offerable ? "d-now" : "d-now d-void"}>
            {editing ? (
              <input
                className="input"
                autoFocus
                autoComplete="off"
                aria-label={`${proposal.label}, in your own words`}
                value={draft}
                onFocus={(e) => e.currentTarget.select()}
                onChange={(e) => onDraft(e.target.value)}
                onClick={(e) => e.stopPropagation()}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    onCommit();
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    onCancel();
                  }
                }}
                onBlur={onCommit}
              />
            ) : (
              value
            )}
          </div>

          {/* A row that writes three values says so while it is being typed
              into, since editing one of them is editing all three. */}
          {editing && proposal.parts && (
            <div className="d-note">
              This row writes {proposal.parts}. A part you leave out keeps what
              it has.
            </div>
          )}

          {/* An edited value is the person's, so the screen stops calling it
              a quotation. The anchor below it is still the document's line. */}
          {row.edited !== null && (
            <div className="d-note">
              This value is yours. The line below is what Perch read in the
              document.
            </div>
          )}

          <Anchor proposal={proposal} />

          {proposal.refusal && (
            <div className="anchor flagged">{proposal.refusal}</div>
          )}
          {proposal.note && (
            <div className="anchor flagged">{proposal.note}</div>
          )}
        </div>

        <div className="d-act" style={actRow}>
          <span className="d-mark d-mark-accepted">accepted</span>
          <span className="d-mark d-mark-skipped">skipped</span>
          {/* Accept and Edit belong to a value the document supports. Editing
              is not a way around that, so a row Perch is not offering has
              neither of them. */}
          {proposal.offerable && (
            <>
              <button
                type="button"
                className="btn btn-ghost btn-sm d-accept"
                onClick={onAccept}
              >
                Accept
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={onEdit}
              >
                Edit
              </button>
            </>
          )}
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={onSkip}
          >
            Skip
          </button>
        </div>
      </div>
    </article>
  );
}

const column: CSSProperties = {
  width: "100%",
  maxWidth: 780,
  margin: "0 auto",
};

const rowGrid: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "1fr auto",
  gap: 24,
  alignItems: "start",
};

const actRow: CSSProperties = { display: "flex", alignItems: "center", gap: 8 };

const footRow: CSSProperties = {
  ...column,
  display: "flex",
  alignItems: "center",
  gap: 24,
};

const footActs: CSSProperties = {
  marginLeft: "auto",
  display: "flex",
  alignItems: "center",
  gap: 8,
};

/** A path or a quoted line is one unbreakable word. It wraps rather than
    pushing the pane sideways. */
const rawWords: CSSProperties = { overflowWrap: "anywhere" };

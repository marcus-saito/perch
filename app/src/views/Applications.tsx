import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { Empty } from "../components/Empty";
import { HintBar } from "../components/HintBar";
import { api, type Application } from "../lib/api";
import { useQueue, type SetAside } from "../lib/keys";

/** The three groups, in the order a person reads them. Headings come from the
    backend; Perch only decides where each group sits. */
const ORDER = ["in_flight", "responded", "archived"] as const;

const column: CSSProperties = { width: "100%", maxWidth: 620 };
const headRow: CSSProperties = {
  ...column,
  display: "flex",
  alignItems: "flex-end",
  justifyContent: "space-between",
  gap: 24,
};
const quietLine: CSSProperties = {
  fontSize: "var(--t-13)",
  color: "var(--ink-3)",
  padding: "26px 0",
};
const archivedNote: CSSProperties = {
  fontSize: "var(--t-12)",
  color: "var(--ink-3)",
  lineHeight: 1.65,
  maxWidth: "56ch",
  margin: "0 0 16px",
  padding: "2px 0 0 18px",
};
const composerBox: CSSProperties = {
  marginTop: 10,
  paddingTop: 10,
  borderTop: "1px solid var(--rule)",
  maxWidth: "56ch",
};
const composerRow: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  minWidth: 0,
};
const composerField: CSSProperties = { flex: 1, minWidth: 0 };
const composerAside: CSSProperties = {
  fontSize: "var(--t-11)",
  color: "var(--ink-3)",
  marginTop: 7,
};

/** Whatever came back, said plainly, with the machinery left off the end. */
function plainly(lead: string, err: unknown): string {
  const text = (err instanceof Error ? err.message : String(err))
    .replace(/^Error:\s*/i, "")
    .trim();
  return text ? `${lead} ${text}` : lead;
}

/** The last thing that happened, in plain words. */
function lastThing(a: Application): string {
  const note = a.note?.trim();
  if (note) return note;
  if (a.archivedQuietly) return "no reply in 30 days";
  if (a.state === "archived") return "archived by hand";
  if (a.state === "responded") return "a reply came back";
  return "nothing since";
}

export function ApplicationsView({ onChanged }: { onChanged: () => void }) {
  const [rows, setRows] = useState<Application[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [composerFor, setComposerFor] = useState<string | null>(null);
  const [noteText, setNoteText] = useState("");
  const [recording, setRecording] = useState(false);
  // `x` on a row that is already archived changes nothing, so the hint bar
  // should not offer an undo for it.
  const [archiveDidNothing, setArchiveDidNothing] = useState(false);

  // Reading the list and marking a row are both round trips that can outlive
  // the view. Nothing below writes into a screen that has already gone away.
  const onScreen = useRef(true);
  useEffect(() => {
    onScreen.current = true;
    return () => {
      onScreen.current = false;
    };
  }, []);

  const load = useCallback(async () => {
    try {
      const applications = await api.applications();
      if (!onScreen.current) return;
      setRows(applications);
      setProblem(null);
    } catch (err) {
      if (onScreen.current) {
        setProblem(
          plainly("Perch could not read your applications just now.", err),
        );
      }
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const groups = useMemo(() => {
    const all = rows ?? [];
    return ORDER.map((state) => all.filter((a) => a.state === state)).filter(
      (g) => g.length > 0,
    );
  }, [rows]);

  // j/k walks the page in the order it is read, archived rows included.
  const ordered = useMemo(() => groups.flat(), [groups]);

  const openComposer = useCallback((reference: string) => {
    setNoteText("");
    setProblem(null);
    setComposerFor(reference);
  }, []);

  // `x` never destroys, and `u` has to land on something real. The last archive
  // that actually happened is kept here so a row with nothing to set aside can
  // stand out of the way and hand that one back instead.
  const lastUndo = useRef<(() => Promise<void>) | null>(null);

  const setAside: SetAside = {
    verb: "archived",
    run: async (reference) => {
      const standAside = async () => {
        const previousUndo = lastUndo.current;
        lastUndo.current = null;
        await previousUndo?.();
      };

      const row = ordered.find((a) => a.reference === reference);
      // Archived rows stay reachable with j/k, but there is nothing left to
      // archive about them.
      if (!row || row.state === "archived") {
        setArchiveDidNothing(true);
        return standAside;
      }

      const previous = row.state;
      try {
        await api.markApplication(reference, "archived");
      } catch (err) {
        if (onScreen.current) {
          setProblem(plainly("That one stayed where it was.", err));
          setArchiveDidNothing(true);
        }
        return standAside;
      }
      if (onScreen.current) {
        setArchiveDidNothing(false);
        setProblem(null);
      }

      let spent = false;
      const undo: () => Promise<void> = async () => {
        if (spent) return;
        spent = true;
        if (lastUndo.current === undo) lastUndo.current = null;
        try {
          await api.markApplication(reference, previous);
          if (onScreen.current) setProblem(null);
        } catch (err) {
          if (onScreen.current)
            setProblem(plainly("That one stayed in the archive.", err));
        }
      };
      lastUndo.current = undo;
      return undo;
    },
  };

  const { selected, setSelected, undoNote } = useQueue(ordered, {
    onOpen: (row) => openComposer(row.reference),
    setAside,
    onChanged: load,
    enabled: composerFor === null,
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (composerFor !== null) {
          e.preventDefault();
          setComposerFor(null);
        }
        return;
      }
      // While the composer is open it holds the keyboard; `r` would only
      // reopen it and throw away what has been typed.
      if (composerFor !== null) return;
      const el = document.activeElement;
      if (
        el instanceof HTMLElement &&
        (el.isContentEditable || /INPUT|TEXTAREA|SELECT/.test(el.tagName))
      )
        return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "r" && selected) {
        e.preventDefault();
        openComposer(selected);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [composerFor, selected, openComposer]);

  const record = async (reference: string, state: "responded" | "archived") => {
    setRecording(true);
    setProblem(null);
    try {
      await api.markApplication(reference, state, noteText.trim() || undefined);
      if (!onScreen.current) return;
      setComposerFor(null);
      setNoteText("");
      await load();
      onChanged();
    } catch (err) {
      if (onScreen.current)
        setProblem(plainly("That reply is not recorded yet.", err));
    } finally {
      if (onScreen.current) setRecording(false);
    }
  };

  return (
    <>
      <div className="stage">
        <section className="pane">
          <header className="pane-head">
            <div style={headRow}>
              <div>
                <h1 className="pane-title">Applications</h1>
                <div className="pane-sub">
                  What you have sent, and what has come back so far.
                </div>
              </div>
              {ordered.length > 0 && (
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => {
                    if (selected) openComposer(selected);
                  }}
                >
                  Record a reply <span className="kbd">r</span>
                </button>
              )}
            </div>
          </header>

          <div className="pane-body">
            <div style={column}>
              {problem && (
                <div className="notice" style={{ overflowWrap: "anywhere" }}>
                  {problem}
                </div>
              )}

              {rows === null && !problem && (
                <div style={quietLine}>reading what you have sent…</div>
              )}

              {rows !== null && rows.length === 0 && (
                <Empty title="Nothing sent yet">
                  Perch records an application once you have sent it; it takes
                  no part in the sending itself. Apply on a company's own board,
                  then run <code>apps mark &lt;ref&gt; in-flight</code> and the
                  role turns up here.
                </Empty>
              )}

              {groups.map((items, gi) => {
                const state = items[0].state;
                const lastGroup = gi === groups.length - 1;
                return (
                  <Fragment key={state}>
                    <div className="section-head">{items[0].heading}</div>

                    {state === "archived" && (
                      <p style={archivedNote}>
                        After 30 days without a reply Perch moves an application
                        here on its own; you can also archive one yourself with{" "}
                        <span className="kbd">x</span>, and nothing is deleted.
                        Recording a reply moves it back up.
                      </p>
                    )}

                    <div className="queue">
                      {items.map((a, i) => (
                        <article
                          key={a.reference}
                          data-row={a.reference}
                          className={`row row-${a.freshness}${selected === a.reference ? " is-selected" : ""}`}
                          style={
                            lastGroup && i === items.length - 1
                              ? { borderBottomColor: "transparent" }
                              : undefined
                          }
                          onClick={() => {
                            setSelected(a.reference);
                            setComposerFor(null);
                          }}
                        >
                          <div className="row-company">{a.company}</div>
                          <div className="row-title">{a.title}</div>
                          <div className="row-meta">
                            <span className={`t-${a.freshness}`}>
                              {a.applied}
                            </span>
                            <span className="sep">·</span>
                            <span>{lastThing(a)}</span>
                          </div>

                          {composerFor === a.reference && (
                            <div
                              style={composerBox}
                              onClick={(e) => e.stopPropagation()}
                            >
                              <div style={composerRow}>
                                <input
                                  className="input"
                                  style={composerField}
                                  autoFocus
                                  autoComplete="off"
                                  aria-label="what came back"
                                  value={noteText}
                                  placeholder="what came back, in a few words"
                                  onChange={(e) => setNoteText(e.target.value)}
                                />
                                <button
                                  type="button"
                                  className="btn btn-sm"
                                  style={{ flex: "none" }}
                                  disabled={recording}
                                  onClick={() =>
                                    void record(a.reference, "responded")
                                  }
                                >
                                  Responded
                                </button>
                                <button
                                  type="button"
                                  className="btn btn-sm"
                                  style={{ flex: "none" }}
                                  disabled={recording}
                                  onClick={() =>
                                    void record(a.reference, "archived")
                                  }
                                >
                                  Archived
                                </button>
                              </div>
                              <div style={composerAside}>
                                Esc closes this. Nothing is recorded until you
                                choose one.
                              </div>
                            </div>
                          )}
                        </article>
                      ))}
                    </div>
                  </Fragment>
                );
              })}
            </div>
          </div>
        </section>
      </div>

      <HintBar
        hints={[
          { keys: ["j", "k"], label: "move" },
          { keys: ["↵", "r"], label: "record a reply" },
          { keys: ["x"], label: "archive" },
          { keys: ["Esc"], label: "close" },
        ]}
        undoNote={archiveDidNothing ? null : undoNote}
      />
    </>
  );
}

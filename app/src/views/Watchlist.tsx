import { useCallback, useEffect, useRef, useState } from "react";
import { Empty } from "../components/Empty";
import { HintBar } from "../components/HintBar";
import { api, type Board } from "../lib/api";
import { useQueue } from "../lib/keys";

/**
 * The companies Perch reads, and nothing else. A row here is a board: what it
 * runs on, where it lives, when it was last read, and whether Perch can fill
 * its forms or has to hand the browser over.
 *
 * `x` stops watching. Unwatching is a date rather than a deletion, so the undo
 * is simply watching again. The backend resumes with every role and dismissal
 * intact. The undo hands back the board's own URL rather than its token, so the
 * adapter that gets asked again is the one this row came from.
 */
type Row = Board & { reference: string };

/** Whatever came back, said plainly, without the machinery around it. */
function plainly(err: unknown): string {
  const text = err instanceof Error ? err.message : String(err);
  return (
    text.replace(/^Error:\s*/i, "").trim() || "that board could not be read"
  );
}

/** The board as a person would say it aloud: no scheme, no trailing slash. */
function bare(url: string): string {
  return url.replace(/^https?:\/\//i, "").replace(/\/+$/, "");
}

export function WatchlistView({ onChanged }: { onChanged: () => void }) {
  const [rows, setRows] = useState<Row[]>([]);
  const [loading, setLoading] = useState(true);
  const [trouble, setTrouble] = useState<string | null>(null);

  const [draft, setDraft] = useState("");
  const [looking, setLooking] = useState<string | null>(null);
  const [found, setFound] = useState<string | null>(null);
  const [addTrouble, setAddTrouble] = useState<string | null>(null);

  // Reading a board, adding one and putting one back are all round trips that
  // can outlive the view. Nothing below writes state into a screen that has
  // already gone away.
  const onScreen = useRef(true);
  useEffect(() => {
    onScreen.current = true;
    return () => {
      onScreen.current = false;
    };
  }, []);

  const load = useCallback(async () => {
    try {
      const boards = await api.watchlist();
      if (!onScreen.current) return;
      setRows(boards.map((board) => ({ ...board, reference: board.key })));
      setTrouble(null);
    } catch (err) {
      if (onScreen.current) setTrouble(plainly(err));
    } finally {
      if (onScreen.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const add = useCallback(
    async (input: string) => {
      const value = input.trim();
      if (!value || looking) return;
      setLooking(value);
      setAddTrouble(null);
      setFound(null);
      try {
        const company = await api.watchAdd(value);
        if (!onScreen.current) return;
        setDraft("");
        setFound(company);
        await load();
        onChanged();
      } catch (err) {
        if (onScreen.current) setAddTrouble(plainly(err));
      } finally {
        if (onScreen.current) setLooking(null);
      }
    },
    [looking, load, onChanged],
  );

  const open = useCallback(async (row: Row) => {
    try {
      await api.openInBrowser(row.url);
    } catch (err) {
      if (onScreen.current) setTrouble(plainly(err));
    }
  }, []);

  const { selected, setSelected, undoNote } = useQueue<Row>(rows, {
    onOpen: (row) => void open(row),
    setAside: {
      verb: "no longer watched",
      run: async (key) => {
        // Hold on to the board's own URL before the list reloads without it:
        // a bare token could be read as a different company on another ATS,
        // and `u` has to put back exactly the board that went away.
        const url = rows.find((row) => row.reference === key)?.url ?? key;
        try {
          await api.watchRemove(key);
          if (onScreen.current) setTrouble(null);
          await load();
          onChanged();
        } catch (err) {
          if (onScreen.current) setTrouble(plainly(err));
          // Nothing was set aside, so there is nothing to put back.
          return async () => {};
        }
        return async () => {
          try {
            await api.watchAdd(url);
            if (onScreen.current) setTrouble(null);
            await load();
            onChanged();
          } catch (err) {
            if (onScreen.current) setTrouble(plainly(err));
          }
        };
      },
    },
  });

  return (
    <div className="stage">
      <section className="pane">
        <header className="pane-head" style={{ maxWidth: 724 }}>
          <div>
            <h1 className="pane-title">Watchlist</h1>
            <p className="pane-sub">
              Perch reads each of these boards about every half hour while this
              Mac is awake, and backs off to once a day for the ones that have
              gone quiet.
            </p>
          </div>
        </header>

        <div className="pane-body pad-top">
          <div style={{ maxWidth: 660 }}>
            <div style={{ padding: "8px 0 4px" }}>
              <input
                className="input"
                type="text"
                autoComplete="off"
                spellCheck={false}
                aria-label="add a company"
                placeholder="add a company: name or careers URL"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void add(draft);
                  }
                }}
              />
              <div
                className="mono muted"
                style={{ fontSize: "var(--t-11)", marginTop: 8 }}
              >
                watch add &lt;company&gt;
              </div>
              {found && (
                <div
                  className="muted"
                  style={{
                    fontSize: "var(--t-12)",
                    marginTop: 10,
                    lineHeight: 1.6,
                  }}
                >
                  Now watching {found}. Anything already open there arrives in
                  the feed at its real age, not as new.
                </div>
              )}
            </div>

            {addTrouble && (
              <div className="notice" style={{ marginTop: 14 }}>
                {addTrouble}
                <div style={{ color: "var(--ink-3)", marginTop: 6 }}>
                  Some companies post to a page with no board behind it. Perch
                  has nothing to read there, so it stays off the list.
                </div>
              </div>
            )}

            {trouble && (
              <div className="notice" style={{ marginTop: 14 }}>
                {trouble}
              </div>
            )}

            {loading && (
              <div
                style={{
                  color: "var(--ink-3)",
                  fontSize: "var(--t-13)",
                  padding: "26px 0 0",
                }}
              >
                reading the watchlist…
              </div>
            )}

            {!loading && rows.length === 0 && !looking && (
              <Empty title="Nothing watched yet">
                Perch only reads boards you name, so the list starts empty. Put
                a company in the field above (a name, or the careers URL if you
                have it) or run <code>watch add &lt;company&gt;</code> in the
                terminal.
              </Empty>
            )}

            <div className="queue" style={{ marginTop: 24 }}>
              {looking && (
                <>
                  <div className="section-head">Just added</div>
                  <div className="row" aria-live="polite">
                    <div className="row-title">{looking}</div>
                    <div className="row-meta">
                      <span className="spin" aria-hidden="true">
                        ◜
                      </span>
                      <span>looking for a board at {looking}…</span>
                    </div>
                  </div>
                </>
              )}

              {rows.length > 0 && <div className="section-head">Watching</div>}

              {rows.map((row) => (
                <div
                  key={row.reference}
                  data-row={row.reference}
                  className={`row ${selected === row.reference ? "is-selected" : ""}`}
                  onClick={() => setSelected(row.reference)}
                  onDoubleClick={() => void open(row)}
                >
                  <div className="row-title">{row.company}</div>
                  <div className="row-meta">
                    <span className="tag">{row.ats}</span>
                    <span
                      className="mono muted"
                      style={{ overflowWrap: "anywhere" }}
                    >
                      {bare(row.url)}
                    </span>
                    <span className="sep">·</span>
                    <span>{row.checked}</span>
                  </div>
                  <div className="row-why">
                    <span>
                      {row.history}{" "}
                      {row.fillSupported
                        ? "Forms fill from your profile here."
                        : "This one opens in the browser."}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>

      <HintBar
        hints={[
          { keys: ["j", "k"], label: "move" },
          { keys: ["↵"], label: "open" },
          { keys: ["x"], label: "stop watching" },
        ]}
        undoNote={undoNote}
      />
    </div>
  );
}

import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { CSSProperties, ReactNode } from "react";
import type { FeedOptions } from "../App";
import {
  api,
  type Block,
  type Detail,
  type Feed,
  type RoleRow,
} from "../lib/api";
import { useQueue, type SetAside } from "../lib/keys";
import { Empty } from "../components/Empty";
import { HintBar, type Hint } from "../components/HintBar";

/**
 * The feed and the posting, in one view.
 *
 * Opening a role narrows the feed; it never navigates away from it. The queue
 * you were reading stays under your hand, in the same order, with the same
 * selection. The detail is a second column, not a second page.
 */
export function FeedView({
  options,
  onOptions,
  onSync,
  syncing,
  onApply,
  applyOpen,
}: {
  options: FeedOptions;
  onOptions: (o: FeedOptions) => void;
  onSync: () => void;
  syncing: boolean;
  /** Start an application: the sheet opens over this feed, dimmed. */
  onApply: (reference: string) => void;
  /** While the sheet has the screen, this view's keys and hints stand down. */
  applyOpen: boolean;
  /**
   * Deliberately not called from here. The parent answers it by remounting
   * this view, which would take the pending `u` down with it the instant the
   * hint bar offered it. `x` reloads the feed in place instead, and the other
   * views read the store fresh when you switch to them. That is the only
   * moment a dismissal could show up anywhere else.
   */
  onChanged: () => void;
}) {
  const [feed, setFeed] = useState<Feed | null>(null);
  const [feedTrouble, setFeedTrouble] = useState<string | null>(null);
  const [openRef, setOpenRef] = useState<string | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [detailTrouble, setDetailTrouble] = useState<string | null>(null);
  const [rowTrouble, setRowTrouble] = useState<string | null>(null);

  // Reading the feed, setting a role aside and putting it back are all round
  // trips that can outlive the view. Nothing below writes into a screen that
  // has already gone away.
  const onScreen = useRef(true);
  useEffect(() => {
    onScreen.current = true;
    return () => {
      onScreen.current = false;
    };
  }, []);

  // Reloading in place rather than remounting: `x` has to leave the row's undo
  // standing for the six seconds the hint bar promises it.
  const load = useCallback(async () => {
    try {
      const next = await api.feed(options);
      if (!onScreen.current) return;
      setFeed(next);
      setFeedTrouble(null);
    } catch (err) {
      if (onScreen.current) setFeedTrouble(plainly(err));
    }
  }, [options]);

  useEffect(() => {
    setFeed(null);
    setFeedTrouble(null);
    setRowTrouble(null);
    setOpenRef(null);
    void load();
  }, [load]);

  useEffect(() => {
    if (!openRef) {
      setDetail(null);
      setDetailTrouble(null);
      return;
    }
    let live = true;
    setDetail(null);
    setDetailTrouble(null);
    api
      .detail(openRef)
      .then((next) => live && setDetail(next))
      .catch((err) => live && setDetailTrouble(plainly(err)));
    return () => {
      live = false;
    };
  }, [openRef]);

  const rows = useMemo<RoleRow[]>(() => feed?.roles ?? [], [feed]);

  // The backend has already put these in the order a person should read them.
  // Group under the bucket each row carries; never sort.
  const groups = useMemo(() => {
    const out: { bucket: string; rows: RoleRow[] }[] = [];
    for (const row of rows) {
      const last = out[out.length - 1];
      if (last && last.bucket === row.bucket) last.rows.push(row);
      else out.push({ bucket: row.bucket, rows: [row] });
    }
    return out;
  }, [rows]);

  // Set aside, the row under it takes the selection. Putting it back takes the
  // selection with it, so you keep your place in the queue either way.
  const selectRow = useRef<((reference: string | null) => void) | null>(null);

  // x sets a row aside and hands back the way to put it back. Nothing is deleted.
  const setAside: SetAside = {
    verb: "dismissed",
    run: async (reference) => {
      const at = rows.findIndex((r) => r.reference === reference);
      const next = rows[at + 1] ?? rows[at - 1] ?? null;
      try {
        await api.dismiss(reference);
        setRowTrouble(null);
      } catch (err) {
        setRowTrouble(`That role is still in the feed. ${plainly(err)}`);
        return async () => {};
      }
      if (reference === openRef) setOpenRef(null);
      selectRow.current?.(next ? next.reference : null);
      return async () => {
        try {
          await api.restore(reference);
          setRowTrouble(null);
          selectRow.current?.(reference);
        } catch (err) {
          setRowTrouble(`That role stayed set aside. ${plainly(err)}`);
        }
      };
    },
  };

  const { selected, setSelected, undoNote } = useQueue(rows, {
    onOpen: (row) => setOpenRef(row.reference),
    setAside,
    onChanged: load,
    enabled: !applyOpen,
  });

  useEffect(() => {
    selectRow.current = setSelected;
  }, [setSelected]);

  // The Dismiss button is the x key: one behaviour, defined once in keys.ts.
  // The pane can be showing a role that j/k has since moved off, so the row is
  // put back under the selection first and the key follows on the next render,
  // by which time the queue is listening for it against the right row.
  const [asideWanted, setAsideWanted] = useState<string | null>(null);
  const askToSetAside = useCallback(
    (reference: string) => {
      setSelected(reference);
      setAsideWanted(reference);
    },
    [setSelected],
  );
  useEffect(() => {
    if (asideWanted === null) return;
    if (!rows.some((r) => r.reference === asideWanted)) {
      setAsideWanted(null);
      return;
    }
    if (selected !== asideWanted) return;
    setAsideWanted(null);
    (document.activeElement as HTMLElement | null)?.blur();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "x" }));
  }, [asideWanted, selected, rows]);

  // Esc narrows the stage back to one column. The palette's Esc wins first,
  // and so does the apply sheet's: Esc closes whatever is in front.
  useEffect(() => {
    if (!openRef || applyOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (document.querySelector(".palette-scrim.is-open")) return;
      e.preventDefault();
      setOpenRef(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openRef, applyOpen]);

  // s is the button's key, and sync is the CLI's word for the same thing.
  useEffect(() => {
    if (applyOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "s" || e.metaKey || e.ctrlKey || e.altKey) return;
      const el = document.activeElement;
      if (
        el instanceof HTMLElement &&
        (el.isContentEditable || /INPUT|TEXTAREA|SELECT/.test(el.tagName))
      )
        return;
      if (document.querySelector(".palette-scrim.is-open")) return;
      e.preventDefault();
      onSync();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onSync, applyOpen]);

  const head =
    detail?.role ?? rows.find((r) => r.reference === openRef) ?? null;
  // Movement, open, this screen's own verb, x, Esc. ⌘K is the bar's own.
  const hints: Hint[] = [
    { keys: ["j", "k"], label: "move" },
    { keys: ["↵"], label: "open" },
    { keys: ["s"], label: "sync" },
    { keys: ["x"], label: "dismiss" },
  ];
  if (openRef) hints.push({ keys: ["esc"], label: "close pane" });

  // The queue's own last hairline: the pane already ends below it.
  const lastRef = rows.length > 0 ? rows[rows.length - 1].reference : null;

  return (
    <>
      <div className={openRef ? "stage has-detail" : "stage"}>
        <section className="pane">
          <header className={openRef ? "pane-head tight" : "pane-head"}>
            <div style={{ ...column, maxWidth: openRef ? "none" : 660 }}>
              <div>
                <h1 className="pane-title">Feed</h1>
                <div className="pane-sub">{subtitle(options)}</div>
              </div>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={onSync}
                disabled={syncing}
              >
                Sync <span className="kbd">s</span>
              </button>
            </div>
          </header>

          <div
            className={openRef ? "pane-body pad-top" : "pane-body"}
            style={openRef ? { paddingLeft: 22, paddingRight: 18 } : undefined}
          >
            <div style={{ maxWidth: openRef ? "none" : 660 }}>
              {feed?.rulesError && (
                <div className="notice">
                  The rules did not parse, so this is every open role rather
                  than the matched ones.{" "}
                  <span className="mono" style={rawWords}>
                    {feed.rulesError}
                  </span>{" "}
                  The feed keeps working. The rules apply again when the file
                  parses.
                </div>
              )}
              {feedTrouble && (
                <div className="notice">
                  Perch did not get the feed back this time.{" "}
                  <span className="mono" style={rawWords}>
                    {feedTrouble}
                  </span>{" "}
                  Nothing was lost. The roles it has are still on this Mac.
                </div>
              )}
              {rowTrouble && (
                <div className="notice" style={rawWords}>
                  {rowTrouble}
                </div>
              )}

              {!feed && !feedTrouble && (
                <p className="muted">Reading the roles Perch has stored.</p>
              )}

              {feed && rows.length > 0 && (
                <div className="queue">
                  {groups.map((group) => (
                    <Fragment key={group.bucket}>
                      <div className="section-head">{group.bucket}</div>
                      {group.rows.map((r) => (
                        <article
                          key={r.reference}
                          className={`row row-${r.freshness}${r.reference === selected ? " is-selected" : ""}`}
                          data-row={r.reference}
                          style={
                            r.reference === lastRef
                              ? { borderBottom: 0 }
                              : undefined
                          }
                          onClick={() => {
                            setSelected(r.reference);
                            setOpenRef(r.reference);
                          }}
                        >
                          <div className="row-company">{r.company}</div>
                          <div className="row-title">{r.title}</div>
                          <div className="row-meta">
                            <span>{r.location}</span>
                            <span className="sep">·</span>
                            <span className={`t-${r.freshness}`}>
                              {r.signal}
                            </span>
                            {!r.fillSupported && (
                              <>
                                <span className="sep">·</span>
                                <span className="muted">opens in browser</span>
                              </>
                            )}
                          </div>
                          {r.whyRule && (
                            <div
                              className="row-why"
                              style={{ flexWrap: "wrap", rowGap: 3 }}
                            >
                              <span>matched</span>
                              <span className="rule-name">{r.whyRule}</span>
                              {r.whyBecause && <span>({r.whyBecause})</span>}
                            </div>
                          )}
                        </article>
                      ))}
                    </Fragment>
                  ))}
                </div>
              )}

              {feed &&
                rows.length === 0 &&
                emptyState(feed, options, onOptions, onSync)}
            </div>
          </div>
        </section>

        {openRef && (
          <section className="pane">
            {head && (
              <header className="pane-head" style={{ display: "block" }}>
                <div className="row-company">{head.company}</div>
                <h2 className="pane-title" style={{ marginTop: 3 }}>
                  {head.title}
                </h2>
                <div className="row-meta" style={{ marginTop: 9 }}>
                  <span>{head.location}</span>
                  <span className="sep">·</span>
                  <span className={`t-${head.freshness}`}>{head.signal}</span>
                  <span className="sep">·</span>
                  <span className="tag">{head.ats}</span>
                </div>
              </header>
            )}

            <div className="pane-body" style={{ paddingBottom: 32 }}>
              {detailTrouble && (
                <div className="notice">
                  Perch did not get this posting back.{" "}
                  <span className="mono" style={rawWords}>
                    {detailTrouble}
                  </span>{" "}
                  The role itself is still in the feed, and the board still has
                  it.
                </div>
              )}
              {!detail && !detailTrouble && (
                <p className="muted">Reading this posting.</p>
              )}

              {detail && (
                <>
                  {detail.applied && (
                    <p className="muted" style={{ ...footnote, marginTop: 4 }}>
                      You have already applied to this one. {detail.applied}
                    </p>
                  )}

                  <div className="section-head">What they wrote</div>
                  {detail.descriptionMissing ? (
                    <p className="muted" style={{ ...footnote, marginTop: 0 }}>
                      The board no longer has the posting's text.
                    </p>
                  ) : (
                    <div className="detail-prose">{prose(detail.blocks)}</div>
                  )}

                  {detail.history.length > 0 && (
                    <>
                      <div className="section-head">Board history</div>
                      <div className="timeline">
                        {detail.history.map((event, i) => (
                          <div
                            key={`${event.when}-${i}`}
                            className={
                              event.latest
                                ? "timeline-item is-latest"
                                : "timeline-item"
                            }
                          >
                            <div className="timeline-when">{event.when}</div>
                            <div className="timeline-what">{event.what}</div>
                          </div>
                        ))}
                      </div>
                      <p
                        className="muted"
                        style={{ ...footnote, marginTop: 2 }}
                      >
                        This is only what Perch watched happen on the board.
                        Anything before that is not recorded.
                      </p>
                    </>
                  )}

                  {detail.velocity && (
                    <>
                      <div className="section-head">Hiring velocity</div>
                      <div className="stat">
                        <span className="n">
                          {detail.velocity.openedRecently}
                        </span>
                        <span>
                          {detail.velocity.openedRecently === 1
                            ? "role"
                            : "roles"}{" "}
                          opened on this board in the last 90 days,{" "}
                          {counted(detail.velocity.stillOpen)} still open.
                        </span>
                      </div>
                      {detail.velocity.medianDaysToClose !== null && (
                        <div className="stat" style={{ marginTop: 10 }}>
                          <span className="n">
                            {detail.velocity.medianDaysToClose}
                          </span>
                          <span>
                            days, median, from a role appearing to it leaving
                            the board.
                          </span>
                        </div>
                      )}
                      <p
                        className="muted"
                        style={{ ...footnote, marginTop: 14 }}
                      >
                        {detail.velocity.caveat}
                      </p>
                    </>
                  )}
                </>
              )}
            </div>

            {/* The sheet carries the screen's one primary action while it is
                open; leaving this bar lit would put two on screen. */}
            {head && !applyOpen && (
              <div className="action-bar" style={{ paddingBottom: 44 }}>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => {
                    // A board Perch can fill goes through the sheet, where the
                    // plan is read before anything is typed. One it cannot fill
                    // is handed over as the page it is. Neither one sends.
                    if (head.fillSupported) {
                      onApply(head.reference);
                      return;
                    }
                    api
                      .openInBrowser(head.url)
                      .catch((err) => setDetailTrouble(plainly(err)));
                  }}
                >
                  {head.fillSupported ? "Start application" : "Open in browser"}
                </button>
                <button
                  type="button"
                  className="btn btn-ghost"
                  onClick={() => askToSetAside(head.reference)}
                >
                  Dismiss <span className="kbd">x</span>
                </button>
                <span
                  className="muted"
                  style={{ fontSize: "var(--t-12)", marginLeft: 6 }}
                >
                  {head.fillSupported
                    ? "You read what Perch would type before it types anything. It never submits."
                    : "Perch cannot fill this board's forms, so it opens the page as it is."}
                </span>
              </div>
            )}
          </section>
        )}
      </div>
      {!applyOpen && <HintBar hints={hints} undoNote={undoNote} />}
    </>
  );
}

const column: CSSProperties = {
  width: "100%",
  display: "flex",
  alignItems: "flex-end",
  justifyContent: "space-between",
  gap: 24,
};

const footnote: CSSProperties = {
  fontSize: "var(--t-12)",
  margin: "10px 0 0",
  maxWidth: "56ch",
};

/** Whatever the store said back, wrapped rather than pushing the pane wide. */
const rawWords: CSSProperties = { overflowWrap: "anywhere" };

const commandWord: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--t-12)",
  lineHeight: "inherit",
  color: "var(--ink-2)",
  background: "var(--paper-sunk)",
  border: "1px solid var(--rule)",
  borderRadius: 3,
  padding: "1px 5px",
  cursor: "pointer",
  whiteSpace: "nowrap",
};

/** A command that this screen can actually run, in the CLI's own words. */
function CommandWord({ text, onRun }: { text: string; onRun: () => void }) {
  return (
    <button type="button" style={commandWord} onClick={onRun}>
      {text}
    </button>
  );
}

function subtitle(options: FeedOptions): string {
  const scope = options.all
    ? "Every open role, the rules set aside."
    : options.fresh
      ? "Posted in the last 24 hours, newest first."
      : "Newest first, the order the boards posted them.";
  return options.company ? `${scope} Narrowed to ${options.company}.` : scope;
}

function counted(n: number): string {
  if (n === 0) return "none of them";
  if (n === 1) return "one of them";
  return `${n} of them`;
}

/**
 * Every empty state here is true of one situation only, and names the one
 * thing that would change it.
 */
function emptyState(
  feed: Feed,
  options: FeedOptions,
  onOptions: (o: FeedOptions) => void,
  onSync: () => void,
): ReactNode {
  if (!feed.hasBoards) {
    return (
      <Empty title="No boards yet.">
        Perch reads company job boards and nothing else, so the feed stays empty
        until it has one to read.{" "}
        <code style={{ whiteSpace: "nowrap" }}>watch add &lt;company&gt;</code>{" "}
        works out which ATS a company uses and puts its board in the rotation.
      </Empty>
    );
  }

  if (options.fresh) {
    return (
      <Empty title="Nothing posted in the last day.">
        Boards post in bursts, so a quiet day is ordinary.{" "}
        <CommandWord
          text="feed"
          onRun={() => onOptions({ ...options, fresh: false })}
        />{" "}
        shows everything still open.
      </Empty>
    );
  }

  if (!options.all && feed.openButUnmatched > 0) {
    const many = feed.openButUnmatched !== 1;
    return (
      <Empty title="Open, but held back by your rules.">
        {many
          ? `${feed.openButUnmatched} roles are open on the boards you watch, and your rules set every one of them aside.`
          : "One role is open on the boards you watch, and your rules set it aside."}{" "}
        <CommandWord
          text="feed --all"
          onRun={() => onOptions({ ...options, all: true })}
        />{" "}
        shows them anyway, in the same order.
      </Empty>
    );
  }

  if (options.company) {
    return (
      <Empty title={`Nothing open at ${options.company}.`}>
        Perch read that board and it is not showing a role right now. Boards
        post in bursts, so this is ordinary.{" "}
        <CommandWord
          text="feed"
          onRun={() => onOptions({ ...options, company: null })}
        />{" "}
        goes back to every company you watch.
      </Empty>
    );
  }

  return (
    <Empty title="Nothing open right now.">
      Every board Perch watches has been read, and none of them is showing a
      role. Boards post in bursts, so a few quiet days is ordinary.{" "}
      <CommandWord text="sync" onRun={onSync} /> checks them all now.
    </Empty>
  );
}

/**
 * The board's words, as words. Consecutive bullets become one list; every
 * block is a text node. This is never dangerouslySetInnerHTML. The text comes
 * from a job board, and the backend reduced it to plain words so the webview
 * never has to interpret it.
 */
function prose(blocks: Block[]): ReactNode[] {
  const out: ReactNode[] = [];
  let bullets: { text: string; at: number }[] = [];

  const flush = () => {
    if (bullets.length === 0) return;
    out.push(
      <ul key={`list-${bullets[0].at}`}>
        {bullets.map((b) => (
          <li key={b.at}>{b.text}</li>
        ))}
      </ul>,
    );
    bullets = [];
  };

  blocks.forEach((block, at) => {
    if (block.kind === "bullet") {
      bullets.push({ text: block.text, at });
      return;
    }
    flush();
    if (block.kind === "heading") out.push(<h4 key={at}>{block.text}</h4>);
    else out.push(<p key={at}>{block.text}</p>);
  });
  flush();
  return out;
}

/** Whatever came back, said plainly, without the machinery around it. */
function plainly(err: unknown): string {
  const text = err instanceof Error ? err.message : String(err);
  return text.replace(/^Error:\s*/i, "").trim() || "no reason came back";
}

import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import {
  api,
  type FillEntry,
  type FillPlan,
  type FillReport,
  type Started,
  type Profile,
  type Provenance,
} from "../lib/api";
import { HintBar, type Hint } from "../components/HintBar";
import { Count, count } from "../lib/words";

/**
 * The application sheet: read it, attach a file, open the form.
 *
 * Perch never submits an application. `perch_fill::Action` has no variant that
 * activates a control, so the plan this screen shows cannot express a click or
 * a submit, and this screen adds none. The flow ends at "Open and fill", with
 * the form open in front of the person.
 *
 * The three kinds of deliberate empty all reach the interface intact: the
 * flagged fields Perch will not guess at, the free text it does not write, and
 * the demographic questions it does not answer and does not store.
 *
 * Opening the form does not close this sheet. The fill happens in a page this
 * app cannot see, so the only honest thing to show at that moment is that the
 * form is opening, and then what the fill said it came to. A sheet that closed
 * on the way out reported success it had no way of knowing.
 */

type Doc = Profile["documents"][number];

const STEPS = ["Review", "Attach", "Open"] as const;
const LAST = STEPS.length - 1;

/** The file's type, for the .tag. The tag names types, never statuses. */
function fileType(path: string): string {
  const at = path.lastIndexOf(".");
  return at > 0 ? path.slice(at + 1).toLowerCase() : "file";
}

export function ApplyView({
  reference,
  onClose,
}: {
  reference: string;
  onClose: () => void;
}) {
  const [step, setStep] = useState(0);
  const [fill, setFill] = useState<FillPlan | null>(null);
  const [documents, setDocuments] = useState<Doc[] | null>(null);
  const [resume, setResume] = useState<string | null>(null);
  const [url, setUrl] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  // The window is up. What went into it is a separate question, answered by
  // the report or, if none arrives, by saying that none arrived.
  const [opened, setOpened] = useState(false);
  const [report, setReport] = useState<FillReport | null>(null);
  const [silence, setSilence] = useState(false);
  const [caveat, setCaveat] = useState<string | null>(null);
  // Set when the press did something other than start a fill.
  const [started, setStarted] = useState<Started | null>(null);
  // The application is the person's to send. Once they say they have, Perch
  // records it the way `apps mark <ref> in-flight` does, and says so here.
  const [recorded, setRecorded] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);

  const sheet = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);

  // The documents decide what the plan can attach, so they are read first and
  // the plan is asked for again whenever the chosen one changes. The count in
  // "what happens next" then stays true of the plan underneath it.
  useEffect(() => {
    let live = true;
    api
      .settings()
      .then((settings) => {
        if (!live) return;
        setDocuments(settings.profile.documents);
        setResume(
          settings.preferredResume ??
            settings.profile.documents[0]?.path ??
            null,
        );
      })
      .catch(() => {
        if (live) setDocuments([]);
      });
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    if (documents === null) return;
    let live = true;
    api
      .fillPlan(reference, resume)
      .then((next) => {
        if (!live) return;
        setFill(next);
        setTrouble(null);
      })
      .catch((err) => {
        if (!live) return;
        setTrouble(
          `Perch could not work out what it would type here. ${plainly(err)} Nothing was opened and nothing was sent.`,
        );
      });
    return () => {
      live = false;
    };
  }, [reference, resume, documents]);

  // Only wanted when there is no plan: the role opens in the browser instead.
  useEffect(() => {
    if (!fill || fill.fillable || url !== null) return;
    let live = true;
    api
      .detail(reference)
      .then((detail) => live && setUrl(detail.role.url))
      .catch((err) => {
        if (live)
          setTrouble(
            `Perch could not find this role's address. ${plainly(err)}`,
          );
      });
    return () => {
      live = false;
    };
  }, [fill, reference, url]);

  useEffect(() => {
    sheet.current?.focus();
  }, []);

  // The fill settles in the employer's page seconds after this sheet has done
  // its part. Listening starts before the form is opened, so nothing the fill
  // says can arrive before there is anyone to hear it.
  useEffect(() => {
    let live = true;
    const listening = api.onFillReport((said) => {
      if (!live || said.reference !== reference) return;
      setReport(said);
      setSilence(false);
    });
    return () => {
      live = false;
      listening.then((stop) => stop()).catch(() => {});
    };
  }, [reference]);

  // A window that was closed, or a page that never ran the fill, says nothing
  // at all. Waiting forever would read as success, so the wait has an end.
  useEffect(() => {
    if (!opened || report) return;
    const timer = window.setTimeout(() => setSilence(true), 20000);
    return () => window.clearTimeout(timer);
  }, [opened, report]);

  const fillable = fill?.fillable ?? false;
  const plan = fill?.plan ?? null;

  const go = useCallback((n: number) => {
    setStep(Math.max(0, Math.min(LAST, n)));
    if (body.current) body.current.scrollTop = 0;
  }, []);

  // Esc closes. ⌥← / ⌥→ move between the steps, ⌘↵ takes the one in front of
  // you. On the last step it takes none: opening the form is a deliberate act
  // and stays under the hand that means it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector(".palette-scrim.is-open")) return;
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return;
      }
      if (!fillable) return;
      if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        go(step + 1);
      } else if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        go(step - 1);
      } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && step < LAST) {
        e.preventDefault();
        go(step + 1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step, fillable, go, onClose]);

  const openAndFill = () => {
    if (opening) return;
    setOpening(true);
    setTrouble(null);
    api
      .openAndFill(reference, resume)
      .then((note) => {
        setCaveat(note.caveat);
        // Only a fill that actually started sends a report. Waiting on the
        // other two would end in Perch saying it never heard back about
        // something that was never going to speak.
        if (note.started === "filling") {
          setOpened(true);
        } else {
          setStarted(note.started);
        }
      })
      .catch((err) => {
        setOpening(false);
        setTrouble(
          `The form did not open. ${plainly(err)} Nothing was typed anywhere.`,
        );
      });
  };

  const recordSent = () => {
    if (recording || recorded) return;
    setRecording(true);
    setTrouble(null);
    api
      .markApplication(reference, "in_flight")
      .then(() =>
        setRecorded(
          "Recorded under Applications as in flight. Perch had no part in sending it, and will not chase it.",
        ),
      )
      .catch((err) =>
        setTrouble(`That application is not recorded yet. ${plainly(err)}`),
      )
      .finally(() => setRecording(false));
  };

  const openInBrowser = () => {
    if (!url || opening) return;
    setOpening(true);
    api
      .openInBrowser(url)
      .then(onClose)
      .catch((err) => {
        setOpening(false);
        setTrouble(`The page did not open. ${plainly(err)}`);
      });
  };

  const hints: Hint[] = [];
  if (fillable) {
    hints.push({ keys: ["⌥←", "⌥→"], label: "steps" });
    if (step < LAST) hints.push({ keys: ["⌘↵"], label: "continue" });
  }
  hints.push({ keys: ["esc"], label: "close sheet" });

  return (
    <>
      <div
        className="sheet-scrim"
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
      >
        <div
          className="sheet"
          role="dialog"
          aria-modal="true"
          aria-label={fill ? `${fill.title} at ${fill.company}` : "Application"}
          tabIndex={-1}
          ref={sheet}
        >
          <div className="sheet-head">
            <div style={headRow}>
              <div style={{ minWidth: 0 }}>
                <div className="row-company">{fill ? fill.company : " "}</div>
                <h2 className={fill ? "sheet-title" : "sheet-title muted"}>
                  {fill ? fill.title : "Reading what Perch would type"}
                </h2>
              </div>
              {plan && <span className="tag">{plan.ats}</span>}
            </div>

            {fillable && (
              <div className="step-marks">
                {STEPS.map((name, i) => (
                  <Fragment key={name}>
                    {i > 0 && <span className="step-sep">·</span>}
                    <button
                      type="button"
                      className={i === step ? "step-mark is-on" : "step-mark"}
                      aria-current={i === step ? "step" : undefined}
                      onClick={() => go(i)}
                    >
                      {name}
                    </button>
                  </Fragment>
                ))}
              </div>
            )}
          </div>

          <div className="sheet-body" ref={body}>
            {trouble && (
              <div className="notice" style={rawWords}>
                {trouble}
              </div>
            )}

            {started && (
              <div className="notice" style={rawWords} aria-live="polite">
                {started === "alreadyOpen"
                  ? "This form was already open, so Perch brought that window forward and typed nothing again."
                  : "This board is not one Perch fills, so the page opened in your browser."}
              </div>
            )}

            {opened && (
              <div className="notice" style={rawWords} aria-live="polite">
                <div>{outcome(report, silence)}</div>
                {report && fileNote(report) && (
                  <div style={{ marginTop: 8 }}>{fileNote(report)}</div>
                )}
                {caveat && <div style={{ marginTop: 8 }}>{caveat}</div>}
                {recorded && <div style={{ marginTop: 8 }}>{recorded}</div>}
              </div>
            )}

            {!fill && !trouble && (
              <p className="muted" style={{ marginTop: 18 }}>
                Reading your profile and the form it would go into.
              </p>
            )}

            {/* A board Perch cannot fill. The sentence, and the page. */}
            {fill && !fillable && (
              <>
                <div className="section-head tight">What happens next</div>
                <p className="lede">{fill.whatHappensNext}</p>
              </>
            )}

            {fill && plan && step === 0 && (
              <section aria-label="Review">
                {plan.entries.length > 0 && (
                  <>
                    <div className="section-head tight">From your profile</div>
                    <div className="sheet-grid">
                      {plan.entries.map((entry, i) => (
                        <div className="field" key={`${i}-${entry.selector}`}>
                          <div className="field-label">{entry.label}</div>
                          <div className="field-value value-line">
                            <span>{shown(entry)}</span>
                            <span className="prov">
                              {provLabel(entry.provenance)}
                            </span>
                          </div>
                        </div>
                      ))}
                    </div>
                  </>
                )}

                {plan.flagged.length > 0 && (
                  <>
                    <div className="section-head">
                      Perch is not guessing at these
                    </div>
                    {plan.flagged.map((field) => (
                      <div className="field" key={field.selector}>
                        <div className="field-label">{field.label}</div>
                        <input
                          className="input flagged"
                          type="text"
                          value=""
                          readOnly
                          aria-label={`${field.label}. Left blank on purpose`}
                        />
                        <p className="note" style={{ marginTop: 8 }}>
                          {field.why}
                        </p>
                      </div>
                    ))}
                  </>
                )}

                {plan.leftToYou.length > 0 && (
                  <>
                    <div className="section-head">Left to you</div>
                    {plan.leftToYou.map((field) => (
                      <div className="field" key={field.selector}>
                        <div className="field-label">{field.label}</div>
                        <textarea
                          className="textarea by-design"
                          rows={4}
                          value=""
                          readOnly
                          placeholder="Perch leaves this to you."
                          aria-label={`${field.label}. Perch leaves this to you`}
                        />
                        <p className="note" style={{ marginTop: 8 }}>
                          {field.why}
                        </p>
                      </div>
                    ))}
                  </>
                )}

                {plan.never.length > 0 && <Refused fields={plan.never} />}
              </section>
            )}

            {fill && plan && step === 1 && (
              <section aria-label="Attach">
                <div className="section-head tight">Attach one file</div>

                {documents && documents.length === 0 ? (
                  <>
                    <p className="lede" style={{ maxWidth: "54ch" }}>
                      No documents are listed yet.
                    </p>
                    <p
                      className="note"
                      style={{ marginTop: 8, maxWidth: "58ch" }}
                    >
                      They live in <span className="mono">profile.toml</span>: a
                      name, a path and a kind for each. Add one there and it
                      appears here. The rest of the form is filled either way,
                      and the page's own file picker still works.
                    </p>
                  </>
                ) : (
                  <div role="radiogroup" aria-label="The file to attach">
                    {(documents ?? []).map((doc) => (
                      <button
                        type="button"
                        role="radio"
                        aria-checked={doc.path === resume}
                        className={
                          doc.path === resume ? "doc-row is-on" : "doc-row"
                        }
                        key={doc.path}
                        onClick={() => setResume(doc.path)}
                      >
                        <span className="doc-mark" />
                        <span style={{ flex: 1, minWidth: 0 }}>
                          <span style={docLine}>
                            <span className="doc-name">{doc.name}</span>
                            <span className="tag">{fileType(doc.path)}</span>
                          </span>
                          <span
                            className="note mono"
                            style={{
                              display: "block",
                              marginTop: 4,
                              ...rawWords,
                            }}
                          >
                            {doc.path}
                          </span>
                        </span>
                      </button>
                    ))}
                  </div>
                )}

                <p className="note" style={{ marginTop: 16, maxWidth: "60ch" }}>
                  Perch attaches the file you pick, byte for byte. It does not
                  keep a version you have not read.
                </p>
                {fill.attachmentCaveat && (
                  <p className="note" style={{ marginTop: 8, maxWidth: "60ch" }}>
                    {fill.attachmentCaveat}
                  </p>
                )}
              </section>
            )}

            {fill && plan && step === 2 && (
              <section aria-label="Open">
                <div className="section-head tight">What happens next</div>
                <p className="lede">{fill.whatHappensNext}</p>

                <div className="section-head">Fill plan</div>
                {plan.entries.length === 0 ? (
                  <p className="note" style={{ maxWidth: "60ch" }}>
                    There is nothing to type: your profile has none of the
                    values this form asks for. The form opens as it is, and you
                    fill it in.
                  </p>
                ) : (
                  <div className="plan">
                    {plan.entries.map((entry, i) => (
                      <div className="plan-row" key={`${i}-${entry.selector}`}>
                        <span>{entry.selector}</span>
                        <span className="arrow">→</span>
                        <span className="val">{shown(entry)}</span>
                        <span className="prov">
                          {provLabel(entry.provenance)}
                        </span>
                      </div>
                    ))}
                  </div>
                )}

                {blanksLine(plan.flagged.length, plan.leftToYou.length) && (
                  <p
                    className="note"
                    style={{ marginTop: 12, maxWidth: "62ch" }}
                  >
                    {blanksLine(plan.flagged.length, plan.leftToYou.length)}
                  </p>
                )}
              </section>
            )}
          </div>

          <div className="sheet-foot">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => (step === 0 ? onClose() : go(step - 1))}
            >
              {step === 0 ? "Close" : "Back"}
            </button>

            <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
              {fillable && step < LAST && (
                <span className="note">
                  <span className="kbd">⌘↵</span> continue
                </span>
              )}
              {fill && !fillable && (
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={url === null || opening}
                  onClick={openInBrowser}
                >
                  Open in browser
                </button>
              )}
              {fillable && step < LAST && (
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => go(step + 1)}
                >
                  Continue
                </button>
              )}
              {fillable && step === LAST && !opened && !started && (
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={opening}
                  onClick={openAndFill}
                >
                  {opening ? "Opening the form" : "Open and fill"}
                </button>
              )}
              {fillable && step === LAST && opened && !recorded && (
                <button
                  type="button"
                  className="btn btn-ghost"
                  disabled={recording}
                  onClick={recordSent}
                >
                  Record that you sent it
                </button>
              )}
              {fillable && step === LAST && (opened || started) && (
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={onClose}
                >
                  Close
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
      <HintBar hints={hints} />
    </>
  );
}

/**
 * The demographic questions, named and refused.
 *
 * They are never fields here: no input, no row that could be typed into, and
 * nothing that could be mistaken for one Perch might fill later. The reasons
 * are all one sentence in practice, so it is said once rather than four times.
 */
function Refused({ fields }: { fields: { label: string; why: string }[] }) {
  const shared = fields.every((f) => f.why === fields[0].why)
    ? fields[0].why
    : null;
  return (
    <>
      <div className="section-head">Never answered</div>
      <ul className="refusals">
        {fields.map((field) => (
          <li key={field.label}>
            {field.label}
            {!shared && <span className="muted">: {field.why}</span>}
          </li>
        ))}
      </ul>
      <p className="note" style={{ marginTop: 10, maxWidth: "62ch" }}>
        {shared ? `${shared} ` : ""}It does not ask them here either.
      </p>
    </>
  );
}

/** What will be put there, as `Action::shown_value` returns it. */
function shown(entry: FillEntry): string {
  return entry.action === "attachFile" ? entry.path : entry.value;
}

/** How the provenance reads under a value, as `Provenance::label` writes it. */
function provLabel(provenance: Provenance): string {
  return provenance.from === "profile" ? "profile.toml" : provenance.name;
}

/**
 * What the fill came to, in one sentence.
 *
 * Every number and label in here was counted in the employer's page. The
 * sentence says what was seen there and claims nothing beyond it, which is the
 * whole reason the report exists.
 */
function outcome(report: FillReport | null, silence: boolean): string {
  if (!report) {
    return silence
      ? "The form is open in a window of its own. Perch did not hear back from it."
      : "The form is opening in a window of its own. Perch says what went into it once the fill has settled.";
  }
  if (report.filled === 0) {
    // Something stopping the fill is not the same as the fill having done
    // nothing. The count is taken by the same code the trouble came from, so
    // when there is trouble the count is not evidence of an untouched form and
    // is not reported as though it were.
    if (report.error) {
      return `Perch could not tell what went into the form, which is open in a window of its own. What stopped it: ${stopped(
        report.error,
      )}`;
    }
    // A plan with no values in it never had anything to type. Saying the form
    // was left untouched would be about the file, which is a separate line.
    if (report.planned === 0) {
      return "There were no values to type into this form, which is open in a window of its own.";
    }
    return "Nothing was typed. The form is open and untouched.";
  }
  if (report.missing.length > 0) {
    return `${Count(report.filled)} of ${count(report.planned)} values are in the form, which is open in a window of its own. ${listed(
      report.missing,
    )} ${report.missing.length === 1 ? "was not a box" : "were not boxes"} on it.`;
  }
  if (report.filled < report.planned) {
    return `${Count(report.filled)} of ${count(report.planned)} values are in the form, which is open in a window of its own.`;
  }
  return `${Count(report.filled)} ${
    report.filled === 1 ? "value is" : "values are"
  } in the form, which is open in a window of its own.`;
}

/**
 * Whatever the page said stopped it, ended as a sentence. The words are the
 * page's own, so they are not rewritten, only closed.
 */
function stopped(error: string): string {
  const said = error.trim();
  return /[.!?]$/.test(said) ? said : `${said}.`;
}

/**
 * Where the file got to, when one was planned and the fill reached the form.
 *
 * A fill that found none of its boxes is already reported as having touched
 * nothing, and a file box is one of the boxes it did not find.
 */
function fileNote(report: FillReport): string | null {
  if (!report.file || report.error) return null;
  if (report.filled === 0 && report.planned > 0) return null;
  // What was seen is the file's name printed on the page, which is the form
  // saying it took it. That is the evidence, so that is what is said: Perch
  // cannot see into the upload itself.
  return report.fileNamed
    ? "The form shows the file's name."
    : "The form has not said it took the file, so check the file box before you send it.";
}

/** Labels as a person would say them aloud. */
function listed(labels: string[]): string {
  if (labels.length < 2) return labels.join("");
  return `${labels.slice(0, -1).join(", ")} and ${labels[labels.length - 1]}`;
}

/** How many boxes stay empty on purpose, and how many are yours to write. */
function blanksLine(flagged: number, left: number): string | null {
  if (flagged === 0 && left === 0) return null;
  const parts: string[] = [];
  if (flagged > 0) {
    parts.push(
      `${Count(flagged)} ${flagged === 1 ? "box stays" : "boxes stay"} empty on purpose`,
    );
  }
  if (left > 0) {
    parts.push(
      `${flagged > 0 ? count(left) : Count(left)} ${
        left === 1 ? "is yours" : "are yours"
      } to write`,
    );
  }
  return `${parts.join(", and ")}. Perch types nothing into any of them.`;
}

/** Whatever came back, said plainly, without the machinery around it. */
function plainly(err: unknown): string {
  const text = err instanceof Error ? err.message : String(err);
  return text.replace(/^Error:\s*/i, "").trim() || "no reason came back";
}

const headRow: CSSProperties = {
  display: "flex",
  alignItems: "flex-start",
  justifyContent: "space-between",
  gap: 16,
};

const docLine: CSSProperties = {
  display: "flex",
  alignItems: "baseline",
  gap: 8,
  flexWrap: "wrap",
};

/** Whatever the store said back, wrapped rather than pushing the sheet wide. */
const rawWords: CSSProperties = { overflowWrap: "anywhere" };

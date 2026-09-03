import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { api, type Profile, type Settings } from "../lib/api";
import { useQueue } from "../lib/keys";
import { Empty } from "../components/Empty";
import { HintBar } from "../components/HintBar";

/**
 * Profile: a window onto two plain text files.
 *
 * Nothing here is a form. Perch reads `profile.toml` and `rules.toml` off the
 * disk every time it runs, so the file is the truth and this screen is the
 * reading of it. There is no primary action, because the action is: open the
 * file in your editor.
 */

const quiet: CSSProperties = {
  fontSize: "var(--t-13)",
  lineHeight: 1.65,
  margin: 0,
  color: "var(--ink-3)",
};

/** A path is a single unbreakable word. It wraps; the pane never scrolls sideways. */
const pathText: CSSProperties = { overflowWrap: "anywhere" };

/** The quiet selection for a field row: moving through the profile spends no accent. */
const fieldSelected: CSSProperties = {
  background: "var(--paper-sunk)",
  borderRadius: 4,
  margin: "0 -10px",
  padding: "11px 10px",
};

const FIELDS: { key: string; label: string; of: (p: Profile) => string }[] = [
  { key: "name", label: "Name", of: (p) => p.name },
  { key: "email", label: "Email", of: (p) => p.email },
  { key: "phone", label: "Phone", of: (p) => p.phone },
  { key: "location", label: "Location", of: (p) => p.location },
  { key: "work", label: "Work authorisation", of: (p) => p.work_authorisation },
  { key: "github", label: "GitHub", of: (p) => p.links.github },
  { key: "website", label: "Website", of: (p) => p.links.website },
  { key: "linkedin", label: "LinkedIn", of: (p) => p.links.linkedin },
  { key: "skills", label: "Skills", of: (p) => p.skills.join(", ") },
];

/** Whatever came back, said plainly, without the machinery around it. */
function plainly(err: unknown): string {
  const text = err instanceof Error ? err.message : String(err);
  return (
    text.replace(/^Error:\s*/i, "").trim() ||
    "the file could not be read just now"
  );
}

/** `.tag` carries a file type and nothing else, so read one off the path. */
function fileType(path: string): string | null {
  const base = path.slice(path.lastIndexOf("/") + 1);
  const dot = base.lastIndexOf(".");
  return dot > 0 && dot < base.length - 1
    ? base.slice(dot + 1).toLowerCase()
    : null;
}

function positionsLine(count: number): string {
  if (count === 0) return "None listed yet.";
  if (count === 1) return "One position, listed below.";
  return `${count} positions, listed below.`;
}

function documentsLine(count: number): string {
  if (count === 0) return "None listed yet.";
  if (count === 1) return "One file, listed below.";
  return `${count} files, listed below.`;
}

function FieldRow({
  id,
  label,
  value,
  selected,
  onSelect,
}: {
  id: string;
  label: string;
  value: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <div
      className="field"
      data-row={id}
      onClick={onSelect}
      style={selected ? fieldSelected : undefined}
    >
      <div className="field-label">{label}</div>
      <div className="field-value">
        {value ? value : <span style={{ color: "var(--ink-4)" }}>not set</span>}
      </div>
    </div>
  );
}

/**
 * Somewhere to put the API key for a remote endpoint.
 *
 * The key goes one way: this field, into the command, into the keychain. It is
 * never read back here, and nothing on this screen can say whether Perch is
 * holding one, because finding out would mean reading it and reading it asks
 * macOS for a password. A key that is missing turns up at the point of use,
 * where the import screen names the host that would not take the request.
 */
function ModelKey({ endpoint }: { endpoint: string }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ text: string; stored: boolean } | null>(
    null,
  );
  const [trouble, setTrouble] = useState<string | null>(null);
  const onScreen = useRef(true);

  useEffect(() => {
    return () => {
      onScreen.current = false;
    };
  }, []);

  const store = useCallback(async () => {
    if (busy || key.trim() === "") return;
    setBusy(true);
    setSaid(null);
    setTrouble(null);
    try {
      const answer = await api.modelKeySet(key);
      if (onScreen.current) setSaid({ text: answer, stored: true });
    } catch (err) {
      if (onScreen.current) setTrouble(plainly(err));
    } finally {
      // However it went, Perch has no further use for what it was handed, so
      // the field and the state behind it are empty from here.
      setKey("");
      if (onScreen.current) setBusy(false);
    }
  }, [busy, key]);

  const forget = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setSaid(null);
    setTrouble(null);
    try {
      const answer = await api.modelKeyForget();
      if (onScreen.current) setSaid({ text: answer, stored: false });
    } catch (err) {
      if (onScreen.current) setTrouble(plainly(err));
    } finally {
      if (onScreen.current) setBusy(false);
    }
  }, [busy]);

  return (
    <div style={{ marginTop: 12 }}>
      <input
        className="input"
        type="password"
        autoComplete="off"
        spellCheck={false}
        aria-label={`API key for ${endpoint}`}
        placeholder="API key for this endpoint"
        value={key}
        onChange={(e) => setKey(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void store();
          }
        }}
      />

      <div style={{ display: "flex", gap: 10, marginTop: 10 }}>
        <button
          type="button"
          className="btn"
          disabled={busy || key.trim() === ""}
          onClick={() => void store()}
        >
          Store it in the keychain
        </button>
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={() => void forget()}
        >
          Forget the stored key
        </button>
      </div>

      <p style={{ ...quiet, marginTop: 10 }}>
        The key goes from this field into the keychain. Perch does not read it
        back, and does not look to see whether one is there.
      </p>

      {said && (
        <p style={{ ...quiet, marginTop: 10, color: "var(--ink)" }}>
          {said.text}
          {said.stored &&
            " Nothing has been asked of the endpoint, so whether it takes this key is answered the next time you read a résumé."}
        </p>
      )}

      {trouble && (
        <div className="notice" style={{ marginTop: 12 }}>
          {trouble}
        </div>
      )}
    </div>
  );
}

export function ProfileView({ onImport }: { onImport: () => void }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [unread, setUnread] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api.settings().then(
      (result) => {
        if (alive) setSettings(result);
      },
      (err) => {
        if (alive) setUnread(plainly(err));
      },
    );
    return () => {
      alive = false;
    };
  }, []);

  // One selection runs the length of the screen: the fields, then the
  // documents, then the rules. Nothing here is clearable, so x does nothing.
  const rows = useMemo(() => {
    if (!settings) return [];
    const ids = FIELDS.map((f) => `field:${f.key}`);
    ids.push("field:experience");
    ids.push("field:documents");
    settings.profile.experience.forEach((_, i) => ids.push(`job:${i}`));
    settings.profile.documents.forEach((_, i) => ids.push(`doc:${i}`));
    settings.rules.forEach((_, i) => ids.push(`rule:${i}`));
    return ids.map((reference) => ({ reference }));
  }, [settings]);

  const { selected, setSelected } = useQueue(rows);

  const profile = settings?.profile;
  const experience = profile?.experience ?? [];
  const documents = profile?.documents ?? [];
  const rules = settings?.rules ?? [];

  return (
    <>
      <div className="stage">
        <section className="pane">
          <header className="pane-head" style={{ maxWidth: 724 }}>
            <div style={{ minWidth: 0 }}>
              <h1 className="pane-title">Profile</h1>
              <div className="pane-sub">
                {settings ? (
                  <>
                    All of this is{" "}
                    <span className="mono" style={pathText}>
                      {settings.profilePath}
                    </span>
                    , a plain text file you can open in any editor and change by
                    hand.
                  </>
                ) : (
                  <>Perch keeps all of this in one plain text file.</>
                )}
              </div>
            </div>
            {settings && (
              <div
                style={{
                  ...quiet,
                  flex: "none",
                  textAlign: "right",
                  whiteSpace: "nowrap",
                }}
              >
                Perch reads the file back each time it runs.
              </div>
            )}
          </header>

          <div className="pane-body pad-top">
            <div style={{ maxWidth: 680 }}>
              {!settings && !unread && (
                <p style={{ ...quiet, paddingTop: 18 }}>
                  Reading the profile and the rules off disk.
                </p>
              )}

              {unread && (
                <div className="notice" style={{ marginTop: 18 }}>
                  <div>Perch could not read the profile just now.</div>
                  <div className="mono" style={{ ...pathText, marginTop: 6 }}>
                    {unread}
                  </div>
                  <div style={{ marginTop: 6 }}>
                    The file itself is untouched. Opening this screen again
                    reads it afresh.
                  </div>
                </div>
              )}

              {settings && profile && (
                <>
                  {/* ── You ───────────────────────────────────────── */}
                  <div className="section-head">You</div>

                  <div className="card">
                    {FIELDS.map((field) => (
                      <FieldRow
                        key={field.key}
                        id={`field:${field.key}`}
                        label={field.label}
                        value={field.of(profile)}
                        selected={selected === `field:${field.key}`}
                        onSelect={() => setSelected(`field:${field.key}`)}
                      />
                    ))}
                    <FieldRow
                      id="field:experience"
                      label="Experience"
                      value={positionsLine(experience.length)}
                      selected={selected === "field:experience"}
                      onSelect={() => setSelected("field:experience")}
                    />
                    <FieldRow
                      id="field:documents"
                      label="Documents"
                      value={documentsLine(documents.length)}
                      selected={selected === "field:documents"}
                      onSelect={() => setSelected("field:documents")}
                    />
                  </div>

                  <p style={{ ...quiet, marginTop: 14 }}>
                    Perch never fills demographic or EEO questions, and there is
                    nowhere in this file to keep an answer to one.
                  </p>

                  {/* ── Experience ────────────────────────────────── */}
                  <div className="section-head">Experience</div>

                  {experience.length === 0 ? (
                    <Empty title="No positions yet">
                      Positions live in <code>profile.toml</code>, most recent
                      first. Reading a résumé proposes them one at a time.
                    </Empty>
                  ) : (
                    <div className="queue">
                      {experience.map((position, i) => {
                        const id = `job:${i}`;
                        return (
                          <div
                            key={id}
                            className={`row ${selected === id ? "is-selected" : ""}`}
                            data-row={id}
                            onClick={() => setSelected(id)}
                          >
                            <div
                              style={{
                                fontSize: "var(--t-13)",
                                color: "var(--ink)",
                              }}
                            >
                              {position.company}
                            </div>
                            <div className="row-meta" style={{ marginTop: 6 }}>
                              {position.title && (
                                <>
                                  <span>{position.title}</span>
                                  <span className="sep">·</span>
                                </>
                              )}
                              <span>{position.dates}</span>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}

                  {/* ── Documents ─────────────────────────────────── */}
                  <div className="section-head">Documents</div>

                  {documents.length === 0 ? (
                    <Empty title="No documents yet">
                      Documents are listed in <code>profile.toml</code>. Give
                      one a name, a path and a kind there, and it appears here.
                    </Empty>
                  ) : (
                    <div className="queue">
                      {documents.map((doc, i) => {
                        const id = `doc:${i}`;
                        const type = fileType(doc.path);
                        return (
                          <div
                            key={id}
                            className={`row ${selected === id ? "is-selected" : ""}`}
                            data-row={id}
                            onClick={() => setSelected(id)}
                          >
                            <div
                              style={{
                                display: "flex",
                                alignItems: "baseline",
                                gap: 10,
                                flexWrap: "wrap",
                              }}
                            >
                              <span
                                className="mono"
                                style={{
                                  ...pathText,
                                  fontSize: "var(--t-13)",
                                  color: "var(--ink)",
                                }}
                              >
                                {doc.name}
                              </span>
                              {type && <span className="tag">{type}</span>}
                            </div>
                            <div className="row-meta" style={{ marginTop: 6 }}>
                              {doc.kind && (
                                <>
                                  <span>{doc.kind}</span>
                                  <span className="sep">·</span>
                                </>
                              )}
                              <span className="mono" style={pathText}>
                                {doc.path}
                              </span>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}

                  {/* ── Rules ─────────────────────────────────────── */}
                  <div className="section-head">Rules</div>

                  {settings.rulesError && (
                    <div className="notice">
                      <div>
                        The rules file did not parse, so Perch is not reading
                        any rules from it.
                      </div>
                      <div
                        className="mono"
                        style={{ ...pathText, marginTop: 6 }}
                      >
                        {settings.rulesError}
                      </div>
                      <div style={{ marginTop: 6 }}>
                        Until it parses, the feed shows every open role and
                        matches nothing away.
                      </div>
                    </div>
                  )}

                  {rules.length === 0 ? (
                    !settings.rulesError && (
                      <Empty title="No rules yet">
                        The feed shows everything Perch finds. Rules narrow that
                        down; they do not gate it. Write one in{" "}
                        <code>rules.toml</code> and the feed starts naming it.
                      </Empty>
                    )
                  ) : (
                    <>
                      <p style={quiet}>
                        Perch tries these top to bottom. The first one to fire
                        is the one the feed names.
                      </p>
                      <div className="queue" style={{ marginTop: 12 }}>
                        {rules.map((rule, i) => {
                          const id = `rule:${i}`;
                          return (
                            <div
                              key={id}
                              className={`row ${selected === id ? "is-selected" : ""}`}
                              data-row={id}
                              onClick={() => setSelected(id)}
                            >
                              <div className="row-why" style={{ marginTop: 0 }}>
                                <span className="rule-name">{rule.name}</span>
                              </div>
                              <div style={{ marginTop: 8 }}>
                                {rule.conditions.map((condition, c) => (
                                  <div className="row-meta" key={c}>
                                    {condition}
                                  </div>
                                ))}
                              </div>
                            </div>
                          );
                        })}
                      </div>
                    </>
                  )}

                  <p style={{ ...quiet, marginTop: 14 }}>
                    They live in{" "}
                    <span className="mono" style={pathText}>
                      {settings.rulesPath}
                    </span>
                    , next to the profile, and are read the same way.
                  </p>

                  {/* ── Closing ───────────────────────────────────── */}
                  <hr className="hr" />

                  <p style={quiet}>
                    Everything Perch has collected sits in{" "}
                    <span className="mono" style={pathText}>
                      {settings.databasePath}
                    </span>
                    . There is no account, no sync and no telemetry. Nothing
                    leaves this Mac.
                  </p>

                  {/* ── Model ─────────────────────────────────────── */}
                  <div className="section-head">Model</div>

                  <p style={quiet}>
                    A model is used for one thing: reading a résumé into
                    proposed profile fields, which you then accept or edit one
                    at a time. Watching boards, matching roles, filling forms
                    and tracking applications all work with no model configured.
                  </p>

                  {settings.model.ollamaRunning ? (
                    <p style={{ ...quiet, marginTop: 10 }}>
                      Ollama is running on this Mac.{" "}
                      {settings.model.onThisMac.length > 0
                        ? `It has ${settings.model.onThisMac.join(", ")} installed.`
                        : "It has no models installed yet."}{" "}
                      Reading a résumé is a small job; a 3B model does it well
                      enough.
                    </p>
                  ) : (
                    <p style={{ ...quiet, marginTop: 10 }}>
                      Nothing is listening on localhost:11434, so Ollama is not
                      running here. Perch can also use LM Studio,
                      llama.cpp&rsquo;s server, or any OpenAI-compatible
                      endpoint.
                    </p>
                  )}

                  <p
                    style={{
                      ...quiet,
                      marginTop: 10,
                      color: settings.model.refused ? "var(--ink)" : undefined,
                    }}
                  >
                    {settings.model.consequence}
                  </p>

                  <p style={{ ...quiet, marginTop: 10 }}>
                    Set it from the command line.{" "}
                    <span className="mono" style={pathText}>
                      perch model list
                    </span>{" "}
                    shows what this Mac can run.
                  </p>

                  <p style={{ ...quiet, marginTop: 10 }}>
                    Reading a résumé asks about each field in turn.{" "}
                    <span className="mono" style={pathText}>
                      perch profile import &lt;file&gt;
                    </span>{" "}
                    does it in the terminal.
                  </p>

                  <button
                    type="button"
                    className="btn"
                    style={{ marginTop: 12 }}
                    onClick={onImport}
                  >
                    Read a résumé
                  </button>

                  <p style={{ ...quiet, marginTop: 10 }}>
                    Whatever a model proposes is checked back against the
                    document it came from. A value Perch cannot point at in the
                    source is not offered at all, and nothing reaches{" "}
                    <span className="mono" style={pathText}>
                      {settings.profilePath}
                    </span>{" "}
                    without you accepting it.
                  </p>

                  <p style={{ ...quiet, marginTop: 10 }}>
                    Settings live in{" "}
                    <span className="mono" style={pathText}>
                      {settings.modelPath}
                    </span>
                    . An API key for a remote endpoint is kept in the system
                    keychain, so there is nowhere in that file for one.
                  </p>

                  {settings.model.configured &&
                    (settings.model.local ? (
                      <p style={{ ...quiet, marginTop: 10 }}>
                        That endpoint is on this Mac, so it wants no key.
                      </p>
                    ) : (
                      <ModelKey endpoint={settings.model.endpoint} />
                    ))}
                </>
              )}
            </div>
          </div>
        </section>
      </div>
      <HintBar hints={[{ keys: ["j", "k"], label: "move" }]} />
    </>
  );
}

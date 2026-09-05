import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** Mirrors the DTOs in `app/src-tauri/src/lib.rs`. */

/** Which of the three things pressing Open and fill did. */
export type Started = "filling" | "alreadyOpen" | "browser";

/**
 * What pressing Open and fill came to in the moment. Only `filling` is
 * followed by a `fill-report`, so the other two must not be waited on.
 */
export interface FillStarted {
  started: Started;
  caveat: string | null;
}

export type Freshness = "fresh" | "recent" | "settled" | "stale" | "tired";

export interface RoleRow {
  reference: string;
  company: string;
  title: string;
  location: string;
  url: string;
  ats: string;
  fillSupported: boolean;
  /** Already written for a person: "posted 6 hours ago", "open 143 days". */
  signal: string;
  freshness: Freshness;
  bucket: string;
  whyRule: string | null;
  whyBecause: string | null;
}

/** A posting's words, never markup. The board's HTML is reduced server-side. */
export interface Block {
  kind: "heading" | "paragraph" | "bullet";
  text: string;
}

export interface HistoryEvent {
  when: string;
  what: string;
  latest: boolean;
}

export interface Velocity {
  openedRecently: number;
  stillOpen: number;
  medianDaysToClose: number | null;
  caveat: string;
}

export interface Detail {
  role: RoleRow;
  blocks: Block[];
  descriptionMissing: boolean;
  history: HistoryEvent[];
  velocity: Velocity | null;
  applied: string | null;
}

export interface Feed {
  roles: RoleRow[];
  openButUnmatched: number;
  hasBoards: boolean;
  rulesError: string | null;
}

export interface Board {
  company: string;
  key: string;
  ats: string;
  url: string;
  checked: string;
  fillSupported: boolean;
  history: string;
}

export interface Application {
  reference: string;
  company: string;
  title: string;
  state: "in_flight" | "responded" | "archived";
  heading: string;
  applied: string;
  note: string | null;
  archivedQuietly: boolean;
  freshness: Freshness;
}

export interface SyncResult {
  lines: string[];
  failures: string[];
  quiet: boolean;
  boardsRead: number;
}

export interface Rule {
  name: string;
  conditions: string[];
}

export interface Profile {
  name: string;
  email: string;
  phone: string;
  location: string;
  work_authorisation: string;
  links: { github: string; website: string; linkedin: string };
  /** Free-form, in the person's own words. */
  skills: string[];
  /** Positions as the résumé states them, most recent first. */
  experience: { company: string; title: string; dates: string }[];
  documents: { name: string; path: string; kind: string }[];
}

export interface ModelState {
  configured: boolean;
  name: string;
  endpoint: string;
  /** Where things stand, in one sentence, written for a person. */
  consequence: string;
  local: boolean;
  /** Remote and not consented: Perch will not send the résumé. */
  refused: boolean;
  onThisMac: string[];
  ollamaRunning: boolean;
}

export interface Settings {
  profile: Profile;
  profilePath: string;
  rulesPath: string;
  databasePath: string;
  rules: Rule[];
  rulesError: string | null;
  model: ModelState;
  modelPath: string;
  /** The document an application attaches unless told otherwise. */
  preferredResume: string | null;
}

/** Where a value came from. Shown beside every value Perch would type. */
export type Provenance =
  { from: "profile" } | { from: "document"; name: string };

/**
 * One value Perch would put into the form.
 *
 * These are every action `perch_fill` can express. None of them activates a
 * control: there is no click here and no submit, because the Rust type this
 * mirrors has no such variant to serialise.
 */
export type FillEntry = { label: string; provenance: Provenance } & (
  | { action: "setText"; selector: string; value: string }
  | { action: "setChoice"; selector: string; value: string }
  | { action: "attachFile"; selector: string; path: string }
);

/** Perch could answer it, and will not guess. Left blank, with the reason. */
export interface FlaggedField {
  label: string;
  selector: string;
  why: string;
}

/** Free text Perch does not write. */
export interface LeftToYouField {
  label: string;
  selector: string;
  why: string;
}

/** A question Perch will not answer at all, and does not store an answer to. */
export interface RefusedField {
  label: string;
  why: string;
}

export interface Plan {
  ats: string;
  url: string;
  entries: FillEntry[];
  flagged: FlaggedField[];
  leftToYou: LeftToYouField[];
  never: RefusedField[];
}

/**
 * What the fill said it managed to do, as `FillReportDto` writes it.
 *
 * The script that fills the form runs in the employer's own page, and that
 * page can write to `window` as easily as the script can. This is something to
 * read, never something to decide on.
 */
export interface FillReport {
  /** The role it is about. A second window's report is not this one's. */
  reference: string;
  /** Planned values sitting in their boxes on the form. */
  filled: number;
  planned: number;
  /** The labels of the boxes that were not on the form. */
  missing: string[];
  file: boolean;
  fileNamed: boolean;
  /** Why nothing happened, when something stopped it. */
  error: string | null;
}

export interface FillPlan {
  /** Absent when this board's forms are not ones Perch can fill. */
  plan: Plan | null;
  /** What will happen and where it stops, in the app's own words. */
  whatHappensNext: string;
  /**
   * Set when the plan attaches a file: the form uploads it itself, and Perch
   * cannot see from outside the page whether that landed.
   */
  attachmentCaveat: string | null;
  company: string;
  title: string;
  fillable: boolean;
}

/** Where a résumé would be read, said before a file is chosen. */
export interface ImportReady {
  configured: boolean;
  verdict: "local" | "remote" | "refused" | "none";
  /** The host a résumé would be sent to, when it would leave this Mac. */
  host: string | null;
  /** What reading a résumé would do, in `Model::consequence`'s words. */
  sentence: string;
  /** Where a person changes that answer. */
  remedy: string | null;
  profilePath: string;
}

/**
 * One proposed field, with the evidence for it.
 *
 * `offerable` is the whole point of the screen: false means the document does
 * not say this, and there is no Accept control for it anywhere. `accepted`
 * arrives true only for a quotation; anything the model read rather than
 * quoted waits for a person.
 */
export interface ImportProposal {
  /** Its place in the read, which is how a decision names it later. */
  id: number;
  label: string;
  value: string;
  /** What the profile holds today, if anything. */
  current: string | null;
  /** The document line the value was found on, quoted whole. */
  quote: string | null;
  /** The matched span within `quote`, counted in UTF-16 code units. */
  highlight: [number, number] | null;
  line: number | null;
  offerable: boolean;
  accepted: boolean;
  /** Why a loose match wants a second look. */
  note: string | null;
  /** Why this one is not offered. */
  refusal: string | null;
  /** The values this row writes, when it writes more than one. */
  parts: string | null;
}

export interface ImportRead {
  /** The file's own name, for the lede. */
  document: string;
  /** Where the reading happened, and whether anything left this Mac. */
  provenance: string;
  proposals: ImportProposal[];
  /** Set when nothing the model returned is supported by the document. */
  nothingAnchored: string | null;
  profilePath: string;
}

/** What a person decided about one row. */
export interface ImportDecision {
  id: number;
  accepted: boolean;
  /** The person's own words, when they typed over the proposed value. */
  value: string | null;
}

export interface ImportWritten {
  written: number;
  profilePath: string;
}

export const api = {
  feed: (
    opts: { fresh?: boolean; company?: string | null; all?: boolean } = {},
  ) =>
    invoke<Feed>("feed", {
      fresh: opts.fresh ?? false,
      company: opts.company ?? null,
      all: opts.all ?? false,
    }),
  detail: (reference: string) => invoke<Detail>("detail", { reference }),
  dismiss: (reference: string) => invoke<void>("dismiss", { reference }),
  restore: (reference: string) => invoke<void>("restore", { reference }),
  sync: () => invoke<SyncResult>("sync"),
  watchlist: () => invoke<Board[]>("watchlist"),
  lastRead: () => invoke<string>("last_read"),
  watchAdd: (input: string) => invoke<string>("watch_add", { input }),
  watchRemove: (key: string) => invoke<boolean>("watch_remove", { key }),
  applications: () => invoke<Application[]>("applications"),
  markApplication: (reference: string, appState: string, note?: string) =>
    invoke<void>("mark_application", {
      reference,
      appState,
      note: note ?? null,
    }),
  settings: () => invoke<Settings>("settings"),
  /**
   * Put a key for the configured endpoint in this Mac's keychain. What comes
   * back is a sentence naming the host. The key is never handed back: no
   * command returns one and no DTO carries one.
   */
  modelKeySet: (key: string) => invoke<string>("model_key_set", { key }),
  /** Take it out again. A host holding no key is not an error. */
  modelKeyForget: () => invoke<string>("model_key_forget"),
  openInBrowser: (url: string) => invoke<void>("open_in_browser", { url }),
  /**
   * What Perch would type into this role's form, to be read first. Building a
   * plan touches nothing: no browser, no network, no file.
   */
  fillPlan: (reference: string, resume?: string | null) =>
    invoke<FillPlan>("fill_plan", { reference, resume: resume ?? null }),
  /**
   * Open the form and type the plan into it. This is the last thing Perch
   * does: the form is left open, filled in, with the person in front of it.
   *
   * Resolving means the window opened, not that anything was typed. The fill
   * runs in the page afterwards and says how it went through `onFillReport`. A
   * sentence comes back when a chosen file could not be read.
   */
  openAndFill: (reference: string, resume?: string | null) =>
    invoke<FillStarted>("open_and_fill", {
      reference,
      resume: resume ?? null,
    }),
  /** What the fill came to, once it has settled. */
  onFillReport: (heard: (report: FillReport) => void) =>
    listen<FillReport>("fill-report", (event) => heard(event.payload)),
  /**
   * What choosing a file would do, before one is chosen. The verdict this
   * carries is something to show. It is not a gate: `import_read` asks the
   * same question again in Rust, before the résumé is read off the disk.
   */
  importReady: () => invoke<ImportReady>("import_ready"),
  /** Read a résumé into proposed fields. Nothing is written by this. */
  importRead: (path: string) => invoke<ImportRead>("import_read", { path }),
  /** Write the accepted fields, and only those. */
  importWrite: (decisions: ImportDecision[]) =>
    invoke<ImportWritten>("import_write", { decisions }),
};

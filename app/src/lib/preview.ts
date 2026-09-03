/**
 * Dev-only preview data.
 *
 * The views talk to `perch-core` over Tauri's IPC, which does not exist in a
 * plain browser. This stands in for it during `npm run dev` so the interface
 * can be worked on without rebuilding the Rust side.
 *
 * Guarded by `import.meta.env.DEV` and by the absence of Tauri's own bridge, so
 * it is stripped from any real build and never runs inside the app. It mirrors
 * the DTOs in `app/src-tauri/src/lib.rs`; if the two ever disagree, this file
 * is the one that is wrong.
 */

const roles = [
  [
    "Oxide Computer",
    "Systems Software Engineer, Control Plane",
    "Emeryville, CA",
    "posted 4 hours ago",
    "fresh",
    "Today",
    "watched-company",
    "the company is Oxide Computer",
  ],
  [
    "Astral",
    "Rust Engineer, uv",
    "Remote (US or EU)",
    "posted 9 hours ago",
    "fresh",
    "Today",
    "rust-in-title",
    "'Rust' in the title",
  ],
  [
    "Fly.io",
    "Distributed Systems Engineer, Machines",
    "Remote (worldwide)",
    "posted yesterday",
    "recent",
    "This week",
    "systems-keywords",
    "'Distributed' in the title",
  ],
  [
    "Tigris Data",
    "Storage Engineer, Replication",
    "Remote (US)",
    "posted 3 days ago",
    "recent",
    "This week",
    "systems-keywords",
    "'Storage' in the title",
  ],
  [
    "Recurse Center",
    "Infrastructure Engineer",
    "Brooklyn, NY, or remote",
    "posted 5 days ago",
    "recent",
    "This week",
    "systems-keywords",
    "'Infrastructure' in the title",
  ],
  [
    "Modal",
    "Backend Engineer, Compute Platform",
    "San Francisco",
    "posted 12 days ago",
    "settled",
    "Earlier this month",
    "systems-keywords",
    "'Platform' in the title",
  ],
  [
    "Zed Industries",
    "Senior Systems Engineer, Collaboration",
    "Remote (US)",
    "reposted 3 weeks ago",
    "settled",
    "Earlier this month",
    "not-staff-plus",
    "the title avoids staff or principal",
  ],
  [
    "Sourcegraph",
    "Senior Software Engineer, Search Backend",
    "Remote (US)",
    "open 41 days",
    "stale",
    "Older",
    "seniority-band",
    "'Senior' in the title",
  ],
  [
    "Cursor",
    "Backend Engineer, Indexing Infrastructure",
    "San Francisco",
    "open 68 days",
    "stale",
    "Older",
    "systems-keywords",
    "'Infrastructure' in the title",
  ],
  [
    "Warp",
    "Software Engineer, Rust Platform",
    "San Francisco",
    "open 213 days",
    "tired",
    "Older",
    "rust-in-title",
    "'Rust' in the title",
  ],
] as const;

const feedRoles = roles.map(
  (
    [company, title, location, signal, freshness, bucket, rule, because],
    i,
  ) => ({
    reference: `preview${i}`,
    company,
    title,
    location,
    url: `https://example.invalid/${i}`,
    ats:
      company === "Recurse Center"
        ? "JSON-LD"
        : company === "Astral"
          ? "Ashby"
          : "Greenhouse",
    fillSupported: company !== "Recurse Center",
    signal,
    freshness,
    bucket,
    whyRule: rule,
    whyBecause: because,
  }),
);

const responses: Record<string, unknown> = {
  feed: {
    roles: feedRoles,
    openButUnmatched: 71,
    hasBoards: true,
    rulesError: null,
  },
  detail: {
    role: feedRoles[1],
    blocks: [
      {
        kind: "paragraph",
        text: "Astral builds Python tooling in Rust. uv is a package and project manager that has to be correct before it is fast, and fast enough that people stop thinking about it.",
      },
      {
        kind: "paragraph",
        text: "This role is on the resolver: the part that decides which versions of which packages can live together, and explains itself when they cannot. It is mostly unglamorous work with a very short feedback loop.",
      },
      { kind: "heading", text: "What we are looking for" },
      {
        kind: "bullet",
        text: "Rust in production, or a real willingness to write it daily",
      },
      {
        kind: "bullet",
        text: "Comfort reading someone else's code at two in the morning",
      },
      { kind: "bullet", text: "An opinion about error messages" },
    ],
    descriptionMissing: false,
    history: [
      { when: "in June", what: "Perch first saw it", latest: false },
      {
        when: "two weeks ago",
        what: "retitled: was 'Rust Engineer'",
        latest: false,
      },
      { when: "9 hours ago", what: "the board reposted it", latest: true },
    ],
    velocity: {
      openedRecently: 4,
      stillOpen: 2,
      medianDaysToClose: 41,
      caveat:
        "Observed, not reported. Perch has only been watching this board for 3 months.",
    },
    applied: null,
  },
  watchlist: [
    {
      company: "Oxide Computer",
      key: "oxidecomputer",
      ats: "Lever",
      url: "https://jobs.lever.co/oxidecomputer",
      checked: "checked 11 minutes ago",
      fillSupported: true,
      history: "3 roles seen since Perch started watching, 1 still open.",
    },
    {
      company: "Astral",
      key: "astral",
      ats: "Ashby",
      url: "https://jobs.ashbyhq.com/astral",
      checked: "checked an hour ago",
      fillSupported: true,
      history: "2 roles seen since Perch started watching, 2 still open.",
    },
    {
      company: "Recurse Center",
      key: "recursecenter",
      ats: "JSON-LD",
      url: "https://www.recurse.com/jobs",
      checked: "checked two hours ago",
      fillSupported: false,
      history: "1 role seen since Perch started watching, 1 still open.",
    },
    {
      company: "Warp",
      key: "warp",
      ats: "Greenhouse",
      url: "https://boards.greenhouse.io/warp",
      checked: "checked yesterday",
      fillSupported: true,
      history: "6 roles seen since Perch started watching, 1 still open.",
    },
  ],
  applications: [
    {
      reference: "preview2",
      company: "Fly.io",
      title: "Systems Engineer, Machines",
      state: "in_flight",
      heading: "In flight",
      applied: "applied 6 days ago",
      note: "confirmation email, same day",
      archivedQuietly: false,
      freshness: "recent",
    },
    {
      reference: "preview0",
      company: "Oxide Computer",
      title: "Software Engineer, Control Plane",
      state: "in_flight",
      heading: "In flight",
      applied: "applied 3 weeks ago",
      note: null,
      archivedQuietly: false,
      freshness: "settled",
    },
    {
      reference: "preview3",
      company: "Tigris Data",
      title: "Distributed Systems Engineer",
      state: "responded",
      heading: "Responded",
      applied: "applied 18 days ago",
      note: "recruiter screen on Thursday, 11am",
      archivedQuietly: false,
      freshness: "settled",
    },
    {
      reference: "preview9",
      company: "Warp",
      title: "Backend Engineer, Terminal Infrastructure",
      state: "responded",
      heading: "Responded",
      applied: "applied 22 days ago",
      note: "a no after the take-home, 6 days ago",
      archivedQuietly: false,
      freshness: "settled",
    },
    {
      reference: "preview5",
      company: "Modal",
      title: "Systems Engineer, Compute",
      state: "archived",
      heading: "Archived",
      applied: "applied 6 weeks ago",
      note: null,
      archivedQuietly: true,
      freshness: "stale",
    },
    {
      reference: "preview8",
      company: "Cursor",
      title: "Backend Engineer, Indexing",
      state: "archived",
      heading: "Archived",
      applied: "applied in May",
      note: null,
      archivedQuietly: true,
      freshness: "tired",
    },
  ],
  settings: {
    profile: {
      name: "Dana Ferreira",
      email: "dana@dferreira.dev",
      phone: "+1 415 555 0148",
      location: "Oakland, California (remote or the Bay Area)",
      work_authorisation: "US citizen. I don't need sponsorship now or later.",
      links: {
        github: "github.com/dferreira",
        website: "dferreira.dev",
        linkedin: "",
      },
      skills: ["Rust", "Go", "Tokio", "gRPC", "PostgreSQL"],
      experience: [
        {
          company: "Cloudflare",
          title: "Senior Software Engineer, Storage",
          dates: "March 2023 – February 2026",
        },
        {
          company: "Honeycomb",
          title: "Software Engineer, Infrastructure",
          dates: "August 2020 – February 2023",
        },
      ],
      documents: [
        {
          name: "resume-systems.pdf",
          path: "~/documents/resume-systems.pdf",
          kind: "résumé",
        },
        {
          name: "resume-platform.pdf",
          path: "~/documents/resume-platform.pdf",
          kind: "résumé",
        },
        {
          name: "letter-opening.md",
          path: "~/documents/letter-opening.md",
          kind: "letter",
        },
      ],
    },
    profilePath: "~/.config/perch/profile.toml",
    rulesPath: "~/.config/perch/rules.toml",
    databasePath: "~/.local/share/perch/perch.db",
    rules: [
      { name: "rust-in-title", conditions: ["title contains rust"] },
      {
        name: "systems-keywords",
        conditions: [
          "title contains distributed, storage, infrastructure or runtime",
          "title excludes staff, principal",
        ],
      },
      {
        name: "remote-ok",
        conditions: ["location contains remote or anywhere"],
      },
    ],
    rulesError: null,
    modelPath: "~/.config/perch/model.toml",
    preferredResume: "~/documents/resume-systems.pdf",
    model: {
      configured: true,
      name: "llama3.1:8b",
      endpoint: "http://localhost:11434/v1",
      consequence:
        "llama3.1:8b runs on this Mac, so your résumé does not leave it.",
      local: true,
      refused: false,
      onThisMac: ["llama3.1:8b", "qwen2.5:7b", "phi3:3.8b"],
      ollamaRunning: true,
    },
  },
  sync: {
    lines: ["Astral: 2 new", "Oxide Computer: 1 reposted"],
    failures: [],
    quiet: false,
    boardsRead: 4,
  },
  dismiss: null,
  restore: null,
  watch_add: "Val Town",
  watch_remove: true,
  mark_application: null,
  open_in_browser: null,
  // Astral's Ashby form, as `perch_fill` would plan it from the profile above
  // with resume-systems.pdf attached. Seven values, two boxes Perch will not
  // guess at, one that is hers to write, and four questions it refuses.
  fill_plan: {
    company: "Astral",
    title: "Rust Engineer, uv",
    fillable: true,
    whatHappensNext:
      "Perch opens Astral's Ashby form in a window, types the seven values below into it, and stops there. It will not press submit. There is no submit anywhere in this program. The form is left open, filled in, with you in front of it.",
    plan: {
      ats: "Ashby",
      url: "https://example.invalid/1",
      entries: [
        {
          label: "Name",
          action: "setText",
          selector: "input[name='_systemfield_name']",
          value: "Dana Ferreira",
          provenance: { from: "profile" },
        },
        {
          label: "Email",
          action: "setText",
          selector: "input[name='_systemfield_email']",
          value: "dana@dferreira.dev",
          provenance: { from: "profile" },
        },
        {
          label: "Phone",
          action: "setText",
          selector: "input[name='_systemfield_phone']",
          value: "+1 415 555 0148",
          provenance: { from: "profile" },
        },
        {
          label: "Location",
          action: "setText",
          selector: "input[name='_systemfield_location']",
          value: "Oakland, California (remote or the Bay Area)",
          provenance: { from: "profile" },
        },
        {
          label: "Résumé",
          action: "attachFile",
          selector: "input[name='_systemfield_resume']",
          path: "~/documents/resume-systems.pdf",
          provenance: { from: "document", name: "resume-systems.pdf" },
        },
        {
          label: "GitHub",
          action: "setText",
          selector: "input[name='q_github_url']",
          value: "github.com/dferreira",
          provenance: { from: "profile" },
        },
        {
          label: "Website",
          action: "setText",
          selector: "input[name='q_website']",
          value: "dferreira.dev",
          provenance: { from: "profile" },
        },
      ],
      flagged: [
        {
          label: "Preferred start date",
          selector: "input[name='q_start_date']",
          why: "Perch does not know your start date. This is left blank.",
        },
        {
          label: "How did you hear about us",
          selector: "input[name='q_how_did_you_hear']",
          why: "Perch does not know how you found this role. This is left blank.",
        },
      ],
      leftToYou: [
        {
          label: "Why do you want to work here",
          selector: "textarea[name='q_why_company']",
          why: "Perch does not answer this one for you.",
        },
      ],
      never: [
        {
          label: "Gender",
          why: "Perch does not fill demographic questions, and does not store answers to them.",
        },
        {
          label: "Race / ethnicity",
          why: "Perch does not fill demographic questions, and does not store answers to them.",
        },
        {
          label: "Veteran status",
          why: "Perch does not fill demographic questions, and does not store answers to them.",
        },
        {
          label: "Disability status",
          why: "Perch does not fill demographic questions, and does not store answers to them.",
        },
      ],
    },
  },
  open_and_fill: null,
  import_ready: {
    configured: true,
    verdict: "local",
    host: null,
    sentence: "llama3.1:8b runs on this Mac, so your résumé does not leave it.",
    remedy: null,
    profilePath: "~/.config/perch/profile.toml",
  },
  // The file picker is Tauri's own, so in a browser it stands in for the one
  // the person would have used.
  "plugin:dialog|open": "~/documents/resume-systems.pdf",
  // What `perch-llm` makes of that file: five values it can quote, one it read
  // rather than quoted, and one the document does not say at all.
  import_read: {
    document: "resume-systems.pdf",
    provenance: "Read on this Mac by llama3.1:8b. Nothing left this Mac.",
    profilePath: "~/.config/perch/profile.toml",
    nothingAnchored: null,
    proposals: [
      {
        id: 0,
        label: "Name",
        value: "Dana Ferreira",
        current: "D. Ferreira",
        quote: "DANA FERREIRA",
        highlight: [0, 13],
        line: 1,
        offerable: true,
        accepted: true,
        note: null,
        refusal: null,
        parts: null,
      },
      {
        id: 1,
        label: "Email",
        value: "dana@dferreira.dev",
        current: "dana@hey.com",
        quote: "dana@dferreira.dev · +1 (415) 555-0148 · github.com/dferreira",
        highlight: [0, 18],
        line: 2,
        offerable: true,
        accepted: true,
        note: null,
        refusal: null,
        parts: null,
      },
      {
        id: 2,
        label: "Phone",
        value: "+1 (415) 555-0148",
        current: "+1 415 555 0148",
        quote: "dana@dferreira.dev · +1 (415) 555-0148 · github.com/dferreira",
        highlight: [21, 38],
        line: 2,
        offerable: true,
        accepted: true,
        note: null,
        refusal: null,
        parts: null,
      },
      {
        id: 3,
        label: "Location",
        value: "San Francisco, CA",
        current: "Oakland, California (remote or the Bay Area)",
        quote: "SF Bay Area · open to remote",
        highlight: [0, 11],
        line: 3,
        offerable: true,
        accepted: false,
        note: "This one is read rather than quoted. The document says something close to it. Worth a look before accepting.",
        refusal: null,
        parts: null,
      },
      {
        id: 4,
        label: "GitHub",
        value: "github.com/dferreira",
        current: null,
        quote: "dana@dferreira.dev · +1 (415) 555-0148 · github.com/dferreira",
        highlight: [41, 61],
        line: 2,
        offerable: true,
        accepted: true,
        note: null,
        refusal: null,
        parts: null,
      },
      {
        id: 5,
        label: "Website",
        value: "dferreira.dev",
        current: null,
        quote: null,
        highlight: null,
        line: null,
        offerable: false,
        accepted: false,
        note: null,
        refusal:
          "No line in resume-systems.pdf says this, so Perch is not offering it.",
        parts: null,
      },
      {
        id: 6,
        label: "Employer: most recent",
        value:
          "Cloudflare · Senior Software Engineer, Storage · March 2023 to February 2026",
        current: null,
        quote:
          "Cloudflare — Senior Software Engineer, Storage\nMarch 2023 – February 2026 · remote",
        highlight: [0, 73],
        line: 6,
        offerable: true,
        accepted: true,
        note: null,
        refusal: null,
        parts: "company · title · dates",
      },
    ],
  },
  import_write: { written: 5, profilePath: "~/.config/perch/profile.toml" },
};

export function installPreview() {
  if (!import.meta.env.DEV) return;
  if ("__TAURI_INTERNALS__" in window) return;

  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    invoke: (cmd: string) =>
      // A short delay so loading copy is visible while it is being worked on.
      new Promise((resolve) =>
        setTimeout(() => resolve(responses[cmd] ?? null), 90),
      ),
    transformCallback: (cb: unknown) => cb,
  };
  document.documentElement.dataset.perchPreview = "on";
}

import { useCallback, useEffect, useState } from "react";
import { Rail } from "./components/Rail";
import { Palette, type Command } from "./components/Palette";
import { api } from "./lib/api";
import { FeedView } from "./views/Feed";
import { ApplyView } from "./views/Apply";
import { ApplicationsView } from "./views/Applications";
import { WatchlistView } from "./views/Watchlist";
import { ProfileView } from "./views/Profile";
import { ImportView } from "./views/Import";

export type View = "feed" | "applications" | "watchlist" | "profile" | "import";

export interface FeedOptions {
  fresh: boolean;
  all: boolean;
  company: string | null;
}

export function App() {
  const [view, setView] = useState<View>("feed");
  const [paletteOpen, setPaletteOpen] = useState(false);
  /** The role whose application sheet is open, over the dimmed feed. */
  const [applyRef, setApplyRef] = useState<string | null>(null);
  const [feedOptions, setFeedOptions] = useState<FeedOptions>({
    fresh: false,
    all: false,
    company: null,
  });
  const [lastSync, setLastSync] = useState("reading…");
  const [syncing, setSyncing] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);

  const refresh = useCallback(() => setRefreshKey((k) => k + 1), []);

  // The sheet belongs to the feed it opened over. Leaving for another view, by
  // the palette or the rail, closes it rather than stranding it.
  useEffect(() => {
    setApplyRef(null);
  }, [view]);

  // What the rail says before anything happens this session. The store knows
  // when the boards were last read; the interface does not, and saying "not
  // synced yet" from a fresh launch was simply wrong.
  useEffect(() => {
    let current = true;
    api
      .lastRead()
      .then((text) => {
        if (current) setLastSync(text);
      })
      .catch((err) => {
        if (current) setLastSync(String(err));
      });
    return () => {
      current = false;
    };
  }, []);

  const runSync = useCallback(async () => {
    if (syncing) return;
    setSyncing(true);
    setLastSync("reading the boards…");
    try {
      const result = await api.sync();
      setLastSync(
        result.quiet
          ? `read ${result.boardsRead} ${result.boardsRead === 1 ? "board" : "boards"}, nothing new`
          : result.lines.join(" · "),
      );
      refresh();
    } catch (err) {
      setLastSync(String(err));
    } finally {
      setSyncing(false);
    }
  }, [syncing, refresh]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((open) => !open);
      } else if (e.key === "Escape" && paletteOpen) {
        setPaletteOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [paletteOpen]);

  // The same words the CLI uses.
  const commands: Command[] = [
    {
      group: "Watching",
      cmd: "watch add <company>",
      desc: "detect ATS, start monitoring",
      run: () => setView("watchlist"),
    },
    {
      group: "Watching",
      cmd: "watch list",
      desc: "companies and their boards",
      run: () => setView("watchlist"),
    },
    {
      group: "Watching",
      cmd: "sync",
      desc: "poll every watched board now",
      run: runSync,
    },

    {
      group: "Feed",
      cmd: "feed",
      desc: "matched roles, newest first",
      run: () => {
        setFeedOptions({ fresh: false, all: false, company: null });
        setView("feed");
      },
    },
    {
      group: "Feed",
      cmd: "feed --fresh",
      desc: "posted in the last 24 hours",
      run: () => {
        setFeedOptions({ fresh: true, all: false, company: null });
        setView("feed");
      },
    },
    {
      group: "Feed",
      cmd: "feed --all",
      desc: "every open role, rules aside",
      run: () => {
        setFeedOptions({ fresh: false, all: true, company: null });
        setView("feed");
      },
    },

    {
      group: "Applying",
      cmd: "apps",
      desc: "in flight, responded, archived",
      run: () => setView("applications"),
    },

    {
      group: "Profile",
      cmd: "profile edit",
      desc: "open profile.toml",
      run: () => setView("profile"),
    },
    {
      group: "Profile",
      cmd: "profile import",
      desc: "propose profile fields from a résumé",
      run: () => setView("import"),
    },
    {
      group: "Rules",
      cmd: "rules list",
      desc: "the rules as Perch reads them",
      run: () => setView("profile"),
    },
  ];

  return (
    <>
      <div className="app">
        <Rail
          // Reading a résumé is something the profile does, so the rail keeps
          // saying Profile while it is on screen.
          view={view === "import" ? "profile" : view}
          onNavigate={setView}
          onCommands={() => setPaletteOpen(true)}
          lastSync={lastSync}
        />
        {view === "feed" && (
          <FeedView
            key={`feed-${refreshKey}`}
            options={feedOptions}
            onOptions={setFeedOptions}
            onSync={runSync}
            syncing={syncing}
            onChanged={refresh}
            onApply={setApplyRef}
            applyOpen={applyRef !== null}
          />
        )}
        {view === "applications" && (
          <ApplicationsView key={`apps-${refreshKey}`} onChanged={refresh} />
        )}
        {view === "watchlist" && (
          <WatchlistView key={`watch-${refreshKey}`} onChanged={refresh} />
        )}
        {view === "profile" && (
          <ProfileView
            key={`profile-${refreshKey}`}
            onImport={() => setView("import")}
          />
        )}
        {/* No refresh key: writing the profile refreshes the other views, and
            remounting this one would throw away what it has just said. */}
        {view === "import" && <ImportView onWritten={refresh} />}
      </div>
      {applyRef !== null && (
        <ApplyView reference={applyRef} onClose={() => setApplyRef(null)} />
      )}
      <Palette
        open={paletteOpen}
        commands={commands}
        onClose={() => setPaletteOpen(false)}
      />
    </>
  );
}

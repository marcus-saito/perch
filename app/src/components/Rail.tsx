import type { ReactNode } from "react";
import type { View } from "../App";

const items: { view: View; label: string; path: ReactNode }[] = [
  { view: "feed", label: "Feed", path: <path d="M4 5h16M4 12h16M4 19h10" /> },
  {
    view: "applications",
    label: "Applications",
    path: (
      <>
        <path d="M5 4h9l5 5v11a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z" />
        <path d="M14 4v5h5" />
      </>
    ),
  },
  {
    view: "watchlist",
    label: "Watchlist",
    path: (
      <>
        <circle cx="12" cy="12" r="3" />
        <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6z" />
      </>
    ),
  },
  {
    view: "profile",
    label: "Profile",
    path: (
      <>
        <circle cx="12" cy="8" r="3.5" />
        <path d="M4.5 20a7.5 7.5 0 0 1 15 0" />
      </>
    ),
  },
];

export function Rail({
  view,
  onNavigate,
  onCommands,
  lastSync,
}: {
  view: View;
  onNavigate: (v: View) => void;
  onCommands: () => void;
  lastSync: string;
}) {
  return (
    <nav className="rail">
      <div className="wordmark">
        Perch<span className="dot" />
      </div>
      {items.map((item) => (
        <button
          key={item.view}
          className={`rail-item ${view === item.view ? "is-active" : ""}`}
          onClick={() => onNavigate(item.view)}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round">
            {item.path}
          </svg>
          {item.label}
        </button>
      ))}
      <button className="rail-item" onClick={onCommands}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round">
          <path d="M4 17l6-6-6-6M12 19h8" />
        </svg>
        Command
        <span style={{ marginLeft: "auto" }} className="kbd">⌘K</span>
      </button>
      <div className="rail-foot">
        <div className="line">
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: "var(--ink-4)", display: "inline-block" }} />
          {lastSync}
        </div>
        <div className="line">everything stays on this Mac</div>
      </div>
    </nav>
  );
}

export interface Hint {
  keys: string[];
  label: string;
}

/**
 * Only the keys that do something on this screen, in the contract's order:
 * movement, open, screen verbs, x, Esc, ⌘K.
 */
export function HintBar({ hints, undoNote }: { hints: Hint[]; undoNote?: string | null }) {
  return (
    <div className="hint-bar">
      {hints.map((hint) => (
        <span className="hint" key={hint.label}>
          {hint.keys.map((k) => (
            <span className="kbd" key={k}>{k}</span>
          ))}
          {hint.label}
        </span>
      ))}
      {undoNote && (
        <span className="undo-note is-on">
          {undoNote}. <span className="kbd">u</span> to undo
        </span>
      )}
      <span className="spacer" />
      <span className="hint">
        <span className="kbd">⌘K</span> commands
      </span>
    </div>
  );
}

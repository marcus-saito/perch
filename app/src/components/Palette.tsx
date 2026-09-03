import { useEffect, useMemo, useRef, useState } from "react";

/**
 * The palette's vocabulary is the CLI's vocabulary, verbatim. A command the
 * person learns here works in the terminal, and the other way round.
 */
export interface Command {
  group: string;
  cmd: string;
  desc: string;
  run?: () => void;
}

export function Palette({
  open,
  commands,
  onClose,
}: {
  open: boolean;
  commands: Command[];
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return commands.filter(
      (c) => !needle || c.cmd.toLowerCase().includes(needle) || c.desc.toLowerCase().includes(needle),
    );
  }, [commands, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      input.current?.focus();
    }
  }, [open]);

  useEffect(() => {
    setActive((a) => Math.min(a, Math.max(shown.length - 1, 0)));
  }, [shown.length]);

  if (!open) return null;

  const run = (command: Command | undefined) => {
    onClose();
    command?.run?.();
  };

  let lastGroup: string | null = null;

  return (
    <div className="palette-scrim is-open" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-label="Command palette">
        <input
          ref={input}
          className="palette-input"
          placeholder="Type a command…"
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((a) => Math.min(a + 1, shown.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            } else if (e.key === "Escape") {
              onClose();
            } else if (e.key === "Enter") {
              run(shown[active]);
            }
          }}
        />
        <div className="palette-list">
          {shown.length === 0 && <div className="palette-group">no command by that name</div>}
          {shown.map((c, i) => {
            const heading = c.group !== lastGroup ? c.group : null;
            lastGroup = c.group;
            return (
              <div key={c.cmd}>
                {heading && <div className="palette-group">{heading}</div>}
                <div
                  className={`palette-item ${i === active ? "is-active" : ""}`}
                  onMouseMove={() => setActive(i)}
                  onClick={() => run(c)}
                >
                  <span className="cmd">{c.cmd}</span>
                  <span className="desc">{c.desc}</span>
                </div>
              </div>
            );
          })}
        </div>
        <div className="palette-foot">
          <span><span className="kbd">↑</span> <span className="kbd">↓</span> move</span>
          <span><span className="kbd">↵</span> run</span>
          <span><span className="kbd">esc</span> close</span>
          <span style={{ marginLeft: "auto" }}>same words as the CLI</span>
        </div>
      </div>
    </div>
  );
}

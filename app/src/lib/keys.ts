import { useCallback, useEffect, useRef, useState } from "react";

/**
 * The keyboard contract, in one place.
 *
 *   j / k   move        Enter  open
 *   x       set aside   u      undo the last x
 *   Esc     close       ⌘K     commands
 *
 * `x` never deletes. Each queue says where the row went and hands back a way
 * to put it back, so `u` always has something to undo.
 */
export interface SetAside {
  /** Past-tense verb for the hint bar: "dismissed", "archived". */
  verb: string;
  run: (reference: string) => Promise<() => Promise<void>>;
}

export function useQueue<T extends { reference: string }>(
  rows: T[],
  opts: {
    onOpen?: (row: T) => void;
    setAside?: SetAside;
    onChanged?: () => void;
    enabled?: boolean;
  } = {},
) {
  const [selected, setSelected] = useState<string | null>(null);
  const [undoNote, setUndoNote] = useState<string | null>(null);
  const undoStack = useRef<(() => Promise<void>)[]>([]);
  const noteTimer = useRef<number | undefined>(undefined);

  // Keep a selection as the list changes, without stealing one that is valid.
  useEffect(() => {
    if (rows.length === 0) {
      setSelected(null);
      return;
    }
    setSelected((current) =>
      current && rows.some((r) => r.reference === current) ? current : rows[0].reference,
    );
  }, [rows]);

  const move = useCallback(
    (delta: number) => {
      if (rows.length === 0) return;
      const at = rows.findIndex((r) => r.reference === selected);
      const next = Math.min(Math.max((at < 0 ? -1 : at) + delta, 0), rows.length - 1);
      setSelected(rows[next].reference);
      document
        .querySelector(`[data-row="${rows[next].reference}"]`)
        ?.scrollIntoView({ block: "nearest" });
    },
    [rows, selected],
  );

  const note = useCallback((text: string) => {
    setUndoNote(text);
    window.clearTimeout(noteTimer.current);
    noteTimer.current = window.setTimeout(() => setUndoNote(null), 6000);
  }, []);

  useEffect(() => {
    if (opts.enabled === false) return;
    const onKey = async (e: KeyboardEvent) => {
      const el = document.activeElement;
      if (el instanceof HTMLElement && (el.isContentEditable || /INPUT|TEXTAREA|SELECT/.test(el.tagName))) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      if (e.key === "j" || e.key === "ArrowDown") {
        e.preventDefault();
        move(1);
      } else if (e.key === "k" || e.key === "ArrowUp") {
        e.preventDefault();
        move(-1);
      } else if (e.key === "Enter" && opts.onOpen) {
        const row = rows.find((r) => r.reference === selected);
        if (row) {
          e.preventDefault();
          opts.onOpen(row);
        }
      } else if (e.key === "x" && opts.setAside && selected) {
        e.preventDefault();
        const undo = await opts.setAside.run(selected);
        undoStack.current.push(undo);
        note(opts.setAside.verb);
        opts.onChanged?.();
      } else if (e.key === "u" && undoStack.current.length > 0) {
        e.preventDefault();
        const undo = undoStack.current.pop()!;
        await undo();
        setUndoNote(null);
        opts.onChanged?.();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [rows, selected, move, note, opts]);

  return { selected, setSelected, undoNote };
}

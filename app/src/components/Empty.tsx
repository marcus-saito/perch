import type { ReactNode } from "react";

/**
 * Empty states read as fine, never as failure. They say what is true, say it is
 * normal, and name the one thing that would change it.
 */
export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <h2 className="empty-title">{title}</h2>
      <div className="empty-body">{children}</div>
    </div>
  );
}

import React from "react";
import ReactDOM from "react-dom/client";
import "./perch.css";
import "./app.css";
import { App } from "./App";

function mount() {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

// In a plain browser during `npm run dev` there is no Rust side to talk to, so
// a preview stands in for it. The import is inside the guard so the fixture
// data is dropped from a real build rather than merely unreachable in it.
if (import.meta.env.DEV) {
  import("./lib/preview")
    .then((m) => m.installPreview())
    .finally(mount);
} else {
  mount();
}

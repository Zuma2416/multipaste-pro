import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Hud from "./Hud";
import Picker from "./Picker";
import Preferences from "./Preferences";

const view = new URLSearchParams(window.location.search).get("view");

function Root() {
  if (view === "hud") return <Hud />;
  if (view === "picker") return <Picker />;
  if (view === "preferences") return <Preferences />;
  return <App />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);

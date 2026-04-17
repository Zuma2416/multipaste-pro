import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "./Hud.css";
import type { ActionResult } from "./types";

type HudState = {
  message: string;
  slotCount: number;
  ok: boolean;
  visible: boolean;
};

const DISPLAY_MS = 2000;
const FADE_MS = 250;

export default function Hud() {
  const [hud, setHud] = useState<HudState>({
    message: "",
    slotCount: 0,
    ok: true,
    visible: false,
  });
  const hideTimerRef = useRef<number | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    void listen<ActionResult>("multipaste://slots-updated", (event) => {
      const { message, slotCount, ok } = event.payload;

      if (hideTimerRef.current) {
        window.clearTimeout(hideTimerRef.current);
      }

      setHud({ message, slotCount, ok, visible: true });

      hideTimerRef.current = window.setTimeout(() => {
        setHud((prev) => ({ ...prev, visible: false }));
      }, DISPLAY_MS);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (hideTimerRef.current) window.clearTimeout(hideTimerRef.current);
      if (unlisten) unlisten();
    };
  }, []);

  return (
    <div
      className={`hud-root ${hud.visible ? "is-visible" : ""}`}
      style={{ "--fade-ms": `${FADE_MS}ms` } as React.CSSProperties}
    >
      <div className={`hud-pill ${hud.ok ? "" : "is-error"}`}>
        <span className="hud-icon">{hud.ok ? "⌘" : "!"}</span>
        <span className="hud-message">{hud.message}</span>
        {hud.ok && (
          <span className="hud-badge">{hud.slotCount} / 10</span>
        )}
      </div>
    </div>
  );
}

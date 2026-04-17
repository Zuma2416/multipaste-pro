import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./Picker.css";
import type { ActionResult, PasteSlot } from "./types";
import { formatSource } from "./types";

const pickerWindow = getCurrentWebviewWindow();

// スロット番号に対応するキー（1〜9, 0 で 10 番目）
const SLOT_KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

export default function Picker() {
  const [slots, setSlots] = useState<PasteSlot[]>([]);
  const [visible, setVisible] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  const close = async () => {
    setVisible(false);
    await pickerWindow.hide();
  };

  const pasteAt = async (index: number) => {
    if (index < 0 || index >= slots.length) return;
    try {
      await invoke<ActionResult>("paste_slot_by_index", { index });
    } catch {
      // エラーは HUD 側のトーストで通知される
    }
    setVisible(false);
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    void listen<PasteSlot[]>("multipaste://picker-show", (event) => {
      setSlots(event.payload);
      setActiveIndex(0);
      setVisible(true);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  useEffect(() => {
    const onKeyDown = async (e: KeyboardEvent) => {
      if (!visible) return;

      if (e.key === "Escape") {
        await close();
        return;
      }

      // 数字キーで即選択
      const numIndex = SLOT_KEYS.indexOf(e.key);
      if (numIndex !== -1 && numIndex < slots.length) {
        e.preventDefault();
        await pasteAt(numIndex);
        return;
      }

      // 矢印キーでフォーカス移動
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIndex((prev) => Math.min(prev + 1, slots.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIndex((prev) => Math.max(prev - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        await pasteAt(activeIndex);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [visible, slots, activeIndex]);

  // フォーカス中のアイテムをスクロール表示
  useEffect(() => {
    const item = listRef.current?.children[activeIndex] as HTMLElement | undefined;
    item?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  return (
    <div className={`picker-root ${visible ? "is-visible" : ""}`}>
      <div className="picker-panel" role="dialog" aria-modal="true">
        <div className="picker-header">
          <span className="picker-title">ペーストするスロットを選択</span>
          <span className="picker-hint">↑↓ / 数字キー / Enter</span>
        </div>

        {slots.length === 0 ? (
          <div className="picker-empty">スロットがありません</div>
        ) : (
          <ul className="picker-list" ref={listRef}>
            {slots.map((slot, i) => (
              <li
                key={slot.id}
                className={`picker-item ${i === activeIndex ? "is-active" : ""}`}
                onClick={() => void pasteAt(i)}
                onMouseEnter={() => setActiveIndex(i)}
              >
                <span className="picker-key">{SLOT_KEYS[i]}</span>
                <div className="picker-item-body">
                  <span className="picker-item-title">{slot.title}</span>
                  <span className="picker-item-preview">{slot.content}</span>
                </div>
                <span className="picker-item-source">{formatSource(slot.source)}</span>
              </li>
            ))}
          </ul>
        )}

        <div className="picker-footer">
          <span className="picker-footer-hint">Esc でキャンセル</span>
        </div>
      </div>
    </div>
  );
}

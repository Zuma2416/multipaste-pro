import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./Preferences.css";
import type { Preferences as PreferencesType } from "./types";

/** ショートカットキー入力フィールドのキャプチャモード用フラグ */
type CapturingField =
  | "shortcutCopy"
  | "shortcutPaste"
  | "shortcutRangeStart"
  | "shortcutRangeEnd"
  | null;

/** キーボードイベントからショートカット文字列を組み立てる */
function buildShortcutString(e: React.KeyboardEvent): string | null {
  // 修飾キー単体の押下は無視
  if (["Meta", "Alt", "Control", "Shift"].includes(e.key)) {
    return null;
  }

  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("CommandOrControl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");

  // キー名の正規化
  let key = e.key;
  if (key.length === 1) {
    key = key.toUpperCase();
  } else {
    // 特殊キーのマッピング
    const keyMap: Record<string, string> = {
      "[": "BracketLeft",
      "]": "BracketRight",
      ArrowUp: "Up",
      ArrowDown: "Down",
      ArrowLeft: "Left",
      ArrowRight: "Right",
      Backspace: "Backspace",
      Delete: "Delete",
      Enter: "Return",
      Escape: "Escape",
      Tab: "Tab",
      " ": "Space",
    };
    key = keyMap[key] ?? key;
  }

  parts.push(key);

  // 修飾キーが1つもない場合は無効
  if (parts.length < 2) return null;

  return parts.join("+");
}

/** ショートカット文字列を人間に読みやすい表示に変換 */
function formatShortcut(shortcut: string): string {
  return shortcut
    .replace("CommandOrControl", "\u2318")
    .replace("Alt", "\u2325")
    .replace("Shift", "\u21E7")
    .replace("BracketLeft", "[")
    .replace("BracketRight", "]")
    .replace(/\+/g, " ");
}

function Preferences() {
  const [prefs, setPrefs] = useState<PreferencesType | null>(null);
  const [capturing, setCapturing] = useState<CapturingField>(null);
  const [error, setError] = useState("");

  const loadPreferences = useCallback(async () => {
    try {
      const result = await invoke<PreferencesType>("get_preferences");
      setPrefs(result);
      setError("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "設定の読み込みに失敗しました。");
    }
  }, []);

  useEffect(() => {
    void loadPreferences();
  }, [loadPreferences]);

  const updatePref = useCallback(
    async (key: string, value: string) => {
      try {
        const result = await invoke<PreferencesType>("update_preference", {
          key,
          value,
        });
        setPrefs(result);
        setError("");
      } catch (e) {
        setError(e instanceof Error ? e.message : "設定の更新に失敗しました。");
      }
    },
    [],
  );

  const handleShortcutKeyDown = useCallback(
    (field: CapturingField, e: React.KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape") {
        setCapturing(null);
        return;
      }

      const shortcut = buildShortcutString(e);
      if (shortcut && field) {
        setCapturing(null);
        void updatePref(field, shortcut);
      }
    },
    [updatePref],
  );

  const handleSlotCapacityChange = useCallback(
    async (delta: number) => {
      if (!prefs) return;
      const next = Math.max(1, Math.min(30, prefs.slotCapacity + delta));
      if (next === prefs.slotCapacity) return;

      // 上限を減らす場合、現在のスロット数を確認
      if (next < prefs.slotCapacity) {
        try {
          const slots = await invoke<unknown[]>("list_slot_previews");
          if (slots.length > next) {
            const excess = slots.length - next;
            const ok = window.confirm(
              `現在 ${slots.length} 件のスロットがあります。\n上限を ${next} に減らすと、古い ${excess} 件が削除されます。\n続けますか？`
            );
            if (!ok) return;
          }
        } catch {
          // スロット数取得失敗時はそのまま進める
        }
      }

      void updatePref("slotCapacity", String(next));
    },
    [prefs, updatePref],
  );

  if (!prefs) {
    return (
      <div className="pref-shell">
        <p className="pref-loading">読み込み中...</p>
      </div>
    );
  }

  return (
    <div className="pref-shell">
      <h1 className="pref-title">設定</h1>

      {error && <p className="pref-error">{error}</p>}

      {/* ショートカットセクション */}
      <section className="pref-section">
        <h2 className="pref-section-title">ショートカットキー</h2>
        <div className="pref-group">
          <ShortcutRow
            label="コピー"
            field="shortcutCopy"
            value={prefs.shortcutCopy}
            capturing={capturing}
            onStartCapture={setCapturing}
            onKeyDown={handleShortcutKeyDown}
          />
          <ShortcutRow
            label="ペースト"
            field="shortcutPaste"
            value={prefs.shortcutPaste}
            capturing={capturing}
            onStartCapture={setCapturing}
            onKeyDown={handleShortcutKeyDown}
          />
          <ShortcutRow
            label="範囲選択開始"
            field="shortcutRangeStart"
            value={prefs.shortcutRangeStart}
            capturing={capturing}
            onStartCapture={setCapturing}
            onKeyDown={handleShortcutKeyDown}
          />
          <ShortcutRow
            label="範囲選択終了"
            field="shortcutRangeEnd"
            value={prefs.shortcutRangeEnd}
            capturing={capturing}
            onStartCapture={setCapturing}
            onKeyDown={handleShortcutKeyDown}
          />
        </div>
      </section>

      {/* スロットセクション */}
      <section className="pref-section">
        <h2 className="pref-section-title">スロット</h2>
        <div className="pref-group">
          <div className="pref-row">
            <span className="pref-label">スロット上限</span>
            <div className="pref-stepper">
              <button
                type="button"
                className="stepper-btn"
                onClick={() => handleSlotCapacityChange(-1)}
                disabled={prefs.slotCapacity <= 1}
              >
                -
              </button>
              <span className="stepper-value">{prefs.slotCapacity}</span>
              <button
                type="button"
                className="stepper-btn"
                onClick={() => handleSlotCapacityChange(1)}
                disabled={prefs.slotCapacity >= 30}
              >
                +
              </button>
            </div>
          </div>
        </div>
      </section>

      {/* 動作セクション */}
      <section className="pref-section">
        <h2 className="pref-section-title">動作</h2>
        <div className="pref-group">
          <div className="pref-row">
            <span className="pref-label">デフォルトペーストモード</span>
            <div className="pref-radio-group">
              <label className="pref-radio">
                <input
                  type="radio"
                  name="pasteMode"
                  checked={prefs.defaultPasteMode === "consume"}
                  onChange={() => void updatePref("defaultPasteMode", "consume")}
                />
                <span>消費</span>
              </label>
              <label className="pref-radio">
                <input
                  type="radio"
                  name="pasteMode"
                  checked={prefs.defaultPasteMode === "keep"}
                  onChange={() => void updatePref("defaultPasteMode", "keep")}
                />
                <span>保持</span>
              </label>
            </div>
          </div>
          <div className="pref-row">
            <span className="pref-label">効果音</span>
            <button
              type="button"
              className={`pref-toggle ${prefs.soundEnabled ? "is-on" : ""}`}
              onClick={() =>
                void updatePref("soundEnabled", String(!prefs.soundEnabled))
              }
            >
              <span className="toggle-knob" />
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

function ShortcutRow({
  label,
  field,
  value,
  capturing,
  onStartCapture,
  onKeyDown,
}: {
  label: string;
  field: NonNullable<CapturingField>;
  value: string;
  capturing: CapturingField;
  onStartCapture: (field: CapturingField) => void;
  onKeyDown: (field: CapturingField, e: React.KeyboardEvent) => void;
}) {
  const isCapturing = capturing === field;

  return (
    <div className="pref-row">
      <span className="pref-label">{label}</span>
      <button
        type="button"
        className={`shortcut-input ${isCapturing ? "is-capturing" : ""}`}
        onClick={() => onStartCapture(isCapturing ? null : field)}
        onKeyDown={(e) => {
          if (isCapturing) {
            onKeyDown(field, e);
          }
        }}
      >
        {isCapturing ? "キーを入力..." : formatShortcut(value)}
      </button>
    </div>
  );
}

export default Preferences;

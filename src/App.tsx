import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import "./App.css";
import type { ActionResult, AppOverview, PasteSlot } from "./types";
import { formatSource } from "./types";

type ToastState = {
  visible: boolean;
  message: string;
  ok: boolean;
};

type UpdateStatus =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "available"; version: string }
  | { phase: "downloading" }
  | { phase: "ready" }
  | { phase: "up-to-date" }
  | { phase: "error"; message: string };

function App() {
  const [overview, setOverview] = useState<AppOverview | null>(null);
  const [slots, setSlots] = useState<PasteSlot[]>([]);
  const [errorMessage, setErrorMessage] = useState("");
  const [notificationGranted, setNotificationGranted] = useState<boolean | null>(null);
  const [toast, setToast] = useState<ToastState>({ visible: false, message: "", ok: true });
  const toastTimerRef = useRef<number | null>(null);
  const [appVersion, setAppVersion] = useState("");
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ phase: "idle" });

  const showToast = useCallback((message: string, ok: boolean) => {
    setToast({ visible: true, message, ok });
    if (toastTimerRef.current) {
      window.clearTimeout(toastTimerRef.current);
    }
    toastTimerRef.current = window.setTimeout(() => {
      setToast((current) => ({ ...current, visible: false }));
    }, 2600);
  }, []);

  const refreshState = async () => {
    const [overviewResult, slotResult] = await Promise.all([
      invoke<AppOverview>("get_app_overview"),
      invoke<PasteSlot[]>("list_slot_previews"),
    ]);
    setOverview(overviewResult);
    setSlots(slotResult);
  };

  useEffect(() => {
    let isMounted = true;

    const init = async () => {
      try {
        await refreshState();
        if (!isMounted) return;
      } catch (error) {
        if (!isMounted) return;
        setErrorMessage(
          error instanceof Error ? error.message : "バックエンドから状態を取得できませんでした。",
        );
      }

      // 通知パーミッション確認は別途実行（失敗してもメイン機能に影響しない）
      try {
        const granted = await isPermissionGranted();
        if (!isMounted) return;
        setNotificationGranted(granted);
      } catch {
        // 通知パーミッション確認に失敗しても無視する
      }

      // アプリバージョン取得
      try {
        const ver = await getVersion();
        if (!isMounted) return;
        setAppVersion(ver);
      } catch {
        // バージョン取得に失敗しても無視する
      }
    };

    let unlisten: (() => void) | undefined;
    let unlistenUpdate: (() => void) | undefined;

    void init();
    void listen<ActionResult>("multipaste://slots-updated", async (event) => {
      if (!isMounted) return;
      showToast(event.payload.message, event.payload.ok);
      await refreshState();
    }).then((cleanup) => {
      unlisten = cleanup;
    });

    // トレイメニューからの更新確認トリガー
    void listen("check-update", () => {
      if (!isMounted) return;
      void checkForUpdate();
    }).then((cleanup) => {
      unlistenUpdate = cleanup;
    });

    return () => {
      isMounted = false;
      if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
      if (unlisten) unlisten();
      if (unlistenUpdate) unlistenUpdate();
    };
  }, [showToast]);

  const deleteSlot = async (index: number) => {
    try {
      const result = await invoke<ActionResult>("delete_slot", { index });
      showToast(result.message, result.ok);
      setErrorMessage("");
      await refreshState();
    } catch (error) {
      const message = error instanceof Error ? error.message : "削除に失敗しました。";
      setErrorMessage(message);
      showToast(message, false);
    }
  };

  const runAction = async (
    command: "capture_clipboard_now" | "paste_next_slot_now" | "clear_slots",
  ) => {
    try {
      const result = await invoke<ActionResult>(command);
      showToast(result.message, result.ok);
      setErrorMessage("");
      await refreshState();
    } catch (error) {
      const message = error instanceof Error ? error.message : "操作の実行に失敗しました。";
      setErrorMessage(message);
      showToast(message, false);
    }
  };

  const checkForUpdate = async () => {
    setUpdateStatus({ phase: "checking" });
    try {
      const update = await check();
      if (update) {
        setUpdateStatus({ phase: "available", version: update.version });
      } else {
        setUpdateStatus({ phase: "up-to-date" });
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "更新の確認に失敗しました。";
      setUpdateStatus({ phase: "error", message });
    }
  };

  const downloadAndInstallUpdate = async () => {
    setUpdateStatus({ phase: "downloading" });
    try {
      const update = await check();
      if (!update) {
        setUpdateStatus({ phase: "up-to-date" });
        return;
      }
      await update.downloadAndInstall();
      setUpdateStatus({ phase: "ready" });
    } catch (error) {
      const message = error instanceof Error ? error.message : "更新のダウンロードに失敗しました。";
      setUpdateStatus({ phase: "error", message });
    }
  };

  const handleRelaunch = async () => {
    try {
      await relaunch();
    } catch (error) {
      const message = error instanceof Error ? error.message : "再起動に失敗しました。";
      setUpdateStatus({ phase: "error", message });
    }
  };

  const togglePasteMode = async () => {
    if (!overview) return;
    const nextMode = overview.pasteMode === "consume" ? "keep" : "consume";
    try {
      const result = await invoke<ActionResult>("set_paste_mode", { mode: nextMode });
      showToast(result.message, result.ok);
      setErrorMessage("");
      await refreshState();
    } catch (error) {
      const message = error instanceof Error ? error.message : "モード変更に失敗しました。";
      setErrorMessage(message);
      showToast(message, false);
    }
  };

  const requestNotificationAccess = async () => {
    try {
      const result = await requestPermission();
      const granted = result === "granted";
      setNotificationGranted(granted);
      showToast(
        granted ? "OS 通知を利用できるようになりました。" : "OS 通知はまだ許可されていません。",
        granted,
      );
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "通知権限の要求に失敗しました。";
      setErrorMessage(message);
      showToast(message, false);
    }
  };

  const pasteMode = overview?.pasteMode ?? "consume";
  const slotCapacity = overview?.slotCapacity ?? 10;
  const poc = overview?.poc;

  return (
    <div className="app-shell">
      {/* Toast */}
      <div
        className={`toast ${toast.visible ? "is-visible" : ""} ${toast.ok ? "is-success" : "is-error"}`}
      >
        {toast.message}
      </div>

      {/* Header */}
      <header className="app-header">
        <div className="app-brand">
          <span className="app-name">MultiPaste Pro</span>
          <div className="poc-indicators">
            <PocDot label="クリップボード" active={poc?.clipboardRead ?? false} />
            <PocDot label="キー監視" active={poc?.globalKeyListening ?? false} />
            <PocDot label="ペースト送信" active={poc?.pasteSimulation ?? false} />
          </div>
        </div>
        <button
          type="button"
          className={`mode-toggle ${pasteMode === "keep" ? "is-keep" : ""}`}
          onClick={() => void togglePasteMode()}
          title="クリックでモードを切り替え"
        >
          <span className="mode-icon">{pasteMode === "consume" ? "↓" : "○"}</span>
          {pasteMode === "consume" ? "消費モード" : "保持モード"}
        </button>
      </header>

      {/* Primary actions */}
      <section className="primary-actions">
        <button
          type="button"
          className="action-btn capture-btn"
          onClick={() => void runAction("capture_clipboard_now")}
        >
          <span className="shortcut-badge">⌘ ⌥ C</span>
          <span className="action-label">クリップボードを保存</span>
        </button>
        <button
          type="button"
          className="action-btn paste-btn"
          onClick={() => void runAction("paste_next_slot_now")}
          disabled={slots.length === 0}
        >
          <span className="shortcut-badge">⌘ ⌥ V</span>
          <span className="action-label">次のスロットをペースト</span>
        </button>
      </section>

      {/* Slot list */}
      <section className="slots-section">
        <div className="slots-header">
          <h2 className="slots-title">スロット一覧</h2>
          <span className="slot-count-badge">
            {slots.length}
            <span className="slot-count-sep"> / </span>
            {slotCapacity}
          </span>
        </div>

        <div className="slot-grid">
          {Array.from({ length: slotCapacity }).map((_, i) => {
            const slot = slots[i];
            return slot ? (
              <article className="slot-card is-filled" key={slot.id}>
                <div className="slot-meta">
                  <span className="slot-index">{String(i + 1).padStart(2, "0")}</span>
                  <span className="slot-source">{formatSource(slot.source)}</span>
                </div>
                <p className="slot-content">{slot.content}</p>
                <button
                  type="button"
                  className="slot-delete-btn"
                  onClick={() => void deleteSlot(i)}
                  title="このスロットを削除"
                >
                  ×
                </button>
              </article>
            ) : (
              <div className="slot-card is-empty" key={`empty-${i}`}>
                <span className="slot-index">{String(i + 1).padStart(2, "0")}</span>
              </div>
            );
          })}
        </div>
      </section>

      {/* Footer */}
      <footer className="app-footer">
        <div className="footer-left">
          {errorMessage ? (
            <span className="error-text">{errorMessage}</span>
          ) : (
            <span className={`notif-status ${notificationGranted ? "is-granted" : ""}`}>
              <span className="notif-dot" />
              {notificationGranted ? "通知許可済み" : "通知未許可"}
            </span>
          )}
        </div>
        <div className="footer-actions">
          {notificationGranted === false && (
            <button
              type="button"
              className="ghost-btn"
              onClick={() => void requestNotificationAccess()}
            >
              通知を有効化
            </button>
          )}
          <button
            type="button"
            className="ghost-btn danger-btn"
            onClick={() => void runAction("clear_slots")}
            disabled={slots.length === 0}
          >
            クリア
          </button>
        </div>
      </footer>

      {/* Update bar */}
      <div className="update-bar">
        <span className="update-version">v{appVersion || "..."}</span>
        {updateStatus.phase === "idle" && (
          <button
            type="button"
            className="ghost-btn update-check-btn"
            onClick={() => void checkForUpdate()}
          >
            アップデートを確認
          </button>
        )}
        {updateStatus.phase === "checking" && (
          <span className="update-status-text">確認中...</span>
        )}
        {updateStatus.phase === "available" && (
          <div className="update-available">
            <span className="update-status-text">
              v{updateStatus.version} が利用可能です
            </span>
            <button
              type="button"
              className="ghost-btn update-action-btn"
              onClick={() => void downloadAndInstallUpdate()}
            >
              更新する
            </button>
          </div>
        )}
        {updateStatus.phase === "downloading" && (
          <span className="update-status-text">ダウンロード中...</span>
        )}
        {updateStatus.phase === "ready" && (
          <button
            type="button"
            className="ghost-btn update-action-btn"
            onClick={() => void handleRelaunch()}
          >
            再起動して更新を適用
          </button>
        )}
        {updateStatus.phase === "up-to-date" && (
          <span className="update-status-text is-ok">最新バージョンです</span>
        )}
        {updateStatus.phase === "error" && (
          <span className="update-status-text is-err">{updateStatus.message}</span>
        )}
      </div>
    </div>
  );
}

function PocDot({ label, active }: { label: string; active: boolean }) {
  return (
    <span
      className={`poc-dot ${active ? "is-active" : "is-inactive"}`}
      title={label}
      aria-label={`${label}: ${active ? "有効" : "無効"}`}
    />
  );
}

export default App;

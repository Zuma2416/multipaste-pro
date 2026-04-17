export type PasteSlotSource = "manual-copy" | "auto-split" | "history-import";
export type PasteMode = "consume" | "keep";

export interface PasteSlot {
  id: string;
  title: string;
  content: string;
  createdAt: string;
  source: PasteSlotSource;
}

export interface PocStatus {
  clipboardRead: boolean;
  globalKeyListening: boolean;
  pasteSimulation: boolean;
}

export interface AppOverview {
  runtimeMode: string;
  slotCapacity: number;
  retentionStrategy: string;
  copyHotkey: string;
  pasteHotkey: string;
  pasteMode: PasteMode;
  storedSlotCount: number;
  lastAction: string;
  poc: PocStatus;
}

export interface ActionResult {
  ok: boolean;
  message: string;
  slotCount: number;
}

export interface Preferences {
  shortcutCopy: string;
  shortcutPaste: string;
  shortcutRangeStart: string;
  shortcutRangeEnd: string;
  slotCapacity: number;
  defaultPasteMode: string;
  soundEnabled: boolean;
}

export function formatSource(source: PasteSlotSource): string {
  switch (source) {
    case "manual-copy": return "手動";
    case "auto-split": return "自動分割";
    case "history-import": return "履歴";
    default: return source;
  }
}

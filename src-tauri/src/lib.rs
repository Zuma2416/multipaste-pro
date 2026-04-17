use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    thread,
    thread::sleep,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use arboard::Clipboard;
use enigo::{
    Button, Coordinate,
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Mouse, Settings,
};
use serde::{Deserialize, Serialize};
use tauri::{ActivationPolicy, AppHandle, Emitter, Manager};
use tauri::menu::{CheckMenuItem, MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_global_shortcut::{Builder as GlobalShortcutBuilder, GlobalShortcutExt, Shortcut};

const APP_EVENT: &str = "multipaste://slots-updated";
const PICKER_EVENT: &str = "multipaste://picker-show";
const STORAGE_FILE: &str = "slots.json";
const PREFERENCES_FILE: &str = "preferences.json";
const DEFAULT_SLOT_CAPACITY: usize = 10;
const DEFAULT_COPY_SHORTCUT: &str = "CommandOrControl+Alt+C";
const DEFAULT_PASTE_SHORTCUT: &str = "CommandOrControl+Alt+V";
const DEFAULT_RANGE_START_SHORTCUT: &str = "CommandOrControl+Alt+BracketLeft";
const DEFAULT_RANGE_END_SHORTCUT: &str = "CommandOrControl+Alt+BracketRight";
const MENU_SHOW_APP: &str = "show-app";
const MENU_PREFERENCES: &str = "preferences";
const MENU_CAPTURE: &str = "capture-now";
const MENU_PASTE: &str = "paste-now";
const MENU_TOGGLE_MODE: &str = "toggle-mode";
const MENU_CHECK_UPDATE: &str = "check-update";
const MENU_QUIT: &str = "quit";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PasteMode {
    Consume,
    Keep,
}

impl PasteMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Consume => "consume",
            Self::Keep => "keep",
        }
    }

    fn toggle(self) -> Self {
        match self {
            Self::Consume => Self::Keep,
            Self::Keep => Self::Consume,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    shortcut_copy: String,
    shortcut_paste: String,
    shortcut_range_start: String,
    shortcut_range_end: String,
    slot_capacity: usize,
    default_paste_mode: String,
    sound_enabled: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            shortcut_copy: DEFAULT_COPY_SHORTCUT.to_string(),
            shortcut_paste: DEFAULT_PASTE_SHORTCUT.to_string(),
            shortcut_range_start: DEFAULT_RANGE_START_SHORTCUT.to_string(),
            shortcut_range_end: DEFAULT_RANGE_END_SHORTCUT.to_string(),
            slot_capacity: DEFAULT_SLOT_CAPACITY,
            default_paste_mode: "consume".to_string(),
            sound_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasteSlot {
    id: String,
    title: String,
    content: String,
    created_at: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSlots {
    slots: Vec<PasteSlot>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PocStatus {
    clipboard_read: bool,
    global_key_listening: bool,
    paste_simulation: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppOverview {
    runtime_mode: &'static str,
    slot_capacity: usize,
    retention_strategy: &'static str,
    copy_hotkey: &'static str,
    paste_hotkey: &'static str,
    paste_mode: &'static str,
    stored_slot_count: usize,
    last_action: String,
    poc: PocStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionResult {
    ok: bool,
    message: String,
    slot_count: usize,
}

/// 範囲選択の開始点。AX テキストカーソル or マウスピクセル座標
#[derive(Debug, Clone)]
enum RangeStart {
    /// アクセシビリティ API によるテキスト文字インデックス
    AxCursor(i64),
    /// マウスの絶対ピクセル座標（AX 非対応アプリ用）
    MousePos(i32, i32),
}

struct SlotStore {
    slots: VecDeque<PasteSlot>,
    last_action: String,
    storage_path: PathBuf,
    paste_mode: PasteMode,
    /// ピッカーを開く直前にフォーカスを持っていたアプリ名
    pre_picker_app: Option<String>,
    /// 範囲選択の開始点
    range_select_start: Option<RangeStart>,
}

struct ParsedShortcuts {
    copy: Shortcut,
    paste: Shortcut,
    range_start: Shortcut,
    range_end: Shortcut,
}

struct AppState {
    inner: Mutex<SlotStore>,
    shortcuts: Mutex<ParsedShortcuts>,
    preferences: Mutex<Preferences>,
    preferences_path: PathBuf,
}

#[tauri::command]
fn get_app_overview(state: tauri::State<'_, AppState>) -> Result<AppOverview, String> {
    let store = state
        .inner
        .lock()
        .map_err(|_| "状態ロックの取得に失敗しました。".to_string())?;
    let prefs = state
        .preferences
        .lock()
        .map_err(|_| "設定の読み込みに失敗しました。".to_string())?;

    Ok(AppOverview {
        runtime_mode: "メニューバー常駐 + 設定ウィンドウ",
        slot_capacity: prefs.slot_capacity,
        retention_strategy: "FIFO ローテーション",
        copy_hotkey: "Cmd + Option + C",
        paste_hotkey: "Cmd + Option + V",
        paste_mode: store.paste_mode.as_str(),
        stored_slot_count: store.slots.len(),
        last_action: store.last_action.clone(),
        poc: PocStatus {
            clipboard_read: true,
            global_key_listening: true,
            paste_simulation: true,
        },
    })
}

#[tauri::command]
fn list_slot_previews(state: tauri::State<'_, AppState>) -> Result<Vec<PasteSlot>, String> {
    let store = state
        .inner
        .lock()
        .map_err(|_| "スロット一覧の取得に失敗しました。".to_string())?;

    Ok(store.slots.iter().cloned().collect())
}

#[tauri::command]
fn capture_clipboard_now(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<ActionResult, String> {
    capture_clipboard_into_store(&app, &state)
}

#[tauri::command]
fn paste_next_slot_now(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<ActionResult, String> {
    paste_next_slot(&app, &state)
}

#[tauri::command]
fn paste_slot_by_index(app: AppHandle, state: tauri::State<'_, AppState>, index: usize) -> Result<ActionResult, String> {
    // スロット取得・状態更新（ロックは早めに解放）
    let (slot, count, pre_picker_app) = {
        let mut store = state
            .inner
            .lock()
            .map_err(|_| "スロット状態の更新に失敗しました。".to_string())?;

        let slot = store
            .slots
            .get(index)
            .cloned()
            .ok_or_else(|| format!("スロット {} が見つかりません。", index + 1))?;

        if matches!(store.paste_mode, PasteMode::Consume) {
            store.slots.remove(index);
        }
        store.last_action = format!("スロット「{}」をペーストしました。", slot.title);
        persist_slots(&store)?;
        let count = store.slots.len();
        let pre_picker_app = store.pre_picker_app.take();
        (slot, count, pre_picker_app)
    };

    // クリップボードへの書き込みはここで完了させる
    let mut clipboard =
        Clipboard::new().map_err(|e| format!("クリップボードの初期化に失敗しました: {e}"))?;
    clipboard
        .set_text(slot.content.clone())
        .map_err(|e| format!("クリップボードへの書き込みに失敗しました: {e}"))?;

    // ピッカーを隠してから元アプリをアクティブにしてペースト
    let message = format!("「{}」をペーストしました。", slot.title);
    let app_clone = app.clone();
    let message_clone = message.clone();
    thread::spawn(move || {
        if let Some(picker) = app_clone.get_webview_window("picker") {
            let _ = picker.hide();
        }

        let paste_result = if let Some(ref app_name) = pre_picker_app {
            // 元アプリを明示的にアクティブにしてから Cmd+V を送る
            activate_and_paste(app_name)
        } else {
            // フォールバック: 少し待ってから enigo で送る
            sleep(Duration::from_millis(200));
            send_cmd_v()
        };

        if let Err(e) = paste_result {
            let payload = ActionResult { ok: false, message: e, slot_count: count };
            emit_slots_updated(&app_clone, &payload);
            return;
        }
        let payload = ActionResult { ok: true, message: message_clone, slot_count: count };
        emit_slots_updated(&app_clone, &payload);
    });

    Ok(ActionResult { ok: true, message, slot_count: count })
}

#[tauri::command]
fn delete_slot(state: tauri::State<'_, AppState>, index: usize) -> Result<ActionResult, String> {
    let mut store = state
        .inner
        .lock()
        .map_err(|_| "スロット状態の更新に失敗しました。".to_string())?;

    if index >= store.slots.len() {
        return Err(format!("スロット {} が見つかりません。", index + 1));
    }

    store.slots.remove(index);
    store.last_action = format!("スロット {} を削除しました。", index + 1);
    persist_slots(&store)?;

    Ok(ActionResult {
        ok: true,
        message: store.last_action.clone(),
        slot_count: store.slots.len(),
    })
}

#[tauri::command]
fn clear_slots(state: tauri::State<'_, AppState>) -> Result<ActionResult, String> {
    let mut store = state
        .inner
        .lock()
        .map_err(|_| "スロット状態の更新に失敗しました。".to_string())?;

    store.slots.clear();
    store.last_action = "すべてのスロットを消去しました。".to_string();
    persist_slots(&store)?;

    Ok(ActionResult {
        ok: true,
        message: store.last_action.clone(),
        slot_count: store.slots.len(),
    })
}

#[tauri::command]
fn set_paste_mode(app: AppHandle, state: tauri::State<'_, AppState>, mode: String) -> Result<ActionResult, String> {
    let next_mode = match mode.as_str() {
        "consume" => PasteMode::Consume,
        "keep" => PasteMode::Keep,
        _ => return Err("未対応のペーストモードです。".to_string()),
    };

    let mut store = state
        .inner
        .lock()
        .map_err(|_| "モード更新に失敗しました。".to_string())?;
    store.paste_mode = next_mode;
    store.last_action = format!(
        "ペーストモードを {} に変更しました。",
        match next_mode {
            PasteMode::Consume => "消費",
            PasteMode::Keep => "保持",
        }
    );
    persist_slots(&store)?;

    let result = ActionResult {
        ok: true,
        message: store.last_action.clone(),
        slot_count: store.slots.len(),
    };
    emit_slots_updated(&app, &result);
    sync_toggle_menu_state(&app, next_mode);
    Ok(result)
}

#[tauri::command]
fn get_preferences(state: tauri::State<'_, AppState>) -> Result<Preferences, String> {
    let prefs = state
        .preferences
        .lock()
        .map_err(|_| "設定の読み込みに失敗しました。".to_string())?;
    Ok(prefs.clone())
}

#[tauri::command]
fn update_preference(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<Preferences, String> {
    let mut prefs = state
        .preferences
        .lock()
        .map_err(|_| "設定の更新に失敗しました。".to_string())?;

    match key.as_str() {
        "shortcutCopy" => {
            // ショートカット文字列のパース検証
            value.parse::<Shortcut>().map_err(|e| format!("ショートカットが無効です: {e}"))?;
            let old = prefs.shortcut_copy.clone();
            prefs.shortcut_copy = value.clone();
            persist_preferences(&prefs, &state.preferences_path)?;
            drop(prefs);
            reregister_shortcut(&app, &state, &old, &value)?;
        }
        "shortcutPaste" => {
            value.parse::<Shortcut>().map_err(|e| format!("ショートカットが無効です: {e}"))?;
            let old = prefs.shortcut_paste.clone();
            prefs.shortcut_paste = value.clone();
            persist_preferences(&prefs, &state.preferences_path)?;
            drop(prefs);
            reregister_shortcut(&app, &state, &old, &value)?;
        }
        "shortcutRangeStart" => {
            value.parse::<Shortcut>().map_err(|e| format!("ショートカットが無効です: {e}"))?;
            let old = prefs.shortcut_range_start.clone();
            prefs.shortcut_range_start = value.clone();
            persist_preferences(&prefs, &state.preferences_path)?;
            drop(prefs);
            reregister_shortcut(&app, &state, &old, &value)?;
        }
        "shortcutRangeEnd" => {
            value.parse::<Shortcut>().map_err(|e| format!("ショートカットが無効です: {e}"))?;
            let old = prefs.shortcut_range_end.clone();
            prefs.shortcut_range_end = value.clone();
            persist_preferences(&prefs, &state.preferences_path)?;
            drop(prefs);
            reregister_shortcut(&app, &state, &old, &value)?;
        }
        "slotCapacity" => {
            let cap: usize = value.parse().map_err(|_| "スロット上限は数値で指定してください。".to_string())?;
            if !(1..=30).contains(&cap) {
                return Err("スロット上限は 1〜30 の範囲で指定してください。".to_string());
            }
            prefs.slot_capacity = cap;
            persist_preferences(&prefs, &state.preferences_path)?;
            drop(prefs);
            // スロット上限を超過する場合は末尾から削除
            let mut store = state
                .inner
                .lock()
                .map_err(|_| "スロット状態の更新に失敗しました。".to_string())?;
            while store.slots.len() > cap {
                store.slots.pop_back();
            }
            persist_slots(&store)?;
        }
        "defaultPasteMode" => {
            if value != "consume" && value != "keep" {
                return Err("ペーストモードは consume か keep で指定してください。".to_string());
            }
            prefs.default_paste_mode = value;
            persist_preferences(&prefs, &state.preferences_path)?;
        }
        "soundEnabled" => {
            let enabled: bool = value.parse().map_err(|_| "効果音設定は true/false で指定してください。".to_string())?;
            prefs.sound_enabled = enabled;
            persist_preferences(&prefs, &state.preferences_path)?;
        }
        _ => {
            return Err(format!("不明な設定キーです: {key}"));
        }
    }

    let prefs = state
        .preferences
        .lock()
        .map_err(|_| "設定の読み込みに失敗しました。".to_string())?;
    Ok(prefs.clone())
}

fn reregister_shortcut(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    old_str: &str,
    new_str: &str,
) -> Result<(), String> {
    let old_sc: Shortcut = old_str.parse().map_err(|e| format!("旧ショートカットのパースに失敗: {e}"))?;
    let new_sc: Shortcut = new_str.parse().map_err(|e| format!("新ショートカットのパースに失敗: {e}"))?;

    let gsm = app.global_shortcut();
    gsm.unregister(old_sc)
        .map_err(|e| format!("ショートカットの解除に失敗しました: {e}"))?;
    gsm.on_shortcut(new_sc, |app, shortcut, event| {
        use tauri_plugin_global_shortcut::ShortcutState;
        if event.state == ShortcutState::Released {
            handle_shortcut(app, shortcut);
        }
    })
    .map_err(|e| format!("ショートカットの再登録に失敗しました: {e}"))?;

    // ParsedShortcuts を更新
    let prefs = state
        .preferences
        .lock()
        .map_err(|_| "設定の読み込みに失敗しました。".to_string())?;
    let mut sc = state
        .shortcuts
        .lock()
        .map_err(|_| "ショートカット状態の更新に失敗しました。".to_string())?;
    sc.copy = prefs.shortcut_copy.parse().unwrap_or(sc.copy);
    sc.paste = prefs.shortcut_paste.parse().unwrap_or(sc.paste);
    sc.range_start = prefs.shortcut_range_start.parse().unwrap_or(sc.range_start);
    sc.range_end = prefs.shortcut_range_end.parse().unwrap_or(sc.range_end);

    Ok(())
}

/// 各行末の空白・タブを除去し、先頭・末尾の空行を取り除く。
/// 改行・行内スペースはそのまま保持する。
fn normalize_whitespace(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();

    // 各行末の空白を除去
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_end()).collect();

    // 先頭の空行を除去
    let start = trimmed.iter().position(|l| !l.is_empty()).unwrap_or(0);
    // 末尾の空行を除去
    let end = trimmed.iter().rposition(|l| !l.is_empty()).map(|i| i + 1).unwrap_or(0);

    trimmed[start..end].join("\n")
}

fn capture_clipboard_into_store(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
) -> Result<ActionResult, String> {
    let raw = capture_selected_text()?;
    let normalized = normalize_whitespace(&raw);
    if normalized.is_empty() {
        return Err("選択中のテキストを取得できませんでした。".to_string());
    }

    let slot_capacity = state
        .preferences
        .lock()
        .map(|p| p.slot_capacity)
        .unwrap_or(DEFAULT_SLOT_CAPACITY);

    let mut store = state
        .inner
        .lock()
        .map_err(|_| "スロット状態の更新に失敗しました。".to_string())?;

    let slot = PasteSlot {
        id: format!("slot-{}", current_timestamp()),
        title: build_slot_title(&normalized),
        content: normalized.clone(),
        created_at: current_timestamp().to_string(),
        source: "manual-copy".to_string(),
    };

    store.slots.push_front(slot);
    while store.slots.len() > slot_capacity {
        store.slots.pop_back();
    }
    store.last_action = format!("選択テキストをスロットへ保存しました。件数: {}", store.slots.len());
    persist_slots(&store)?;

    let result = ActionResult {
        ok: true,
        message: store.last_action.clone(),
        slot_count: store.slots.len(),
    };
    emit_slots_updated(app, &result);
    Ok(result)
}

fn capture_selected_text() -> Result<String, String> {
    if let Ok(text) = capture_selected_text_directly() {
        if !text.trim().is_empty() {
            return Ok(text.trim().to_string());
        }
    }

    let mut clipboard =
        Clipboard::new().map_err(|error| format!("クリップボードの初期化に失敗しました: {error}"))?;

    let previous_text = clipboard.get_text().ok();

    send_cmd_c().map_err(|error| format!("選択コピー送信に失敗しました: {error}"))?;
    sleep(Duration::from_millis(140));

    let copied_text = clipboard
        .get_text()
        .map_err(|error| format!("選択テキストの取得に失敗しました: {error}"))?;

    if let Some(previous_text) = previous_text {
        let _ = clipboard.set_text(previous_text);
    }

    Ok(copied_text.trim().to_string())
}

fn capture_selected_text_directly() -> Result<String, String> {
    let script = r#"
tell application "System Events"
    tell (first process whose frontmost is true)
        try
            set focusedElement to value of attribute "AXFocusedUIElement"
            set selectedText to value of attribute "AXSelectedText" of focusedElement
            return selectedText
        on error errMsg number errNum
            error errMsg number errNum
        end try
    end tell
end tell
"#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| format!("直接取得スクリプトの起動に失敗しました: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "直接取得スクリプトが失敗しました。".to_string()
        } else {
            format!("直接取得に失敗しました: {stderr}")
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn paste_next_slot(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
) -> Result<ActionResult, String> {
    let (slot, count) = {
        let mut store = state
            .inner
            .lock()
            .map_err(|_| "スロット状態の更新に失敗しました。".to_string())?;

        let Some(slot) = store.slots.front().cloned() else {
            return Err("ペースト対象のスロットがありません。".to_string());
        };

        if matches!(store.paste_mode, PasteMode::Consume) {
            store.slots.pop_front();
        }
        store.last_action = format!(
            "スロット「{}」を {} モードでペーストしました。",
            slot.title,
            store.paste_mode.as_str()
        );
        persist_slots(&store)?;
        let count = store.slots.len();
        (slot, count)
    };

    let mut clipboard =
        Clipboard::new().map_err(|error| format!("クリップボードの初期化に失敗しました: {error}"))?;
    clipboard
        .set_text(slot.content.clone())
        .map_err(|error| format!("クリップボードへの書き戻しに失敗しました: {error}"))?;

    send_cmd_v().map_err(|error| format!("ペースト送信に失敗しました: {error}"))?;

    let result = ActionResult {
        ok: true,
        message: format!("「{}」をペーストしました。", slot.title),
        slot_count: count,
    };
    emit_slots_updated(app, &result);
    Ok(result)
}

fn send_cmd_key(key: char) -> Result<(), String> {
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|error| format!("入力初期化エラー: {error}"))?;

    enigo
        .key(Key::Meta, Press)
        .map_err(|error| format!("Cmd 押下エラー: {error}"))?;
    enigo
        .key(Key::Unicode(key), Click)
        .map_err(|error| format!("{} 送信エラー: {error}", key.to_uppercase()))?;
    enigo
        .key(Key::Meta, Release)
        .map_err(|error| format!("Cmd 解放エラー: {error}"))?;
    Ok(())
}

fn send_cmd_v() -> Result<(), String> {
    send_cmd_key('v')
}

fn send_cmd_c() -> Result<(), String> {
    send_cmd_key('c')
}

fn play_sound(app: &AppHandle, name: &'static str) {
    let sound_enabled = app
        .try_state::<AppState>()
        .and_then(|state| state.preferences.lock().ok().map(|p| p.sound_enabled))
        .unwrap_or(true);
    if !sound_enabled {
        return;
    }
    let _ = Command::new("afplay")
        .arg(format!("/System/Library/Sounds/{name}.aiff"))
        .spawn();
}

/// ピッカーを開く直前にフォーカスを持っているアプリ名を取得する
fn get_frontmost_app_name() -> Option<String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to get name of first application process whose frontmost is true"#)
        .output()
        .ok()?;

    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    } else {
        None
    }
}

/// 指定アプリをアクティブにしてから Cmd+V を送信する
fn activate_and_paste(app_name: &str) -> Result<(), String> {
    let safe_name = app_name.replace('"', "\\\"");
    let script = format!(
        r#"tell application "{safe_name}" to activate
delay 0.15
tell application "System Events"
    keystroke "v" using command down
end tell"#
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("AppleScript の実行に失敗しました: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("フォーカス復元またはペーストに失敗しました: {stderr}"));
    }
    Ok(())
}

fn show_picker(app: &AppHandle) {
    let state = app.state::<AppState>();

    let slots: Vec<PasteSlot> = match state.inner.lock() {
        Ok(mut store) => {
            if store.slots.is_empty() {
                drop(store);
                let payload = ActionResult {
                    ok: false,
                    message: "ペースト対象のスロットがありません。".to_string(),
                    slot_count: 0,
                };
                emit_slots_updated(app, &payload);
                return;
            }
            // ピッカーを表示する前に現在フォーカスしているアプリを記憶する
            store.pre_picker_app = get_frontmost_app_name();
            store.slots.iter().cloned().collect()
        }
        Err(_) => return,
    };

    let _ = app.emit(PICKER_EVENT, &slots);

    if let Some(picker) = app.get_webview_window("picker") {
        let _ = picker.show();
        let _ = picker.set_focus();
    }
}

fn show_hud(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        if let Some(hud) = app.get_webview_window("hud") {
            let _ = hud.show();
            sleep(Duration::from_millis(2200));
            let _ = hud.hide();
        }
    });
}

fn emit_slots_updated(app: &AppHandle, result: &ActionResult) {
    let _ = app.emit(APP_EVENT, result);
    if result.ok {
        play_sound(app, "Tink");
    } else {
        play_sound(app, "Basso");
    }
    show_hud(app);
}

fn position_hud(app: &AppHandle) -> Result<(), String> {
    let hud = app
        .get_webview_window("hud")
        .ok_or_else(|| "HUD ウィンドウが見つかりません。".to_string())?;

    if let Ok(Some(monitor)) = hud.primary_monitor() {
        let size: tauri::PhysicalSize<u32> = *monitor.size();
        let scale = monitor.scale_factor();
        let logical_w = size.width as f64 / scale;
        let x = (logical_w - 440.0) / 2.0;
        let _ = hud.set_position(tauri::LogicalPosition::new(x, 20.0));
    }

    Ok(())
}

fn position_picker(app: &AppHandle) -> Result<(), String> {
    let picker = app
        .get_webview_window("picker")
        .ok_or_else(|| "ピッカーウィンドウが見つかりません。".to_string())?;

    if let Ok(Some(monitor)) = picker.primary_monitor() {
        let size: tauri::PhysicalSize<u32> = *monitor.size();
        let scale = monitor.scale_factor();
        let logical_w = size.width as f64 / scale;
        let logical_h = size.height as f64 / scale;
        let x = (logical_w - 500.0) / 2.0;
        let y = (logical_h - 520.0) / 2.0 - 40.0;
        let _ = picker.set_position(tauri::LogicalPosition::new(x, y.max(20.0)));
    }

    Ok(())
}

fn build_slot_title(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or(content).trim();
    first_line.chars().take(18).collect()
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn persist_slots(store: &SlotStore) -> Result<(), String> {
    let payload = StoredSlots {
        slots: store.slots.iter().cloned().collect(),
    };

    if let Some(parent) = store.storage_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("保存先フォルダの作成に失敗しました: {error}"))?;
    }

    let json = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("JSON 変換に失敗しました: {error}"))?;
    fs::write(&store.storage_path, json)
        .map_err(|error| format!("スロット保存に失敗しました: {error}"))?;
    Ok(())
}

fn load_slots(storage_path: &Path) -> VecDeque<PasteSlot> {
    let Ok(raw) = fs::read_to_string(storage_path) else {
        return VecDeque::new();
    };

    serde_json::from_str::<StoredSlots>(&raw)
        .map(|stored| stored.slots.into())
        .unwrap_or_default()
}

fn resolve_storage_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("multipaste-pro")
        .join(STORAGE_FILE)
}

fn resolve_preferences_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("multipaste-pro")
        .join(PREFERENCES_FILE)
}

fn load_preferences(path: &Path) -> Preferences {
    let Ok(raw) = fs::read_to_string(path) else {
        return Preferences::default();
    };
    serde_json::from_str::<Preferences>(&raw).unwrap_or_default()
}

fn persist_preferences(prefs: &Preferences, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("設定フォルダの作成に失敗しました: {e}"))?;
    }
    let json = serde_json::to_string_pretty(prefs)
        .map_err(|e| format!("設定の JSON 変換に失敗しました: {e}"))?;
    fs::write(path, json)
        .map_err(|e| format!("設定の保存に失敗しました: {e}"))?;
    Ok(())
}

// ── ウィンドウ状態の手動保存/復元 ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MainWindowState {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn window_state_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("multipaste-pro")
        .join("window-state.json")
}

fn save_main_window_state(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    // 非表示時は保存しない（最小化や hide() 直後の誤った座標を記録しないため）
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let Ok(pos) = window.outer_position() else { return };
    let Ok(size) = window.outer_size() else { return };
    let Ok(scale) = window.scale_factor() else { return };

    let state = MainWindowState {
        x: pos.x as f64 / scale,
        y: pos.y as f64 / scale,
        width: size.width as f64 / scale,
        height: size.height as f64 / scale,
    };

    let path = window_state_path(app);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = fs::write(&path, json);
    }
}

fn load_main_window_state(app: &AppHandle) -> Option<MainWindowState> {
    let path = window_state_path(app);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn restore_main_window_state(app: &AppHandle) {
    let Some(state) = load_main_window_state(app) else { return };
    let Some(window) = app.get_webview_window("main") else { return };
    let _ = window.set_position(tauri::LogicalPosition::new(state.x, state.y));
    let _ = window.set_size(tauri::LogicalSize::new(state.width, state.height));
}

fn initialize_state(app: &AppHandle) -> AppState {
    let storage_path = resolve_storage_path(app);
    let preferences_path = resolve_preferences_path(app);
    let slots = load_slots(&storage_path);
    let prefs = load_preferences(&preferences_path);

    let initial_paste_mode = match prefs.default_paste_mode.as_str() {
        "keep" => PasteMode::Keep,
        _ => PasteMode::Consume,
    };

    let shortcuts = ParsedShortcuts {
        copy: prefs.shortcut_copy.parse().unwrap_or_else(|_| DEFAULT_COPY_SHORTCUT.parse().unwrap()),
        paste: prefs.shortcut_paste.parse().unwrap_or_else(|_| DEFAULT_PASTE_SHORTCUT.parse().unwrap()),
        range_start: prefs.shortcut_range_start.parse().unwrap_or_else(|_| DEFAULT_RANGE_START_SHORTCUT.parse().unwrap()),
        range_end: prefs.shortcut_range_end.parse().unwrap_or_else(|_| DEFAULT_RANGE_END_SHORTCUT.parse().unwrap()),
    };

    AppState {
        inner: Mutex::new(SlotStore {
            slots,
            last_action: "待機中".to_string(),
            storage_path,
            paste_mode: initial_paste_mode,
            pre_picker_app: None,
            range_select_start: None,
        }),
        shortcuts: Mutex::new(shortcuts),
        preferences: Mutex::new(prefs),
        preferences_path,
    }
}

fn create_tray(app: &AppHandle) -> Result<(), String> {
    let show_item = MenuItemBuilder::with_id(MENU_SHOW_APP, "ダッシュボードを開く")
        .build(app)
        .map_err(|error| format!("トレイメニューの作成に失敗しました: {error}"))?;
    let preferences_item = MenuItemBuilder::with_id(MENU_PREFERENCES, "設定...")
        .build(app)
        .map_err(|error| format!("トレイメニューの作成に失敗しました: {error}"))?;
    let capture_item = MenuItemBuilder::with_id(MENU_CAPTURE, "クリップボードを保存")
        .accelerator("CmdOrCtrl+Alt+C")
        .build(app)
        .map_err(|error| format!("トレイメニューの作成に失敗しました: {error}"))?;
    let paste_item = MenuItemBuilder::with_id(MENU_PASTE, "先頭スロットを貼り付け")
        .accelerator("CmdOrCtrl+Alt+V")
        .build(app)
        .map_err(|error| format!("トレイメニューの作成に失敗しました: {error}"))?;
    let toggle_item = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_MODE,
        "保持モード",
        true,
        false,
        None::<&str>,
    )
    .map_err(|error| format!("トレイメニューの作成に失敗しました: {error}"))?;
    let check_update_item = MenuItemBuilder::with_id(MENU_CHECK_UPDATE, "アップデート確認...")
        .build(app)
        .map_err(|error| format!("トレイメニューの作成に失敗しました: {error}"))?;
    let quit_item = MenuItemBuilder::with_id(MENU_QUIT, "終了")
        .build(app)
        .map_err(|error| format!("トレイメニューの作成に失敗しました: {error}"))?;

    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .item(&preferences_item)
        .separator()
        .item(&capture_item)
        .item(&paste_item)
        .item(&toggle_item)
        .separator()
        .item(&check_update_item)
        .item(&quit_item)
        .build()
        .map_err(|error| format!("トレイメニューの構築に失敗しました: {error}"))?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "トレイアイコンを取得できませんでした。".to_string())?;

    TrayIconBuilder::with_id("multipaste-tray")
        .menu(&menu)
        .icon(icon)
        .tooltip("MultiPaste Pro")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event: MenuEvent| {
            handle_tray_menu_event(app, event.id().as_ref());
        })
        .on_tray_icon_event(|tray, event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                let _ = toggle_main_window(&app);
            }
        })
        .build(app)
        .map_err(|error| format!("トレイアイコンの作成に失敗しました: {error}"))?;

    Ok(())
}

fn handle_tray_menu_event(app: &AppHandle, menu_id: &str) {
    match menu_id {
        MENU_SHOW_APP => {
            let _ = show_main_window(app);
        }
        MENU_PREFERENCES => {
            show_preferences_window(app);
        }
        MENU_CAPTURE => {
            let state = app.state::<AppState>();
            let _ = capture_clipboard_into_store(app, &state);
        }
        MENU_PASTE => {
            let state = app.state::<AppState>();
            let _ = paste_next_slot(app, &state);
        }
        MENU_TOGGLE_MODE => {
            let state = app.state::<AppState>();
            let mode = state
                .inner
                .lock()
                .map(|store| store.paste_mode.toggle())
                .unwrap_or(PasteMode::Consume);
            let _ = set_paste_mode(app.clone(), state, mode.as_str().to_string());
        }
        MENU_CHECK_UPDATE => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
                let _ = window.emit("check-update", ());
            }
        }
        MENU_QUIT => {
            app.exit(0);
        }
        _ => {}
    }
}

fn show_preferences_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("preferences") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "メインウィンドウが見つかりません。".to_string())?;
    window
        .show()
        .map_err(|error| format!("ウィンドウ表示に失敗しました: {error}"))?;
    // macOS は hide() → show() でウィンドウ位置がリセットされる場合があるため再適用する
    restore_main_window_state(app);
    window
        .set_focus()
        .map_err(|error| format!("ウィンドウのフォーカスに失敗しました: {error}"))?;
    Ok(())
}

fn toggle_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "メインウィンドウが見つかりません。".to_string())?;
    let is_visible = window
        .is_visible()
        .map_err(|error| format!("ウィンドウ状態の取得に失敗しました: {error}"))?;

    if is_visible {
        window
            .hide()
            .map_err(|error| format!("ウィンドウ非表示に失敗しました: {error}"))?;
    } else {
        show_main_window(app)?;
    }
    Ok(())
}

fn sync_toggle_menu_state(app: &AppHandle, mode: PasteMode) {
    if let Some(menu) = app.menu() {
        if let Some(menu_item) = menu.get(MENU_TOGGLE_MODE) {
            if let Some(check_item) = menu_item.as_check_menuitem() {
            let _ = check_item.set_checked(matches!(mode, PasteMode::Keep));
            }
        }
    }
}

/// フォーカス中テキストフィールドのカーソル位置（文字インデックス）を取得する
fn get_ax_cursor_position() -> Result<i64, String> {
    let script = r#"tell application "System Events"
    tell (first process whose frontmost is true)
        try
            set el to value of attribute "AXFocusedUIElement"
            set r to value of attribute "AXSelectedTextRange" of el
            return (location of r) as string
        on error
            return "-1"
        end try
    end tell
end tell"#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("カーソル位置の取得に失敗しました: {e}"))?;

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<i64>()
        .map_err(|_| "カーソル位置の解析に失敗しました。".to_string())
}

/// 現在のマウスカーソルのスクリーン座標を取得する
fn get_mouse_position() -> Result<(i32, i32), String> {
    let enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("入力初期化エラー: {e}"))?;
    enigo
        .location()
        .map_err(|e| format!("マウス位置の取得に失敗しました: {e}"))
}

/// マウスドラッグでテキストを選択 → ⌘C でコピー → スロットへ保存する
fn simulate_drag_and_capture(
    app: &AppHandle,
    start: (i32, i32),
    end: (i32, i32),
) -> Result<ActionResult, String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("入力初期化エラー: {e}"))?;

    // 開始点へ移動 → 左ボタン押下 → 終了点へ移動 → 左ボタン解放
    enigo
        .move_mouse(start.0, start.1, Coordinate::Abs)
        .map_err(|e| format!("マウス移動エラー: {e}"))?;
    sleep(Duration::from_millis(60));
    enigo
        .button(Button::Left, Press)
        .map_err(|e| format!("マウスダウンエラー: {e}"))?;
    sleep(Duration::from_millis(60));
    enigo
        .move_mouse(end.0, end.1, Coordinate::Abs)
        .map_err(|e| format!("マウス移動エラー: {e}"))?;
    sleep(Duration::from_millis(80));
    enigo
        .button(Button::Left, Release)
        .map_err(|e| format!("マウスアップエラー: {e}"))?;
    sleep(Duration::from_millis(100));

    // 選択テキストを ⌘C でコピー
    send_cmd_c()?;
    sleep(Duration::from_millis(150));

    // クリップボードから取得
    let mut clipboard = Clipboard::new()
        .map_err(|e| format!("クリップボードの初期化に失敗しました: {e}"))?;
    let raw = clipboard
        .get_text()
        .map_err(|e| format!("テキストの取得に失敗しました: {e}"))?;
    let normalized = normalize_whitespace(&raw);

    if normalized.is_empty() {
        return Err("選択範囲のテキストを取得できませんでした。".to_string());
    }

    let state = app.state::<AppState>();
    let slot_capacity = state
        .preferences
        .lock()
        .map(|p| p.slot_capacity)
        .unwrap_or(DEFAULT_SLOT_CAPACITY);

    let mut store = state
        .inner
        .lock()
        .map_err(|_| "スロット状態の更新に失敗しました。".to_string())?;

    let slot = PasteSlot {
        id: format!("slot-{}", current_timestamp()),
        title: build_slot_title(&normalized),
        content: normalized.clone(),
        created_at: current_timestamp().to_string(),
        source: "manual-copy".to_string(),
    };

    store.slots.push_front(slot);
    while store.slots.len() > slot_capacity {
        store.slots.pop_back();
    }
    store.last_action = format!("マウス選択テキストを保存しました。件数: {}", store.slots.len());
    persist_slots(&store)?;

    Ok(ActionResult {
        ok: true,
        message: store.last_action.clone(),
        slot_count: store.slots.len(),
    })
}

/// 開始〜終了の範囲をテキスト選択状態にする
fn apply_ax_selection(start: i64, end: i64) -> Result<(), String> {
    let (loc, len) = if start <= end {
        (start, end - start)
    } else {
        (end, start - end)
    };

    let script = format!(
        r#"tell application "System Events"
    tell (first process whose frontmost is true)
        try
            set el to value of attribute "AXFocusedUIElement"
            set value of attribute "AXSelectedTextRange" of el to {{location:{loc}, length:{len}}}
        on error errMsg
            error errMsg
        end try
    end tell
end tell"#
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("テキスト選択の適用に失敗しました: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("テキスト選択に失敗しました: {stderr}"));
    }
    Ok(())
}

fn handle_range_start(app: &AppHandle) {
    let slot_count = app
        .state::<AppState>()
        .inner
        .lock()
        .map(|s| s.slots.len())
        .unwrap_or_default();

    // まず AX カーソル位置を試みる。失敗したらマウス座標にフォールバック
    let (range_start, mode_label) = match get_ax_cursor_position() {
        Ok(pos) if pos >= 0 => (RangeStart::AxCursor(pos), "テキスト"),
        _ => match get_mouse_position() {
            Ok((x, y)) => (RangeStart::MousePos(x, y), "マウス"),
            Err(e) => {
                let payload = ActionResult {
                    ok: false,
                    message: format!("開始位置の取得に失敗しました: {e}"),
                    slot_count,
                };
                emit_slots_updated(app, &payload);
                return;
            }
        },
    };

    if let Ok(mut store) = app.state::<AppState>().inner.lock() {
        store.range_select_start = Some(range_start);
    }

    let payload = ActionResult {
        ok: true,
        message: format!(
            "開始位置を記録しました（{}モード）。終了位置へ移動して ⌘⌥] を押してください。",
            mode_label
        ),
        slot_count,
    };
    let _ = app.emit(APP_EVENT, &payload);
    show_hud(app);
}

fn handle_range_end(app: &AppHandle) {
    let slot_count = app
        .state::<AppState>()
        .inner
        .lock()
        .map(|s| s.slots.len())
        .unwrap_or_default();

    let start = match app.state::<AppState>().inner.lock() {
        Ok(mut store) => store.range_select_start.take(),
        Err(_) => return,
    };

    let Some(start) = start else {
        let payload = ActionResult {
            ok: false,
            message: "先に ⌘⌥[ で開始位置を指定してください。".to_string(),
            slot_count,
        };
        emit_slots_updated(app, &payload);
        return;
    };

    match start {
        // ── テキストカーソルモード（AX API）────────────────────────────
        RangeStart::AxCursor(start_pos) => {
            let end = match get_ax_cursor_position() {
                Ok(pos) if pos >= 0 => pos,
                _ => {
                    let payload = ActionResult {
                        ok: false,
                        message: "終了位置の取得に失敗しました。".to_string(),
                        slot_count,
                    };
                    emit_slots_updated(app, &payload);
                    return;
                }
            };

            if start_pos == end {
                let payload = ActionResult {
                    ok: false,
                    message: "開始位置と終了位置が同じです。".to_string(),
                    slot_count,
                };
                emit_slots_updated(app, &payload);
                return;
            }

            match apply_ax_selection(start_pos, end) {
                Ok(()) => {
                    let chars = (end - start_pos).unsigned_abs() as usize;
                    let payload = ActionResult {
                        ok: true,
                        message: format!("{chars} 文字を選択しました。⌘⌥C で保存できます。"),
                        slot_count,
                    };
                    emit_slots_updated(app, &payload);
                }
                Err(e) => {
                    emit_slots_updated(app, &ActionResult { ok: false, message: e, slot_count });
                }
            }
        }

        // ── マウスドラッグモード（AX 非対応アプリ用）────────────────────
        RangeStart::MousePos(start_x, start_y) => {
            let (end_x, end_y) = match get_mouse_position() {
                Ok(pos) => pos,
                Err(e) => {
                    let payload = ActionResult {
                        ok: false,
                        message: format!("終了位置の取得に失敗しました: {e}"),
                        slot_count,
                    };
                    emit_slots_updated(app, &payload);
                    return;
                }
            };

            // ドラッグ・コピー・スロット保存は別スレッドで実行（ブロッキング回避）
            let app_clone = app.clone();
            thread::spawn(move || {
                match simulate_drag_and_capture(&app_clone, (start_x, start_y), (end_x, end_y)) {
                    Ok(result) => emit_slots_updated(&app_clone, &result),
                    Err(e) => {
                        emit_slots_updated(
                            &app_clone,
                            &ActionResult { ok: false, message: e, slot_count },
                        );
                    }
                }
            });
        }
    }
}

fn handle_shortcut(app: &AppHandle, shortcut: &Shortcut) {
    let state = app.state::<AppState>();
    let sc = match state.shortcuts.lock() {
        Ok(sc) => sc,
        Err(_) => return,
    };

    if shortcut == &sc.paste {
        drop(sc);
        show_picker(app);
        return;
    }
    if shortcut == &sc.range_start {
        drop(sc);
        handle_range_start(app);
        return;
    }
    if shortcut == &sc.range_end {
        drop(sc);
        handle_range_end(app);
        return;
    }

    let is_copy = shortcut == &sc.copy;
    drop(sc);

    let result = if is_copy {
        capture_clipboard_into_store(app, &state)
    } else {
        Ok(ActionResult {
            ok: false,
            message: format!("未定義のショートカットです: {}", shortcut.into_string()),
            slot_count: 0,
        })
    };

    if let Err(message) = result {
        let payload = ActionResult {
            ok: false,
            message,
            slot_count: state
                .inner
                .lock()
                .map(|store| store.slots.len())
                .unwrap_or_default(),
        };
        emit_slots_updated(app, &payload);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_handle = app.handle().clone();
            #[cfg(target_os = "macos")]
            {
                let _ = app.set_activation_policy(ActivationPolicy::Accessory);
                let _ = app.set_dock_visibility(false);
            }
            let state = initialize_state(&app_handle);

            // Preferences から初期 PasteMode を取得（manage の前に）
            let initial_paste_mode = state
                .inner
                .lock()
                .map(|s| s.paste_mode)
                .unwrap_or(PasteMode::Consume);

            // Preferences からショートカット文字列を取得（register 用）
            let (sc_copy, sc_paste, sc_range_start, sc_range_end) = {
                let prefs = state.preferences.lock().unwrap();
                (
                    prefs.shortcut_copy.clone(),
                    prefs.shortcut_paste.clone(),
                    prefs.shortcut_range_start.clone(),
                    prefs.shortcut_range_end.clone(),
                )
            };

            app.manage(state);
            create_tray(&app_handle)?;
            position_hud(&app_handle)?;
            position_picker(&app_handle)?;
            sync_toggle_menu_state(&app_handle, initial_paste_mode);

            // Preferences のショートカットでグローバルショートカットを登録
            let gsm = app_handle.global_shortcut();
            let shortcuts_to_register: Vec<Shortcut> = [
                sc_copy.as_str(),
                sc_paste.as_str(),
                sc_range_start.as_str(),
                sc_range_end.as_str(),
            ]
            .iter()
            .filter_map(|s| s.parse::<Shortcut>().ok())
            .collect();

            for sc in shortcuts_to_register {
                gsm.on_shortcut(sc, |app, shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state == ShortcutState::Released {
                        handle_shortcut(app, shortcut);
                    }
                })
                .map_err(|e| format!("ショートカットの登録に失敗しました: {e}"))?;
            }

            // 前回のウィンドウ位置・サイズを復元
            restore_main_window_state(&app_handle);

            // Moved / Resized のたびにファイルへ保存（Ctrl+C 終了にも対応）
            if let Some(main_window) = app.get_webview_window("main") {
                let app_for_event = app_handle.clone();
                main_window.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                        save_main_window_state(&app_for_event);
                    }
                });
            }

            Ok(())
        })
        .plugin(
            GlobalShortcutBuilder::new()
                .with_handler(|app, shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;

                    if event.state == ShortcutState::Released {
                        handle_shortcut(app, shortcut);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            get_app_overview,
            list_slot_previews,
            capture_clipboard_now,
            paste_next_slot_now,
            paste_slot_by_index,
            delete_slot,
            clear_slots,
            set_paste_mode,
            get_preferences,
            update_preference
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

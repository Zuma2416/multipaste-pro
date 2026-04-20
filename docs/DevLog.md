# MultiPaste Pro 開発ログ

## 現状の実装状態

- Tauri v2 + React 19 + TypeScript によるマルチスロットクリップボードマネージャ
- バージョン: v0.2.0
- 主要機能: スロットへのコピー保存、ペーストモード切替（消費モード / 保持モード）、グローバルホットキー、設定ウィンドウ（Preferences）、HUD / Picker ウィンドウ、自動アップデーター
- GitHub Actions によるリリースビルド（.dmg + .app.tar.gz + 署名 + latest.json）
- `src-tauri/src/lib.rs` にバックエンド状態管理・コマンド群を実装
- `preferences.json` による設定永続化

## 次のタスク

1. 今回のバグ修正（デッドロック修正 + ペーストモード永続化）をコミットする
2. `cargo check` は PASS 済み。`npm run tauri dev` で実機動作確認を行う
   - 設定ウィンドウから defaultPasteMode を「保持」に変更 → アプリ再起動 → モードが維持されていることを確認
   - メインUI のモード切替ボタンでモード変更 → アプリ再起動 → モードが維持されていることを確認
3. 動作確認後、v0.2.1 としてリリースビルドを検討

## 技術的メモ

### デッドロックの原因パターン（Rust std::sync::Mutex）

`std::sync::Mutex` はリエントラントでないため、同一スレッドで二重ロックするとデッドロックする。
`update_preference` 関数では、`defaultPasteMode` と `soundEnabled` ブランチで `prefs`（MutexGuard）を `drop` せずに、関数末尾で同じ `state.preferences` mutex を再ロックしようとしていた。他のブランチ（shortcut 系）では正しく `drop(prefs)` を呼んでいたのに、この 2 ブランチでは漏れていた。

### ペーストモード永続化の設計

`set_paste_mode`（メインUI のモード切替ボタン）と `update_preference`（設定ウィンドウ）の2つの経路からモード変更が可能。両方でランタイム（`store.paste_mode`）と永続化（`prefs.default_paste_mode` + `preferences.json`）の両方を更新する必要がある。

- `set_paste_mode`: ランタイム更新のみだった → `preferences.json` への永続化を追加
- `update_preference` の `defaultPasteMode` ブランチ: 永続化のみだった → ランタイム反映 + メニュー同期を追加

### デフォルトペーストモードの変更

`Preferences::default()` の `default_paste_mode` を `"consume"` から `"keep"` に変更した（ユーザーの運用実態に合わせた）。

### ビルド・開発コマンド

```bash
cd /Users/Taishiro/Dev_Projects/multipaste-pro
npm run tauri dev       # 開発サーバー + Tauri ウィンドウ起動
cargo check             # Rust 側の型チェック（src-tauri/ で実行）
npm run build           # フロントエンドビルド
```

### バージョン更新時の注意

3 ファイル同時更新が必須:
- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

### リリース

- GitHub Actions の `workflow_dispatch` で手動トリガー
- `tauri-action@v0` は使用禁止（macOS codesign エラー）。直接 `npx tauri build` を使う
- Apple Developer ID 未署名のため Gatekeeper ブロックあり → `xattr -cr` で解除

## 更新履歴

| 日付 | 作業内容 |
|------|---------|
| 2026-04-20 | バグ修正: `update_preference` のデッドロック（`defaultPasteMode` / `soundEnabled` で `drop(prefs)` 漏れ）、`set_paste_mode` のペーストモード未永続化。デフォルトペーストモードを `consume` → `keep` に変更。`cargo check` PASS |
| 2026-04-18 | 設定ウィンドウ（Preferences）を実装 |
| 2026-04-18 | リリースビルドに自己署名証明書によるコード署名を追加 |
| 2026-04-18 | v0.2.0 リリースマニフェスト更新、updater バンドル修正 |

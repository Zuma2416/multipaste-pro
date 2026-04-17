# MultiPaste Pro App

MultiPaste Pro の本番実装に向けた `Tauri + React + TypeScript` の土台です。

## この段階で入っているもの

- PoC で確認できた 3 要素の状態を Rust 側から返す `get_app_overview` コマンド
- スロット保存モデル、ホットキー、保持戦略を可視化する初期ダッシュボード
- 本番実装前に UI と状態設計をすり合わせるためのモックスロット表示

## ディレクトリ

- `src/`: 初期ダッシュボード UI
- `src/types.ts`: フロント側の状態モデル
- `src-tauri/src/lib.rs`: Rust コマンドとアプリ初期状態
- `../multipaste_poc/`: macOS ネイティブ API の検証 CLI

## 次の実装候補

- グローバルホットキー監視を `src-tauri` に移植
- スロットの永続化レイヤー追加
- メニューバー常駐化と Preferences 画面分離
- ペーストメニューの表示と選択操作の実装

## 開発コマンド

```bash
npm install
npm run tauri dev
```

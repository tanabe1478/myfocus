# テーマ設計

テーマ機能は `src/theme.ts` を境界として、UIコンポーネントから色の定義を分離する。

## 永続化

- 選択中のID: SQLite `settings.theme_id`
- ユーザー定義: SQLite `settings.custom_themes`（JSON配列）
- 起動直後のちらつき防止: 同じ値をlocalStorageにもミラーする
- SQLiteを正として読み直し、`settings-updated`イベントで全Tauriウィンドウへ反映する

## テーマ定義

```json
{
  "id": "user:solarized",
  "name": "Solarized",
  "appearance": "dark",
  "base": "warm-dark",
  "tokens": {
    "bgReading": "#002b36",
    "text": "#839496",
    "accent": "#b58900"
  }
}
```

`base`は`warm-light`または`warm-dark`。`tokens`は部分指定できる。未指定トークンを基底テーマから継承するため、将来アプリ側にトークンを追加しても既存のユーザーテーマは壊れない。

現在の組み込みテーマ:

- `warm-light`: 従来のライトテーマ
- `warm-dark`: ダークテーマ

## 安全性と互換性

- 組み込みテーマと同じIDのユーザーテーマは読み込まない
- ID・名前・appearance・tokensの型を検証する
- CSS値は長さを制限し、外部リソースを取得できる`url(...)`を拒否する
- UIはCSS変数だけを参照し、テーマ適用処理がDOMルートへ変数を設定する

将来のテーマエディターやテーマファイルのインポート／エクスポートは、`ThemeDefinition`を生成・保存するだけで追加できる。

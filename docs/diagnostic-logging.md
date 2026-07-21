# 診断ログ

ドッグフーディング中の問題を再現・調査するため、設定画面の「ドッグフーディング診断ログ」からロギングモードを有効化できる。

## 保存先と形式

- アプリデータディレクトリ配下の `logs/myfocus-diagnostic.jsonl`
- 1行1 JSONのJSON Lines形式
- 5 MBに達すると `myfocus-diagnostic.1.jsonl`へローテーション
- 現在ファイルと直前の1世代だけを保持
- 各レコードには日時、レベル、イベント、詳細、PID、OS、アプリバージョンを含む

設定画面では保存先と容量を確認でき、「ログフォルダーを開く」「ログを削除」を実行できる。

## 記録対象

- アプリおよびWebViewの起動
- ロギング設定の変更
- フィード更新の件数と失敗件数
- 記事クリーンアップ結果
- フロントエンドの未処理エラーとPromise rejection
- 明示的に送信された診断イベント

記事本文、AIプロンプト、認証情報は意図的に記録しない。ログは外部へ自動送信せず、端末内だけに保存する。

## 実装

- Rustロガー: `src-tauri/src/diagnostics.rs`
- Tauriコマンド: `diagnostic_log`, `get_diagnostic_info`, `clear_diagnostic_logs`, `open_diagnostic_folder`
- フロントエンド収集: `src/diagnostics.ts`
- 永続設定キー: `diagnostic_logging_enabled`

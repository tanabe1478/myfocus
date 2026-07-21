import { useEffect, useRef, useState } from "react";
import {
  clearDiagnosticLogs,
  closeSettings,
  getDiagnosticInfo,
  getSetting,
  listPiModels,
  openDiagnosticFolder,
  setSetting,
  type DiagnosticInfo,
} from "../api";
import {
  BUILTIN_THEMES,
  applyTheme,
  loadAndApplyTheme,
  saveTheme,
  type ThemeDefinition,
} from "../theme";
import {
  DEFAULT_SHORTCUTS,
  parseShortcuts,
  shortcutFromEvent,
  SHORTCUT_LABELS,
  type KeyboardShortcuts,
} from "../shortcuts";
import { ThemeEditor } from "./ThemeEditor";
import "../App.css";

const DEFAULT_TRANSLATE_MODEL = "openai-codex/gpt-5.6-luna";

export function SettingsWindow() {
  const [days, setDays] = useState("90");
  const [model, setModel] = useState(DEFAULT_TRANSLATE_MODEL);
  const [summaryModel, setSummaryModel] = useState(DEFAULT_TRANSLATE_MODEL);
  const [models, setModels] = useState<string[]>([]);
  const [shortcuts, setShortcuts] = useState<KeyboardShortcuts>(DEFAULT_SHORTCUTS);
  const [themeId, setThemeId] = useState(BUILTIN_THEMES[0].id);
  const [themes, setThemes] = useState<ThemeDefinition[]>(BUILTIN_THEMES);
  const [loggingEnabled, setLoggingEnabled] = useState(false);
  const [diagnosticInfo, setDiagnosticInfo] = useState<DiagnosticInfo | null>(null);
  const themeIdRef = useRef(BUILTIN_THEMES[0].id);
  const themeTouched = useRef(false);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getSetting("retention_days").then((v) => setDays(v ?? "90"));
    getSetting("translate_model").then((v) => setModel(v || DEFAULT_TRANSLATE_MODEL));
    getSetting("summary_model").then((v) => setSummaryModel(v || DEFAULT_TRANSLATE_MODEL));
    getSetting("keyboard_shortcuts").then((v) => setShortcuts(parseShortcuts(v)));
    getSetting("diagnostic_logging_enabled").then((v) => setLoggingEnabled(v === "true"));
    getDiagnosticInfo().then(setDiagnosticInfo).catch(() => {});
    loadAndApplyTheme().then(({ theme, catalog }) => {
      if (!themeTouched.current) {
        themeIdRef.current = theme.id;
        setThemeId(theme.id);
      }
      setThemes(catalog);
    });
    listPiModels().then(setModels).catch(() => setModels([]));
  }, []);

  const save = async () => {
    const n = parseInt(days, 10);
    if (isNaN(n) || n < 0) {
      setNote("保持期間には0以上の日数を入力してください");
      return;
    }
    const values = Object.values(shortcuts).map((v) => v.toLowerCase());
    if (new Set(values).size !== values.length) {
      setNote("同じショートカットを複数の操作には設定できません");
      return;
    }
    await Promise.all([
      setSetting("retention_days", String(n)),
      setSetting("translate_model", model),
      setSetting("summary_model", summaryModel),
      setSetting("keyboard_shortcuts", JSON.stringify(shortcuts)),
      saveTheme(themeIdRef.current),
    ]);
    setNote("保存しました");
  };

  return (
    <main className="settings-window" data-testid="settings-window">
      <div className="pane-header settings-window-header">
        <span className="pane-title">設定</span>
        <button
          className="icon-button"
          data-testid="close-settings"
          title="閉じる"
          onClick={() => closeSettings()}
        >
          ×
        </button>
      </div>

      <div className="settings-window-body">
        <section className="settings-section">
          <div className="settings-title">外観</div>
          <select
            className="settings-model-input settings-wide-input"
            data-testid="theme-select"
            value={themeId}
            onChange={(e) => {
              const id = e.target.value;
              themeTouched.current = true;
              themeIdRef.current = id;
              setThemeId(id);
              const theme = themes.find((candidate) => candidate.id === id);
              if (theme) applyTheme(theme);
              saveTheme(id)
                .then(() => setNote("テーマを保存しました"))
                .catch((error) => setNote(`テーマを保存できませんでした: ${error}`));
            }}
          >
            {themes.map((theme) => (
              <option key={theme.id} value={theme.id}>
                {theme.name}{theme.builtin ? "" : "（カスタム）"}
              </option>
            ))}
          </select>
          <div className="settings-hint">
            選択するとすぐに保存され、すべてのウィンドウへ反映されます。
          </div>
          <ThemeEditor
            themes={themes}
            selectedId={themeId}
            onThemesChange={setThemes}
            onSelectedChange={(id) => {
              themeTouched.current = true;
              themeIdRef.current = id;
              setThemeId(id);
            }}
            onNote={setNote}
          />
        </section>

        <section className="settings-section">
          <div className="settings-title">記事の保持期間</div>
          <div className="settings-row">
            <input
              data-testid="retention-days"
              type="number"
              min={0}
              value={days}
              onChange={(e) => setDays(e.target.value)}
            />
            <span>日</span>
          </div>
          <div className="settings-hint">
            既読記事をこの日数の経過後に自動削除します。スター付きは削除されません。0で無効。
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-title">RSS記事要約のモデル</div>
          <select
            className="settings-model-input settings-wide-input"
            value={summaryModel}
            onChange={(e) => setSummaryModel(e.target.value)}
          >
            {models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
            {!models.includes(summaryModel) && (
              <option value={summaryModel}>{summaryModel}</option>
            )}
          </select>
          <div className="settings-hint">
            記事を開いて「AI要約を生成」したときだけ使用されます。
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-title">Hacker News翻訳・要約のモデル</div>
          <select
            className="settings-model-input settings-wide-input"
            value={model}
            onChange={(e) => setModel(e.target.value)}
          >
            {models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
            {!models.includes(model) && <option value={model}>{model}</option>}
          </select>
          <div className="settings-hint">
            Hacker Newsの新しい翻訳・要約から適用されます。既存の生成結果は保持されます。
          </div>
        </section>

        <section className="settings-section" data-testid="diagnostic-settings">
          <div className="settings-title">ドッグフーディング診断ログ</div>
          <label className="settings-toggle-row">
            <span>
              <strong>ロギングモード</strong>
              <span className="settings-hint">
                アプリの動作、フィード更新結果、フロントエンドエラーをローカルへ記録します。
              </span>
            </span>
            <input
              type="checkbox"
              role="switch"
              data-testid="diagnostic-logging-toggle"
              checked={loggingEnabled}
              onChange={async (event) => {
                const next = event.target.checked;
                setLoggingEnabled(next);
                try {
                  await setSetting("diagnostic_logging_enabled", String(next));
                  setDiagnosticInfo(await getDiagnosticInfo());
                  setNote(next ? "診断ログを有効にしました" : "診断ログを無効にしました");
                } catch (error) {
                  setLoggingEnabled(!next);
                  setNote(`診断ログ設定を変更できませんでした: ${error}`);
                }
              }}
            />
          </label>
          <div className="settings-hint diagnostic-privacy-note">
            記事本文やAIプロンプトは記録しません。ログは5MBでローテーションし、直前の1世代を保持します。
          </div>
          {diagnosticInfo && (
            <div className="diagnostic-log-info">
              <code title={diagnosticInfo.file}>{diagnosticInfo.file}</code>
              <span>{(diagnosticInfo.sizeBytes / 1024).toFixed(1)} KB</span>
            </div>
          )}
          <div className="diagnostic-actions">
            <button
              className="settings-reset"
              data-testid="open-diagnostic-folder"
              onClick={() =>
                openDiagnosticFolder().catch((error) =>
                  setNote(`ログフォルダーを開けませんでした: ${error}`),
                )
              }
            >
              ログフォルダーを開く
            </button>
            <button
              className="settings-reset"
              data-testid="clear-diagnostic-logs"
              onClick={async () => {
                try {
                  await clearDiagnosticLogs();
                  setDiagnosticInfo(await getDiagnosticInfo());
                  setNote("診断ログを削除しました");
                } catch (error) {
                  setNote(`診断ログを削除できませんでした: ${error}`);
                }
              }}
            >
              ログを削除
            </button>
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-title">キーボードショートカット</div>
          <div className="settings-hint settings-shortcut-hint">
            入力欄を選択し、割り当てたいキーを押してください。Modは⌘またはCtrlです。
          </div>
          <div className="shortcut-list">
            {SHORTCUT_LABELS.map(([action, label]) => (
              <label className="shortcut-row" key={action}>
                <span>{label}</span>
                <input
                  readOnly
                  value={shortcuts[action]}
                  onKeyDown={(e) => {
                    e.preventDefault();
                    if (e.key === "Escape") {
                      e.currentTarget.blur();
                      return;
                    }
                    const value = shortcutFromEvent(e);
                    if (value) setShortcuts((cur) => ({ ...cur, [action]: value }));
                  }}
                />
              </label>
            ))}
          </div>
          <button
            className="settings-reset"
            onClick={() => setShortcuts(DEFAULT_SHORTCUTS)}
          >
            初期設定に戻す
          </button>
        </section>

        <div className="settings-footer">
          {note && (
            <span className="settings-note" data-testid="settings-note">
              {note}
            </span>
          )}
          <button className="settings-save" data-testid="save-settings" onClick={save}>
            保存
          </button>
        </div>
      </div>
    </main>
  );
}

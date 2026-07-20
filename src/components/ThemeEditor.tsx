import { useRef, useState } from "react";
import {
  BUILTIN_THEMES,
  applyTheme,
  parseCustomThemes,
  resolveThemeTokens,
  saveCustomThemes,
  saveTheme,
  type ThemeDefinition,
  type ThemeTokens,
} from "../theme";

const TOKEN_LABELS: [keyof ThemeTokens, string][] = [
  ["bgSidebar", "サイドバー背景"],
  ["bgList", "リスト背景"],
  ["bgReading", "本文背景"],
  ["bgHover", "ホバー背景"],
  ["bgSelected", "選択背景"],
  ["text", "本文文字"],
  ["textSecondary", "補助文字"],
  ["textMuted", "薄い文字"],
  ["border", "境界線"],
  ["accent", "アクセント"],
  ["accentSoft", "薄いアクセント"],
  ["shadow", "影"],
];

interface Props {
  themes: ThemeDefinition[];
  selectedId: string;
  onThemesChange: (themes: ThemeDefinition[]) => void;
  onSelectedChange: (id: string) => void;
  onNote: (note: string) => void;
}

function editableCopy(source: ThemeDefinition): ThemeDefinition {
  return {
    id: `user:${crypto.randomUUID()}`,
    name: `${source.name} のコピー`,
    appearance: source.appearance,
    base: source.appearance === "dark" ? "warm-dark" : "warm-light",
    tokens: resolveThemeTokens(source),
  };
}

export function ThemeEditor({
  themes,
  selectedId,
  onThemesChange,
  onSelectedChange,
  onNote,
}: Props) {
  const [draft, setDraft] = useState<ThemeDefinition | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const selected = themes.find((theme) => theme.id === selectedId) ?? BUILTIN_THEMES[0];
  const customs = themes.filter((theme) => !theme.builtin);

  const updateDraft = (next: ThemeDefinition) => {
    setDraft(next);
    applyTheme(next);
  };

  const saveDraft = async () => {
    if (!draft?.name.trim()) {
      onNote("テーマ名を入力してください");
      return;
    }
    const saved = { ...draft, name: draft.name.trim(), builtin: undefined };
    const nextCustoms = [...customs.filter((theme) => theme.id !== saved.id), saved];
    await saveCustomThemes(nextCustoms);
    await saveTheme(saved.id);
    onThemesChange([...BUILTIN_THEMES, ...nextCustoms]);
    onSelectedChange(saved.id);
    setDraft(null);
    onNote("カスタムテーマを保存しました");
  };

  const deleteSelected = async () => {
    if (selected.builtin || !confirm(`「${selected.name}」を削除しますか？`)) return;
    const nextCustoms = customs.filter((theme) => theme.id !== selected.id);
    await saveCustomThemes(nextCustoms);
    await saveTheme(BUILTIN_THEMES[0].id);
    onThemesChange([...BUILTIN_THEMES, ...nextCustoms]);
    onSelectedChange(BUILTIN_THEMES[0].id);
    setDraft(null);
    onNote("カスタムテーマを削除しました");
  };

  const importFile = async (file: File) => {
    const imported = parseCustomThemes(await file.text());
    if (imported.length === 0) {
      onNote("有効なテーマが見つかりませんでした");
      return;
    }
    const byId = new Map(customs.map((theme) => [theme.id, theme]));
    for (const theme of imported) byId.set(theme.id, theme);
    const nextCustoms = [...byId.values()];
    await saveCustomThemes(nextCustoms);
    onThemesChange([...BUILTIN_THEMES, ...nextCustoms]);
    onSelectedChange(imported[0].id);
    await saveTheme(imported[0].id);
    onNote(`${imported.length}件のテーマをインポートしました`);
  };

  const exportSelected = async () => {
    if (selected.builtin) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(selected, null, 2));
      onNote("テーマJSONをクリップボードへコピーしました");
    } catch {
      onNote("クリップボードへコピーできませんでした");
    }
  };

  if (draft) {
    const tokens = resolveThemeTokens(draft);
    return (
      <div className="theme-editor" data-testid="theme-editor">
        <label className="theme-editor-field">
          <span>テーマ名</span>
          <input
            data-testid="theme-name"
            value={draft.name}
            onChange={(event) => updateDraft({ ...draft, name: event.target.value })}
          />
        </label>
        <label className="theme-editor-field">
          <span>基調</span>
          <select
            value={draft.appearance}
            onChange={(event) => {
              const appearance = event.target.value as "light" | "dark";
              updateDraft({
                ...draft,
                appearance,
                base: appearance === "dark" ? "warm-dark" : "warm-light",
              });
            }}
          >
            <option value="light">ライト</option>
            <option value="dark">ダーク</option>
          </select>
        </label>
        <div className="theme-token-grid">
          {TOKEN_LABELS.map(([key, label]) => (
            <label className="theme-token-row" key={key}>
              <span>{label}</span>
              {key !== "shadow" && /^#[0-9a-f]{6}$/i.test(tokens[key]) && (
                <input
                  className="theme-color-input"
                  type="color"
                  value={tokens[key]}
                  onChange={(event) =>
                    updateDraft({
                      ...draft,
                      tokens: { ...draft.tokens, [key]: event.target.value },
                    })
                  }
                />
              )}
              <input
                className="theme-token-value"
                value={tokens[key]}
                onChange={(event) =>
                  updateDraft({
                    ...draft,
                    tokens: { ...draft.tokens, [key]: event.target.value },
                  })
                }
              />
            </label>
          ))}
        </div>
        <div className="theme-editor-actions">
          <button
            onClick={() => {
              applyTheme(selected);
              setDraft(null);
            }}
          >
            キャンセル
          </button>
          <button className="settings-save" data-testid="save-custom-theme" onClick={saveDraft}>
            テーマを保存
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="theme-manager-actions">
      <button onClick={() => setDraft(editableCopy(selected))}>
        {selected.builtin ? "複製して編集" : "複製"}
      </button>
      {!selected.builtin && (
        <>
          <button
            data-testid="edit-custom-theme"
            onClick={() =>
              setDraft({ ...selected, tokens: resolveThemeTokens(selected) })
            }
          >
            編集
          </button>
          <button onClick={exportSelected}>JSONをコピー</button>
          <button className="theme-delete" onClick={deleteSelected}>
            削除
          </button>
        </>
      )}
      <button onClick={() => fileRef.current?.click()}>JSONをインポート</button>
      <input
        ref={fileRef}
        type="file"
        accept="application/json,.json"
        hidden
        onChange={(event) => {
          const file = event.target.files?.[0];
          if (file) void importFile(file);
          event.target.value = "";
        }}
      />
    </div>
  );
}

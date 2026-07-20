export interface KeyboardShortcuts {
  search: string;
  nextArticle: string;
  previousArticle: string;
  nextFeed: string;
  previousFeed: string;
  openBackground: string;
  markAllRead: string;
  openSettings: string;
}

export const DEFAULT_SHORTCUTS: KeyboardShortcuts = {
  search: "Mod+K",
  nextArticle: "J",
  previousArticle: "K",
  nextFeed: "Shift+J",
  previousFeed: "Shift+K",
  openBackground: "B",
  markAllRead: "Shift+A",
  openSettings: "Mod+,",
};

export const SHORTCUT_LABELS: Array<[keyof KeyboardShortcuts, string]> = [
  ["search", "記事を検索"],
  ["nextArticle", "次の記事"],
  ["previousArticle", "前の記事"],
  ["nextFeed", "次のフィード／カテゴリ"],
  ["previousFeed", "前のフィード／カテゴリ"],
  ["openBackground", "ブラウザで開く"],
  ["markAllRead", "まとめて既読"],
  ["openSettings", "設定を開く"],
];

export function parseShortcuts(value: string | null): KeyboardShortcuts {
  if (!value) return DEFAULT_SHORTCUTS;
  try {
    const parsed = JSON.parse(value) as Partial<KeyboardShortcuts>;
    return Object.fromEntries(
      Object.entries(DEFAULT_SHORTCUTS).map(([key, fallback]) => {
        const candidate = parsed[key as keyof KeyboardShortcuts];
        return [key, typeof candidate === "string" && candidate ? candidate : fallback];
      })
    ) as unknown as KeyboardShortcuts;
  } catch {
    return DEFAULT_SHORTCUTS;
  }
}

export function shortcutFromEvent(e: KeyboardEvent | React.KeyboardEvent): string | null {
  if (["Control", "Meta", "Alt", "Shift"].includes(e.key)) return null;
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("Mod");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(normalizeKey(e.key));
  return parts.join("+");
}

export function matchesShortcut(e: KeyboardEvent, shortcut: string): boolean {
  const parts = shortcut.split("+");
  const key = parts[parts.length - 1]?.toLowerCase();
  if (!key) return false;
  if ((e.metaKey || e.ctrlKey) !== parts.includes("Mod")) return false;
  if (e.altKey !== parts.includes("Alt")) return false;
  if (e.shiftKey !== parts.includes("Shift")) return false;
  return normalizeKey(e.key).toLowerCase() === key;
}

function normalizeKey(key: string): string {
  if (key === " ") return "Space";
  if (key.length === 1) return key.toUpperCase();
  return key;
}

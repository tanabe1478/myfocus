import { listen } from "@tauri-apps/api/event";
import { getSetting, setSetting } from "./api";

export const THEME_SETTING_KEY = "theme_id";
export const CUSTOM_THEMES_SETTING_KEY = "custom_themes";
const THEME_CACHE_KEY = "myfocus.theme";
const CUSTOM_THEMES_CACHE_KEY = "myfocus.customThemes";
let applyGeneration = 0;

export type ThemeAppearance = "light" | "dark";

export interface ThemeTokens {
  bgSidebar: string;
  bgList: string;
  bgReading: string;
  bgHover: string;
  bgSelected: string;
  text: string;
  textSecondary: string;
  textMuted: string;
  border: string;
  accent: string;
  accentSoft: string;
  shadow: string;
}

/**
 * User themes use the same shape as built-ins. `base` allows a custom theme to
 * override only selected tokens, so adding a new token later remains backward
 * compatible with existing themes.
 */
export interface ThemeDefinition {
  id: string;
  name: string;
  appearance: ThemeAppearance;
  base?: "warm-light" | "warm-dark";
  tokens: Partial<ThemeTokens>;
  builtin?: boolean;
}

const LIGHT_TOKENS: ThemeTokens = {
  bgSidebar: "#f2efe9",
  bgList: "#faf8f4",
  bgReading: "#fffdf9",
  bgHover: "#ece8df",
  bgSelected: "#e6e0d3",
  text: "#35322c",
  textSecondary: "#6b665c",
  textMuted: "#9b9588",
  border: "#e3ded3",
  accent: "#c26d4a",
  accentSoft: "#c26d4a22",
  shadow: "0 8px 32px rgba(50, 45, 35, 0.18)",
};

const DARK_TOKENS: ThemeTokens = {
  bgSidebar: "#1c1b18",
  bgList: "#211f1c",
  bgReading: "#262421",
  bgHover: "#2b2925",
  bgSelected: "#33302a",
  text: "#dcd8cf",
  textSecondary: "#a29c90",
  textMuted: "#7d786d",
  border: "#33302a",
  accent: "#d08159",
  accentSoft: "#d0815926",
  shadow: "0 8px 32px rgba(0, 0, 0, 0.5)",
};

export const BUILTIN_THEMES: ThemeDefinition[] = [
  {
    id: "warm-light",
    name: "ライト（現在のテーマ）",
    appearance: "light",
    tokens: LIGHT_TOKENS,
    builtin: true,
  },
  {
    id: "warm-dark",
    name: "ダーク",
    appearance: "dark",
    tokens: DARK_TOKENS,
    builtin: true,
  },
];

const CSS_TOKEN_NAMES: Record<keyof ThemeTokens, string> = {
  bgSidebar: "--bg-sidebar",
  bgList: "--bg-list",
  bgReading: "--bg-reading",
  bgHover: "--bg-hover",
  bgSelected: "--bg-selected",
  text: "--text",
  textSecondary: "--text-secondary",
  textMuted: "--text-muted",
  border: "--border",
  accent: "--accent",
  accentSoft: "--accent-soft",
  shadow: "--shadow",
};

function validCustomTheme(value: unknown): value is ThemeDefinition {
  if (!value || typeof value !== "object") return false;
  const theme = value as Partial<ThemeDefinition>;
  return (
    typeof theme.id === "string" &&
    theme.id.length > 0 &&
    theme.id.length <= 80 &&
    !BUILTIN_THEMES.some((builtin) => builtin.id === theme.id) &&
    typeof theme.name === "string" &&
    theme.name.length > 0 &&
    (theme.appearance === "light" || theme.appearance === "dark") &&
    !!theme.tokens &&
    typeof theme.tokens === "object" &&
    Object.values(theme.tokens).every(
      (token) => typeof token === "string" && token.length <= 200 && !/url\s*\(/i.test(token)
    )
  );
}

export function parseCustomThemes(raw: string | null): ThemeDefinition[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    const values = Array.isArray(parsed) ? parsed : [parsed];
    return values.filter((value): value is ThemeDefinition => {
      return validCustomTheme(value);
    });
  } catch {
    return [];
  }
}

export function resolveThemeTokens(theme: ThemeDefinition): ThemeTokens {
  const baseId = theme.base ?? (theme.appearance === "dark" ? "warm-dark" : "warm-light");
  const base = baseId === "warm-dark" ? DARK_TOKENS : LIGHT_TOKENS;
  return { ...base, ...theme.tokens };
}

export function applyTheme(theme: ThemeDefinition): void {
  applyGeneration += 1;
  const root = document.documentElement;
  const tokens = resolveThemeTokens(theme);
  for (const [key, cssName] of Object.entries(CSS_TOKEN_NAMES) as [
    keyof ThemeTokens,
    string,
  ][]) {
    root.style.setProperty(cssName, tokens[key]);
  }
  root.dataset.theme = theme.id;
  root.style.colorScheme = theme.appearance;
}

function cachedCatalog(): ThemeDefinition[] {
  return [
    ...BUILTIN_THEMES,
    ...parseCustomThemes(localStorage.getItem(CUSTOM_THEMES_CACHE_KEY)),
  ];
}

function applyById(id: string | null, catalog = cachedCatalog()): ThemeDefinition {
  const theme = catalog.find((candidate) => candidate.id === id) ?? BUILTIN_THEMES[0];
  applyTheme(theme);
  localStorage.setItem(THEME_CACHE_KEY, theme.id);
  return theme;
}

export function applyCachedTheme(): void {
  applyById(localStorage.getItem(THEME_CACHE_KEY));
}

export async function loadThemeCatalog(): Promise<ThemeDefinition[]> {
  const raw = await getSetting(CUSTOM_THEMES_SETTING_KEY);
  if (raw) localStorage.setItem(CUSTOM_THEMES_CACHE_KEY, raw);
  else localStorage.removeItem(CUSTOM_THEMES_CACHE_KEY);
  return [...BUILTIN_THEMES, ...parseCustomThemes(raw)];
}

export async function loadAndApplyTheme(force = false): Promise<{
  theme: ThemeDefinition;
  catalog: ThemeDefinition[];
}> {
  const generation = applyGeneration;
  const [id, catalog] = await Promise.all([
    getSetting(THEME_SETTING_KEY),
    loadThemeCatalog(),
  ]);
  const theme = catalog.find((candidate) => candidate.id === id) ?? BUILTIN_THEMES[0];
  // Preview changes protect themselves from an older async read. Explicit
  // synchronization points (startup, focus and cross-window events) treat the
  // SQLite setting as authoritative so windows cannot remain out of sync.
  if (force || generation === applyGeneration) applyById(theme.id, catalog);
  return { theme, catalog };
}

export async function saveTheme(themeId: string): Promise<void> {
  const catalog = await loadThemeCatalog();
  const theme = applyById(themeId, catalog);
  await setSetting(THEME_SETTING_KEY, theme.id);
}

export async function saveCustomThemes(themes: ThemeDefinition[]): Promise<void> {
  const valid = themes.filter(validCustomTheme).map(({ builtin: _, ...theme }) => theme);
  const raw = JSON.stringify(valid);
  localStorage.setItem(CUSTOM_THEMES_CACHE_KEY, raw);
  await setSetting(CUSTOM_THEMES_SETTING_KEY, raw);
}

/** Apply cached colors synchronously, then follow DB changes from any window. */
export function initializeThemeSync(): void {
  applyCachedTheme();

  const syncFromDatabase = (retries = 4) => {
    void loadAndApplyTheme(true).catch(() => {
      // On native startup the WebView can execute before Tauri's invoke bridge
      // is ready. Do not interpret that transient failure as a light theme.
      if (retries > 0) {
        window.setTimeout(() => syncFromDatabase(retries - 1), 150);
      }
    });
  };

  syncFromDatabase();
  void listen<string>("settings-updated", (event) => {
    if (event.payload === THEME_SETTING_KEY || event.payload === CUSTOM_THEMES_SETTING_KEY) {
      syncFromDatabase();
    }
  });

  // Native window focus changes are a reliable synchronization boundary even
  // when a platform drops an event while another WebView is hidden.
  window.addEventListener("focus", () => syncFromDatabase());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") syncFromDatabase();
  });
}

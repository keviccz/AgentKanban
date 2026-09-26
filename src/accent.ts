// Theme colors. "teal" is the original look and keeps its separate blue for links;
// every other choice colors both. The logo always stays brand teal (--brand).
export const ACCENTS = ['teal', 'blue', 'indigo', 'purple', 'rose', 'orange', 'green', 'graphite'] as const;
export type Accent = typeof ACCENTS[number];

export const accentLabels: Record<Accent, string> = {
  teal: '青绿', blue: '湖蓝', indigo: '靛青', purple: '葡萄紫', rose: '玫红', orange: '琥珀橙', green: '松绿', graphite: '石墨',
};

const palette: Record<Exclude<Accent, 'teal'>, { light: string; dark: string }> = {
  blue: { light: '#1f6fd1', dark: '#6aa9ff' },
  indigo: { light: '#4f53c9', dark: '#9da2ff' },
  purple: { light: '#8446c2', dark: '#c49af0' },
  rose: { light: '#c23a67', dark: '#f283a8' },
  orange: { light: '#c05a0a', dark: '#f5a55a' },
  green: { light: '#2d7f47', dark: '#7fcb97' },
  graphite: { light: '#475261', dark: '#b8c2ce' },
};

/** Swatch color shown in Settings, independent of the current theme. */
export const swatch = (accent: Accent) => accent === 'teal' ? '#007f89' : palette[accent].light;

export function applyAccent(accent: Accent, theme: 'light' | 'dark') {
  const style = document.documentElement.style;
  if (accent === 'teal' || !palette[accent]) {
    for (const name of ['--teal', '--accent', '--focus']) style.removeProperty(name);
    return;
  }
  const color = palette[accent][theme];
  for (const name of ['--teal', '--accent', '--focus']) style.setProperty(name, color);
}

/** Project label colors; keys match the native validation list. */
export const PROJECT_COLORS = ['red', 'orange', 'yellow', 'green', 'teal', 'blue', 'purple', 'gray'] as const;
export type ProjectColor = typeof PROJECT_COLORS[number];

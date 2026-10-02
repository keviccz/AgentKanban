// Which terminal a pet stands for, and how that shows. The only input is the Agent name
// the client reported on its tasks ("Claude Code", "Codex", ...), so no Agent needs to
// send anything new. Names nobody drew get a color and mark picked from the name.

/** A small pixel badge above the head: '#' is the main color, '+' and '*' the second and third. */
export interface Mark { rows: string[]; colors: Record<string, string> }

export interface Look {
  /** Matched terminal id, or null for a name nobody drew. */
  id: string | null;
  /** Head frame and status light. */
  color: string;
  /** Legs, torso and arms; a shade of the head color when absent. */
  body?: string;
  mark: Mark;
}

const mark = (rows: string[], main: string, second = main, third = second): Mark => ({ rows, colors: { '#': main, '+': second, '*': third } });

// Pixel nods to each terminal's colors, not their logos.
const KNOWN: { id: string; match: string; look: Omit<Look, 'id'> }[] = [
  { id: 'claude', match: 'claude', look: { color: '#d97757', mark: mark(['#.#.#', '.###.', '#####', '.###.', '#.#.#'], '#e8875f') } },
  { id: 'codex', match: 'codex', look: { color: '#eef1f3', body: '#4a5057', mark: mark(['******', '*#****', '**#***', '*#*##*', '******'], '#eef1f3', '#eef1f3', '#1d2329') } },
  { id: 'cursor', match: 'cursor', look: { color: '#7d8fa3', mark: mark(['.+++.', '#+++*', '###**', '###**', '.##*.'], '#b9c3cc', '#f2f4f6', '#5b6670') } },
  { id: 'gemini', match: 'gemini', look: { color: '#a68cf0', mark: mark(['..#..', '.###.', '#####', '.+++.', '..+..'], '#5fa8f5', '#c27ad8') } },
  { id: 'deepseek', match: 'deepseek', look: { color: '#5b78ff', mark: mark(['#...#', '##.##', '.###.', '..#..', '..#..'], '#7d93ff') } },
];

// For other names; none of these is a terminal color above or the default theme teal.
const COLORS = ['#f283a8', '#6cc7f0', '#a6d96a', '#e0c27a'];
const SHAPES = [
  ['.###.', '#...#', '#...#', '#...#', '.###.'],
  ['..#..', '.#.#.', '#...#', '#####'],
  ['.#.#.', '#####', '#####', '.###.', '..#..'],
  ['...##', '..##.', '.####', '..##.', '.##..'],
];

const squash = (name: string) => name.toLowerCase().replace(/[^a-z0-9]/g, '');

export function knownLook(name: string): Look | null {
  const key = squash(name);
  const hit = key ? KNOWN.find(entry => key.includes(entry.match)) : undefined;
  return hit ? { id: hit.id, ...hit.look } : null;
}

const hash = (text: string) => [...text].reduce((h, ch) => (h * 31 + ch.charCodeAt(0)) >>> 0, 7);

/**
 * Looks for the pets on screen, by name. An empty name (the board's resting pet) gets
 * none. Other names keep the color their name picks unless a name sorted before them
 * already wears it, so every pet window settles on the same colors.
 */
export function looksFor(names: string[]): Map<string, Look> {
  const looks = new Map<string, Look>();
  const used = new Set<string>();
  for (const name of [...new Set(names)].filter(Boolean).sort()) {
    const known = knownLook(name);
    if (known) { looks.set(name, known); continue; }
    const h = hash(squash(name) || name);
    let color = COLORS[h % COLORS.length];
    for (let step = 1; used.has(color) && step < COLORS.length; step++) color = COLORS[(h + step) % COLORS.length];
    used.add(color);
    looks.set(name, { id: null, color, mark: mark(SHAPES[Math.floor(h / COLORS.length) % SHAPES.length], color) });
  }
  return looks;
}

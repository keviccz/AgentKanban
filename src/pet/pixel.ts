// The pixel Agent from the promo (direction B): a square screen for a head whose face is
// drawn in pixels. Coordinates are grid units; the feet sit at (0, 0), the head spans
// y -15..-5 and the status light y -18..-16.

export type Face = 'eyes' | 'blink' | 'happy' | 'dots' | 'q' | 'ok' | 'up' | 'sleep';
export type Arm = 'down' | 'up' | 'fwd' | 'out' | 'hold';
export type Legs = 'stand' | 'a' | 'b' | 'tuck';
/** x, y, width, height, color, opacity */
export type Rect = [number, number, number, number, string, number?];

export const SCREEN = '#0a1315';
export const PAPER = '#eef3f4';
export const INK = '#b7c2c7';
export const INK_DARK = '#6d7a82';

const GLYPHS: Record<string, string[]> = {
  q: ['.###.', '#...#', '....#', '..##.', '..#..', '.....', '..#..'],
  ok: ['......#', '.....#.', '#...#..', '.#.#...', '..#....'],
  hat: ['.#.', '#.#'],
  z: ['####', '..#.', '.#..', '####'],
};

const DIGITS: Record<string, string[]> = {
  '0': ['###', '#.#', '#.#', '#.#', '###'],
  '1': ['.#.', '##.', '.#.', '.#.', '###'],
  '2': ['###', '..#', '###', '#..', '###'],
  '3': ['###', '..#', '###', '..#', '###'],
  '4': ['#.#', '#.#', '###', '..#', '..#'],
  '5': ['###', '#..', '###', '..#', '###'],
  '6': ['###', '#..', '###', '#.#', '###'],
  '7': ['###', '..#', '..#', '..#', '..#'],
  '8': ['###', '#.#', '###', '#.#', '###'],
  '9': ['###', '#.#', '###', '..#', '###'],
  '/': ['..#', '..#', '.#.', '#..', '#..'],
};

const ARMS: Record<Arm, [number, number, number, number][]> = {
  down: [[-5, -5, 1, 2]],
  up: [[-6, -5, 2, 1], [-7, -6, 1, 1], [-8, -9, 1, 3]],
  fwd: [[-8, -5, 4, 1]],
  out: [[-6, -5, 1, 1], [-7, -4, 1, 1]],
  hold: [[-6, -5, 2, 1], [-7, -6, 1, 1], [-8, -7, 1, 2]],
};

export const glyph = (rows: string[], gx: number, gy: number, c: string, o?: number): Rect[] =>
  rows.flatMap((row, j) => [...row].flatMap((ch, i) => (ch === '#' ? [[gx + i, gy + j, 1, 1, c, o] as Rect] : [])));

/** Pixel digits ("3/5"); 3 units wide each with a 1-unit gap. */
export const digits = (text: string, gx: number, gy: number, c: string): Rect[] =>
  [...text].flatMap((ch, k) => (DIGITS[ch] ? glyph(DIGITS[ch], gx + k * 4, gy, c) : []));
export const digitsWidth = (text: string) => text.length * 4 - 1;

const shade = (hex: string, k: number) => '#' + [1, 3, 5].map(i => Math.round(parseInt(hex.slice(i, i + 2), 16) * (1 - k)).toString(16).padStart(2, '0')).join('');

export type Pose = {
  color: string;
  /** Legs, torso and arms; a shade of `color` when absent. */
  body?: string;
  /** Terminal badge above the head's right corner (see look.ts). */
  mark?: { rows: string[]; colors: Record<string, string> };
  face?: Face;
  look?: number;
  glyph?: string;
  dot?: string | null;
  dotOn?: boolean;
  armL?: Arm;
  armR?: Arm;
  legs?: Legs;
  bob?: number;
  dots?: number;
  carry?: boolean;
  /** 0..1: sleep sinks the body one unit */
  sink?: boolean;
};

export function agentRects(p: Pose): Rect[] {
  const body = p.body ?? shade(p.color, 0.42);
  const y = -(p.bob ?? 0) + (p.sink ? 1 : 0);
  const out: Rect[] = [];
  const at = (dx: number, dy: number, w: number, h: number, c: string, o?: number) => out.push([dx, y + dy, w, h, c, o]);
  const lift = Math.min(Math.max(p.bob ?? 0, 0), 3);
  out.push([-5 + lift, 0, 10 - 2 * lift, 1, '#000000', 0.35]);
  const legs = p.legs ?? 'stand';
  const tuck = legs === 'tuck';
  const ly = (side: number) => (tuck || (legs === 'a' && side < 0) || (legs === 'b' && side > 0) ? -3 : -2);
  at(-4, ly(-1) - (p.sink ? 1 : 0), 2, tuck || p.sink ? 1 : 2, body);
  at(2, ly(1) - (p.sink ? 1 : 0), 2, tuck || p.sink ? 1 : 2, body);
  at(-4, -5, 8, 3, body);
  for (const [side, arm] of [[-1, p.armL ?? 'down'], [1, p.armR ?? 'down']] as const) {
    for (const [ax, ay, w, h] of ARMS[arm]) at(side < 0 ? ax : -ax - w, ay, w, h, body);
  }
  at(-7, -14, 14, 8, p.color);
  at(-6, -15, 12, 1, p.color);
  at(-6, -6, 12, 1, p.color);
  at(-6, -14, 12, 8, SCREEN);
  const gx = -6 + (p.look ?? 0);
  const gy = y - 14;
  const gc = p.glyph ?? p.color;
  switch (p.face ?? 'eyes') {
    case 'eyes': out.push([gx + 3, gy + 2, 2, 3, gc], [gx + 7, gy + 2, 2, 3, gc]); break;
    case 'up': out.push([gx + 3, gy + 1, 2, 3, gc], [gx + 7, gy + 1, 2, 3, gc]); break;
    case 'blink': out.push([gx + 3, gy + 4, 2, 1, gc], [gx + 7, gy + 4, 2, 1, gc]); break;
    case 'sleep': out.push([gx + 3, gy + 5, 2, 1, gc, 0.7], [gx + 7, gy + 5, 2, 1, gc, 0.7]); break;
    case 'happy': out.push(...glyph(GLYPHS.hat, gx + 2, gy + 3, gc), ...glyph(GLYPHS.hat, gx + 7, gy + 3, gc)); break;
    case 'dots': for (let i = 0; i < 3; i++) out.push([gx + 1 + i * 4, gy + 4, 2, 2, gc, i < (p.dots ?? 3) ? 1 : 0.25]); break;
    case 'q': out.push(...glyph(GLYPHS.q, -2, gy + 1, gc)); break;
    case 'ok': out.push(...glyph(GLYPHS.ok, -4, gy + 2, gc)); break;
  }
  if (p.dot) at(-1, -18, 2, 2, p.dot, p.dotOn === false ? 0.25 : 1);
  // The badge stands on the head's top edge, right of the status light, with a drop
  // shadow so light badges still read on a light wallpaper.
  if (p.mark) {
    const { rows, colors } = p.mark;
    const top = -15 - rows.length;
    const pixels = rows.flatMap((row, j) => [...row].flatMap((ch, i) => (colors[ch] ? [[2 + i, top + j, colors[ch]] as const] : [])));
    for (const [x, py] of pixels) at(x + 1, py + 1, 1, 1, '#000000', 0.4);
    for (const [x, py, c] of pixels) at(x, py, 1, 1, c);
  }
  if (p.carry) {
    at(-8, -21, 16, 6, PAPER);
    at(-6, -19, 9, 1, INK_DARK);
    at(-6, -17, 12, 1, INK);
  }
  return out;
}

export const OK_GLYPH = GLYPHS.ok;
export const Z_GLYPH = GLYPHS.z;

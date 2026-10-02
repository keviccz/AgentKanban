import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { native, onPetAction, onPetDropped, onPetMotion, onPetOptions, onPreferencesChanged, petAttention, petDrag, petHello, petLayout, petMenu, petReady, petOpen, petResize, readPreferences, readRevision, readSnapshot, readTrackingPaused, type PetAction, type PetMotion, type PetOptions } from '../bridge';
import { accentOnDark } from '../accent';
import { resolveLanguage, setLanguage, t } from '../i18n';
import { defaults, type Preferences, type Snapshot } from '../types';
import { agentRects, digits, digitsWidth, glyph, OK_GLYPH, Z_GLYPH, type Legs, type Pose, type Rect } from './pixel';
import { looksFor, type Look } from './look';
import { derivePets, diffEvents, EVENT_SECONDS, marksOf, type PetEvent, type PetEventKind, type PetView, type TaskMark } from './model';
import './pet.css';

// A desktop pet window: pixel Agents acting out the board. Merged, one window shows every
// pet; split, each window shows the one Agent Rust assigned it. Sizes are logical pixels;
// the pets are drawn on a 3px grid and move on their own 12 fps clock. Rust moves the
// window itself (docking, dragging, gravity) and tells us how it is moving.

const U = 3;
const SLOT = 96;
const HEIGHT = 132; // matches pet::HEIGHT in src-tauri
const COLS = SLOT / U;
const ROWS = HEIGHT / U;
const FEET = 42;
const X = 11;
const AMBER = '#f3b34e';
const GREEN = '#87c5a3';
const GREY = '#8c96a0';
const PIN = '#e5534b';
const WALK: Legs[] = ['a', 'stand', 'b', 'stand'];
const UP_ARROW = ['..#..', '.###.', '#.#.#', '..#..', '..#..'];
const DOWN_ARROW = [...UP_ARROW].reverse();
const ACTION_SECONDS = 0.9;

type Action = PetAction | 'nope';
interface Played { kind: Action; at: number }

const reduceMotion = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
const clock = () => performance.now() / 1000;

function useClock() {
  const [time, setTime] = useState(clock);
  useEffect(() => {
    const id = window.setInterval(() => setTime(clock()), reduceMotion() ? 1000 : 1000 / 12);
    return () => window.clearInterval(id);
  }, []);
  return time;
}

const moodText = (pet: PetView, paused: boolean) => paused ? t("记录已暂停") : {
  needs: t("需要你补充"), advancing: t("推进中"), review: t("已完成，等你验收"), waiting: t("待继续"), resting: t("暂时没有进行中的任务"),
}[pet.mood];

/** A pushpin whose needle tip is at (x + 1, y + 4). */
const pinRects = (x: number, y: number, o = 1): Rect[] => [[x, y, 3, 2, PIN, o], [x + 1, y + 2, 1, 2, '#c9d1d6', o]];

interface Frame {
  time: number;
  event?: PetEvent;
  lifted: boolean;
  dropAge: number;
  landAge: number;
  motion: PetMotion;
  action?: Played;
  pinned: boolean;
  accent: string;
  /** The pet's terminal; `terminal` dresses the whole pet in it, otherwise only the badge. */
  look?: Look;
  terminal: boolean;
  paused: boolean;
  seed: number;
}

/** Pose plus the props around the pet (box, card, sparkles, pin), for one frame. */
function scene(pet: PetView, f: Frame): { pose: Pose; extra: Rect[] } {
  const { time, event } = f;
  const extra: Rect[] = [];
  const at = (fps: number) => Math.floor(time * fps + f.seed);
  const grey = f.paused || pet.mood === 'resting';
  // Dressed pets keep their colors at rest (they fade instead): grey would hide which is which.
  const dressed = f.terminal ? f.look : undefined;
  const accent = dressed?.color ?? f.accent;
  const color = dressed ? dressed.color : grey ? GREY : accent;
  const airborne = f.motion.state === 'fall' || f.motion.state === 'glide';
  const walking = f.motion.state === 'walk';
  let pose: Pose = { color, body: dressed?.body, mark: f.look?.mark, dot: color, dotOn: true };
  if (f.lifted) return { pose: { ...pose, legs: at(12) % 2 ? 'a' : 'b', armL: 'up', armR: 'up', face: 'up' }, extra };
  if (airborne) return { pose: { ...pose, legs: 'tuck', armL: 'up', armR: 'up', face: 'up', dot: pet.mood === 'needs' ? AMBER : color }, extra: f.pinned ? pinRects(X - 8, FEET - 19) : [] };
  const blink = (time + f.seed) % 4.2 < 0.12;
  let boxPulse = false;
  switch (grey ? 'resting' : pet.mood) {
    case 'advancing': {
      const phase = at(8) % 4;
      pose = { ...pose, legs: WALK[phase], bob: phase % 2 === 0 ? 1 : 0, armR: 'fwd', face: blink ? 'blink' : 'eyes', look: 1, dotOn: at(3) % 3 !== 0 };
      // Treadmill dashes under the feet sell the walk.
      if (!walking) for (let k = 0; k < 3; k++) extra.push([X - 9 + ((k * 6 - at(8)) % 18 + 18) % 18, FEET + 1, 2, 1, color, 0.35]);
      break;
    }
    case 'needs':
      pose = { ...pose, face: 'q', glyph: AMBER, armL: 'up', armR: at(4) % 2 ? 'up' : 'out', dot: AMBER, dotOn: at(2.5) % 2 === 0 };
      break;
    case 'review':
      pose = { ...pose, face: 'ok', glyph: GREEN, dot: GREEN, armR: at(1) % 2 ? 'out' : 'down' };
      break;
    default: {
      if (walking) { pose = { ...pose, face: blink ? 'blink' : 'eyes' }; break; }
      // Waiting and resting: dozing, with a "z" drifting up.
      pose = { ...pose, face: 'sleep', sink: true, dotOn: false };
      const k = ((time + f.seed) % 2.4) / 2.4;
      extra.push(...glyph(Z_GLYPH, X + 8 + Math.round(k * 2), FEET - 20 - Math.round(k * 6), color, 1 - k));
    }
  }
  if (walking) {
    // Wandering along the taskbar: legs going, eyes on the way ahead.
    const phase = at(8) % 4;
    pose = { ...pose, legs: WALK[phase], bob: phase % 2 === 0 ? 1 : 0, look: f.motion.dir, sink: false, armL: pose.armL === 'up' ? 'up' : 'down', armR: pose.armR === 'up' ? 'up' : 'down' };
  }
  if (event && !walking) {
    const age = time - event.at;
    const kind: PetEventKind = event.kind;
    if (kind === 'register') {
      if (age < 0.6) pose = { ...pose, carry: true, sink: false, legs: WALK[at(8) % 4], armL: 'up', armR: 'up', face: 'happy', glyph: undefined };
      else if (age < 0.95) {
        const k = (age - 0.6) / 0.35;
        pose = { ...pose, sink: false, armL: 'up', armR: 'up', face: 'happy', glyph: undefined, legs: 'stand' };
        const y = FEET - 21 - Math.round(k * 16);
        extra.push([X - 8, y, 16, 6, '#eef3f4', 1 - k * 0.4], [X - 6, y + 2, 9, 1, '#6d7a82', 1 - k * 0.4]);
      } else if (at(10) % 2 === 0) {
        for (const [dx, dy] of [[-9, -2], [9, -2], [-6, 2], [6, 2]]) extra.push([X + dx, FEET - 37 + dy, 1, 1, accent]);
      }
    } else if (kind === 'step') {
      boxPulse = true;
      if (age < 0.15) pose = { ...pose, bob: 1 };
    } else if (kind === 'needs') {
      // Jumps up waving to be noticed.
      pose = { ...pose, sink: false, bob: age < 0.5 ? Math.round(3 * Math.sin((Math.PI * age) / 0.5)) : 0, armL: 'up', armR: at(8) % 2 ? 'up' : 'out' };
    } else if (kind === 'done') {
      pose = { ...pose, face: 'ok', glyph: GREEN, dot: GREEN, sink: false, bob: age < 0.25 ? 2 : 0, armL: age < 0.5 ? 'up' : 'down', armR: age < 0.5 ? 'up' : 'down' };
    } else {
      const air = age < 0.55;
      const bob = air ? Math.round(6 * Math.sin((Math.PI * age) / 0.55)) : 0;
      pose = { ...pose, bob, sink: false, legs: air ? 'tuck' : 'stand', armL: air ? 'up' : age < 0.8 ? 'out' : 'down', armR: air ? 'up' : age < 0.8 ? 'out' : 'down', face: air ? 'ok' : 'happy', glyph: air ? GREEN : undefined, dot: GREEN };
      if (age > 0.25 && age < 1.3 && at(8) % 3 !== 2) extra.push(...glyph(OK_GLYPH, X + 8, FEET - 26, GREEN));
    }
  }
  // Just landed on the taskbar: a squash.
  if (f.landAge < 0.35) pose = { ...pose, sink: true, bob: 0, legs: 'stand', armL: 'out', armR: 'out', face: 'blink', glyph: undefined };
  if (f.dropAge < 0.5) pose = { ...pose, face: 'happy', glyph: undefined, armL: 'out', armR: 'out', legs: 'stand', bob: 0 };
  let pin: Rect[] = f.pinned ? pinRects(X - 8, FEET - 19) : [];
  const age = f.action ? time - f.action.at : Infinity;
  if (f.action && age < ACTION_SECONDS) {
    const k = Math.min(age / 0.6, 1);
    switch (f.action.kind) {
      case 'split': case 'merge':
        // A little pop as the pets come apart or back together.
        pose = { ...pose, sink: false, bob: age < 0.5 ? Math.round(3 * Math.sin((Math.PI * age) / 0.5)) : 0, armL: 'out', armR: 'out', face: 'happy', glyph: undefined };
        break;
      case 'pin':
        pin = pinRects(X - 8, FEET - 19 - Math.round((1 - k) * 10), 0.4 + 0.6 * k);
        pose = { ...pose, face: k < 1 ? 'up' : 'happy', glyph: undefined, bob: k === 1 && age < 0.75 ? -1 : pose.bob };
        break;
      case 'unpin':
        pin = pinRects(X - 8, FEET - 19 - Math.round(k * 10), 1 - k);
        pose = { ...pose, face: 'happy', glyph: undefined, armL: 'out', armR: 'out' };
        break;
      case 'top': case 'untop': {
        const up = f.action.kind === 'top';
        extra.push(...glyph(up ? UP_ARROW : DOWN_ARROW, X + 8, FEET - 26 + (up ? -1 : 1) * Math.round(k * 5), up ? accent : GREY, 1 - k * 0.8));
        pose = { ...pose, face: up ? 'up' : 'blink', glyph: undefined };
        break;
      }
      case 'gravity': case 'float':
        pose = { ...pose, face: 'up', glyph: undefined, armL: 'up', armR: 'up' };
        break;
      case 'nope':
        // Pinned: shakes its head instead of moving.
        pose = { ...pose, face: 'eyes', glyph: undefined, look: at(14) % 2 ? -1 : 1, legs: 'stand', bob: 0 };
        if (age < 0.6) pin = pin.map(([x, y, w, h, c, o]) => [x + (at(14) % 2 ? -1 : 0), y, w, h, c, o] as Rect);
        break;
    }
  }
  // The pin sits on the head, so it rides the bob.
  const shift = -(pose.bob ?? 0) + (pose.sink ? 1 : 0);
  extra.push(...pin.map(([x, y, w, h, c, o]) => [x, y + shift, w, h, c, o] as Rect));
  if (pet.mood === 'advancing' && !grey && !walking) extra.push(...box(pet.steps, time, boxPulse, accent));
  return { pose, extra };
}

/** The task "box" an advancing Agent pushes, showing its step count. */
function box(steps: string, time: number, pulse: boolean, color: string): Rect[] {
  const gx = X + 8;
  const h = pulse ? 9 : 8;
  const top = FEET - h;
  const edge = pulse ? '#ffffff' : color;
  const rects: Rect[] = [
    [gx, top, 13, h, '#1a2126'],
    [gx, top, 13, 1, edge, 0.8], [gx, FEET - 1, 13, 1, edge, 0.8], [gx, top, 1, h, edge, 0.8], [gx + 12, top, 1, h, edge, 0.8],
  ];
  const text = steps && digitsWidth(steps) <= 11 ? steps : '';
  if (text) rects.push(...digits(text, gx + 1 + Math.floor((11 - digitsWidth(text)) / 2), top + 2, pulse ? '#ffffff' : '#e8ebef'));
  else for (let i = 0; i < 3; i++) rects.push([gx + 3 + i * 3, FEET - 5, 1, 1, '#e8ebef', Math.floor(time * 4) % 3 === i ? 1 : 0.35]);
  return rects;
}

const seedOf = (key: string) => [...key].reduce((sum, ch) => sum + ch.charCodeAt(0), 0) % 7 * 0.37;

// Docked under a board at the top of the screen, the window's top runs off screen:
// the bubble moves down to stay visible.
const screenTop = () => (screen as Screen & { availTop?: number }).availTop ?? 0;

/** Stand-in for a split window whose Agent has nothing going on right now. */
const idlePet = (key: string): PetView => ({ key, name: key === 'rest' || key === 'board' ? '' : key, mood: 'resting', taskId: null, title: '', steps: '' });

export function PetApp() {
  const [hello, setHello] = useState<{ key: string } | null>(null);
  const [options, setOptions] = useState<PetOptions>({ split: false, pinned: false, on_top: true, gravity: false });
  const [motion, setMotion] = useState<PetMotion>({ state: 'still', dir: 1 });
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [preferences, setPreferences] = useState<Preferences>(defaults);
  const [paused, setPaused] = useState(false);
  const [events, setEvents] = useState<PetEvent[]>([]);
  const [action, setAction] = useState<Played | undefined>();
  const [lifted, setLifted] = useState(false);
  const [droppedAt, setDroppedAt] = useState(-10);
  const [landedAt, setLandedAt] = useState(-10);
  const [hover, setHover] = useState<string | null>(null);
  const marks = useRef<Map<number, TaskMark> | null>(null);
  const revision = useRef(-1);
  const pressed = useRef<{ x: number; y: number; taskId: number | null } | null>(null);
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;
  const keyRef = useRef<string | null>(null);
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const motionRef = useRef(motion);
  const time = useClock();
  setLanguage(resolveLanguage(preferences.language));

  useEffect(() => {
    let alive = true;
    const move = (next: PetMotion) => {
      if (motionRef.current.state === 'fall' && next.state !== 'fall') setLandedAt(clock());
      motionRef.current = next;
      setMotion(next);
    };
    const refresh = async () => {
      try {
        const next = await readRevision();
        if (next === revision.current) return;
        const board = await readSnapshot();
        if (!alive) return;
        revision.current = next;
        const nextMarks = marksOf(board);
        const now = clock();
        const found = diffEvents(marks.current, nextMarks, now);
        marks.current = nextMarks;
        if (found.length) {
          setEvents(previous => [...previous.filter(event => now - event.at < EVENT_SECONDS[event.kind]), ...found]);
          // Gravity mode: an Agent that needs you, or wants a review, comes back to the board.
          const key = keyRef.current;
          const mine = (event: PetEvent) => !key || key === 'board' || event.agent === key;
          if (optionsRef.current.gravity && found.some(event => (event.kind === 'needs' || event.kind === 'done') && mine(event))) void petAttention().catch(() => {});
        }
        setSnapshot(board);
      } catch { /* the board shows errors; the pet just keeps its last state */ }
    };
    const readPaused = () => void readTrackingPaused().then(value => { if (alive) setPaused(value); }, () => {});
    const subscriptions = [
      onPreferencesChanged(setPreferences),
      onPetDropped(() => { setLifted(false); setDroppedAt(clock()); }),
      onPetOptions(setOptions),
      onPetMotion(move),
      onPetAction(kind => setAction({ kind, at: clock() })),
    ];
    // Listen first, then ask: a window that starts falling right away still shows it.
    void Promise.all(subscriptions).then(() => petHello()).then(value => {
      if (!alive) return;
      keyRef.current = value.key;
      setOptions(value.options);
      move(value.motion);
      setHello({ key: value.key });
    }, () => { if (alive) { keyRef.current = ''; setHello({ key: '' }); } });
    void readPreferences().then(value => { if (alive) setPreferences(value); }, () => {});
    void refresh();
    readPaused();
    const timers = [window.setInterval(() => void refresh(), 1500), window.setInterval(readPaused, 5000)];
    // Browser preview only: ?pet&demo&petevent=accepted replays one reaction every few seconds.
    const replay = !native && new URLSearchParams(location.search).get('petevent');
    if (replay) timers.push(window.setInterval(() => {
      const kind = replay as PetEventKind | Action;
      if (kind in EVENT_SECONDS) {
        const first = derivePets(snapshotRef.current ?? { revision: 0, projects: [] }, 'per_agent', 30, Date.now())[0];
        setEvents([{ kind: kind as PetEventKind, agent: first.key, taskId: first.taskId ?? 0, at: clock() }]);
      } else setAction({ kind: kind as Action, at: clock() });
    }, 2600));
    return () => { alive = false; timers.forEach(id => window.clearInterval(id)); subscriptions.forEach(p => void p.then(unlisten => unlisten())); };
  }, []);

  const board = snapshot ?? { revision: 0, projects: [] };
  const live = events.filter(event => time - event.at < EVENT_SECONDS[event.kind] && time >= event.at);
  const all = derivePets(board, preferences.pet_mode, preferences.activity_minutes, Date.now(), new Set(live.map(event => event.agent)));
  const key = hello?.key ?? '';
  const pets = key ? [all.find(pet => pet.key === key) ?? idlePet(key)] : all;
  const width = pets.length * SLOT + 16;
  const keys = all.map(pet => pet.key).join('\n');
  // Split mode: the first window (or the merged one, when splitting) keeps the set of
  // Agent windows in step with the board. Rust ignores the others.
  useEffect(() => {
    if (hello && snapshot && options.split) void petLayout(keys.split('\n')).catch(() => {});
  }, [hello, snapshot !== null, options.split, keys]);
  useEffect(() => { if (hello && !key) void petResize(width).catch(() => {}); }, [hello, key, width]);
  // Rust keeps a new window hidden until its pets are drawn at the right width.
  const shown = useRef(false);
  useEffect(() => {
    if (!hello || !snapshot || shown.current) return;
    shown.current = true;
    void (key ? Promise.resolve() : petResize(width)).catch(() => {}).then(() => petReady()).catch(() => {});
  });
  const accent = accentOnDark(preferences.accent);
  const terminal = preferences.pet_look === 'terminal';
  const looks = looksFor(all.map(pet => pet.name));
  const lookOf = (name: string) => name ? looks.get(name) ?? looksFor([name]).get(name) : undefined;

  const play = (kind: Action) => setAction({ kind, at: clock() });
  const onMove = (event: ReactMouseEvent) => {
    if (lifted && event.buttons === 0) setLifted(false);
    const press = pressed.current;
    if (!press || Math.hypot(event.screenX - press.x, event.screenY - press.y) < 4) return;
    pressed.current = null;
    if (options.pinned) { play('nope'); return; }
    setLifted(true);
    void petDrag().then(started => { if (!started) { setLifted(false); play('nope'); } }, () => setLifted(false));
  };
  const hovered = pets.findIndex(pet => pet.key === hover);
  const hoveredPet = hovered >= 0 ? pets[hovered] : null;
  if (!hello) return null;
  const bubbleWidth = Math.min(220, width - 8);
  const bubbleLeft = Math.min(Math.max(8 + hovered * SLOT + X * U - bubbleWidth / 2, 4), width - 4 - bubbleWidth);

  return <main className="pet-stage" style={{ width }} onMouseMove={onMove} onMouseUp={() => { pressed.current = null; }}
    onMouseLeave={() => { setHover(null); pressed.current = null; }}
    onContextMenu={event => { event.preventDefault(); pressed.current = null; void petMenu().catch(() => {}); }}>
    {hoveredPet && !lifted && motion.state !== 'fall' && <div className="pet-bubble" style={{ top: 4 + Math.min(Math.max(screenTop() - window.screenY, 0), 56), left: bubbleLeft, maxWidth: bubbleWidth }}>
      <strong>{moodText(hoveredPet, paused)}</strong>
      {hoveredPet.title && <span>{hoveredPet.title}</span>}
    </div>}
    {pets.map((pet, index) => {
      const event = [...live].reverse().find(item => key === 'board' || preferences.pet_mode === 'single' || item.agent === pet.key);
      const look = lookOf(pet.name);
      const { pose, extra } = scene(pet, { time, event, lifted, dropAge: time - droppedAt, landAge: time - landedAt, motion, action, pinned: options.pinned, accent, look, terminal, paused, seed: seedOf(pet.key) });
      const faded = terminal && look && (paused || pet.mood === 'resting');
      const label = pet.name === '教学示例' ? t(pet.name) : pet.name;
      return <div key={pet.key} className="pet-slot" style={{ left: 8 + index * SLOT }}
        onMouseEnter={() => setHover(pet.key)}
        onMouseDown={event => { if (event.button === 0) pressed.current = { x: event.screenX, y: event.screenY, taskId: pet.taskId }; }}
        onMouseUp={event => { if (event.button === 0 && pressed.current) { const taskId = pressed.current.taskId; pressed.current = null; void petOpen(taskId).catch(() => {}); } }}>
        {label && <span className="pet-name" style={{ left: SLOT / 2 - 4 }}>{label}</span>}
        <svg width={SLOT} height={HEIGHT} viewBox={`0 0 ${COLS} ${ROWS}`} shapeRendering="crispEdges" aria-hidden="true">
          <g transform={`translate(${X} ${FEET})`} opacity={faded ? 0.6 : undefined}>{agentRects(pose).map(([x, y, w, h, c, o], i) => <rect key={i} x={x} y={y} width={w} height={h} fill={c} opacity={o} />)}</g>
          {extra.map(([x, y, w, h, c, o], i) => <rect key={`e${i}`} x={x} y={y} width={w} height={h} fill={c} opacity={o} />)}
        </svg>
      </div>;
    })}
  </main>;
}

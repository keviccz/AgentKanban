// What each desktop pet shows, derived only from the board snapshot: no extra data from
// Agents is needed. Pure functions so the rules stay easy to test.
import type { Snapshot, Task } from '../types';

/** Most urgent first. */
export type PetMood = 'needs' | 'advancing' | 'review' | 'waiting' | 'resting';
const RANK: Record<PetMood, number> = { needs: 0, advancing: 1, review: 2, waiting: 3, resting: 4 };

export interface PetView {
  /** Agent name in per-agent mode, "board" in single mode, "rest" when nothing is going on. */
  key: string;
  name: string;
  mood: PetMood;
  taskId: number | null;
  title: string;
  steps: string;
}

export type PetEventKind = 'register' | 'step' | 'needs' | 'done' | 'accepted';
export interface PetEvent { kind: PetEventKind; agent: string; taskId: number; at: number }

/** The parts of a task whose changes the pets react to. */
export interface TaskMark { agent: string; status: Task['status']; review: Task['review_status']; stepsDone: number; needs: boolean }

const agentOf = (task: Task) => task.agent?.trim() || '';
const stepsDone = (task: Task) => task.steps.filter(step => step.status === 'done').length;

/** Agent work only: personal todos and archived rows never move a pet. */
const agentTasks = (snapshot: Snapshot) => snapshot.projects.flatMap(project => project.tasks.filter(task => !task.personal && !task.archived));

const needsYou = (task: Task) => task.status === 'blocked' || (task.status !== 'done' && task.needs_input !== '');

export function moodOf(task: Task, activityMinutes: number, now: number): PetMood | null {
  if (needsYou(task)) return 'needs';
  if (task.status === 'in_progress') {
    const reported = task.agent_updated_at ? Date.parse(task.agent_updated_at) : NaN;
    return now - reported < activityMinutes * 60_000 ? 'advancing' : 'waiting';
  }
  if (task.status === 'done' && task.review_status === 'pending') return 'review';
  return null;
}

const recency = (task: Task) => Date.parse(task.agent_updated_at ?? task.updated_at) || 0;

function best(tasks: Task[], activityMinutes: number, now: number) {
  let pick: { task: Task; mood: PetMood } | null = null;
  for (const task of tasks) {
    const mood = moodOf(task, activityMinutes, now);
    if (!mood) continue;
    if (!pick || RANK[mood] < RANK[pick.mood] || (RANK[mood] === RANK[pick.mood] && recency(task) > recency(pick.task))) pick = { task, mood };
  }
  return pick;
}

const view = (key: string, name: string, pick: { task: Task; mood: PetMood } | null): PetView => ({
  key,
  name,
  mood: pick?.mood ?? 'resting',
  taskId: pick?.task.id ?? null,
  title: pick?.task.title ?? '',
  steps: pick && pick.task.steps.length ? `${stepsDone(pick.task)}/${pick.task.steps.length}` : '',
});

export const MAX_PETS = 3;

/**
 * Pets to show. Per-agent mode: one per Agent client with something going on, most
 * urgent first. `keep` holds agents whose celebration is still playing, so a pet does
 * not vanish the moment its last task is accepted.
 */
export function derivePets(snapshot: Snapshot, mode: 'per_agent' | 'single', activityMinutes: number, now: number, keep: Set<string> = new Set()): PetView[] {
  const tasks = agentTasks(snapshot);
  if (mode === 'single') {
    const pick = best(tasks, activityMinutes, now);
    return [view('board', pick ? agentOf(pick.task) : '', pick)];
  }
  const groups = new Map<string, Task[]>();
  for (const task of tasks) {
    const agent = agentOf(task);
    if (!agent) continue;
    groups.set(agent, [...(groups.get(agent) ?? []), task]);
  }
  const pets: { pet: PetView; recency: number }[] = [];
  for (const [agent, list] of groups) {
    const pick = best(list, activityMinutes, now);
    if (!pick && !keep.has(agent)) continue;
    pets.push({ pet: view(agent, agent, pick), recency: pick ? recency(pick.task) : Math.max(...list.map(recency)) });
  }
  pets.sort((a, b) => RANK[a.pet.mood] - RANK[b.pet.mood] || b.recency - a.recency || a.pet.key.localeCompare(b.pet.key));
  const shown = pets.slice(0, MAX_PETS).map(entry => entry.pet);
  return shown.length ? shown : [view('rest', '', null)];
}

export function marksOf(snapshot: Snapshot): Map<number, TaskMark> {
  return new Map(agentTasks(snapshot).map(task => [task.id, { agent: agentOf(task), status: task.status, review: task.review_status, stepsDone: stepsDone(task), needs: needsYou(task) }]));
}

/** Changes worth a little show. The first snapshot after start produces none. */
export function diffEvents(previous: Map<number, TaskMark> | null, next: Map<number, TaskMark>, at: number): PetEvent[] {
  if (!previous) return [];
  const events: PetEvent[] = [];
  for (const [taskId, mark] of next) {
    const before = previous.get(taskId);
    if (!mark.agent) continue;
    if (!before) {
      if (mark.status !== 'done') events.push({ kind: 'register', agent: mark.agent, taskId, at });
      continue;
    }
    if (before.review !== 'accepted' && mark.review === 'accepted') events.push({ kind: 'accepted', agent: mark.agent, taskId, at });
    else if (before.status !== 'done' && mark.status === 'done') events.push({ kind: 'done', agent: mark.agent, taskId, at });
    else if (mark.needs && !before.needs) events.push({ kind: 'needs', agent: mark.agent, taskId, at });
    else if (mark.stepsDone > before.stepsDone) events.push({ kind: 'step', agent: mark.agent, taskId, at });
  }
  return events;
}

/** How long each reaction plays, in seconds. */
export const EVENT_SECONDS: Record<PetEventKind, number> = { register: 1.6, step: 0.7, needs: 1.2, done: 1, accepted: 1.5 };

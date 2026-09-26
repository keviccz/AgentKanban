import type { Filter, Project, Task } from './types';
import { t } from './i18n';

export function relativeTime(timestamp: string, now: number) {
  const minutes = Math.max(0, Math.floor((now - Date.parse(timestamp)) / 60000));
  if (!Number.isFinite(minutes)) return t("时间未知");
  if (minutes < 1) return t("刚刚");
  if (minutes < 60) return t("{0} 分钟前", minutes);
  if (minutes < 1440) return t("{0} 小时前", Math.floor(minutes / 60));
  return t("{0} 天前", Math.floor(minutes / 1440));
}

export function isStale(task: Task, hours: number, now: number) {
  return hours > 0 && (task.status === 'in_progress' || task.status === 'blocked')
    && now - Date.parse(task.agent_updated_at ?? task.updated_at) >= hours * 3_600_000;
}

// The board has no live connection to an Agent; a recent report on an in-progress task is the best signal.
export const isAdvancing = (task: Task, minutes: number, now: number) => task.status === 'in_progress'
  && task.agent_updated_at !== null && now - Date.parse(task.agent_updated_at) < minutes * 60_000;

export const awaitsReview = (task: Task) => task.status === 'done' && task.review_status === 'pending';
// Done means finished: an unreviewed delivery turns grey with the rest, and reviewing it is optional.
export const inActiveList = (task: Task) => task.status !== 'done';
// Everything the user has to act on: a blocker or a question.
export const needsAttention = (task: Task) => task.status === 'blocked' || (task.status !== 'done' && task.needs_input !== '');
export const matchesFilter = (task: Task, filter: Filter) => filter === 'all' || filter === 'recent' || (filter === 'attention' ? needsAttention(task) : task.status === filter);
// Filters saved by v0.3 and earlier: blocked/review now live under attention, todo under all.
export const normalizeFilter = (value: string): Filter => value === 'blocked' || value === 'review' || value === 'attention' ? 'attention' : value === 'in_progress' || value === 'recent' ? value : 'all';
export const matchesSearch = (project: Project, task: Task, query: string) => !query || [task.title, project.name, project.path, task.task_key, task.goal, task.request, task.progress, task.user_note].some(value => value.toLowerCase().includes(query));
export const changedAt = (task: Task) => Date.parse(task.updated_at) || 0;
export const stepProgress = (task: Task) => task.steps.length ? `${task.steps.filter(step => step.status === 'done').length}/${task.steps.length}` : '';
export const reviewLabels = { none: '', pending: '未验收', accepted: '已验收', changes_requested: '需修改' };

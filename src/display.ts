import type { Filter, Task } from './types';

export function relativeTime(timestamp: string, now: number) {
  const minutes = Math.max(0, Math.floor((now - Date.parse(timestamp)) / 60000));
  if (!Number.isFinite(minutes)) return '时间未知';
  if (minutes < 1) return '刚刚';
  if (minutes < 60) return `${minutes} 分钟前`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时前`;
  return `${Math.floor(minutes / 1440)} 天前`;
}

export function isStale(task: Task, hours: number, now: number) {
  return hours > 0 && (task.status === 'in_progress' || task.status === 'blocked')
    && now - Date.parse(task.agent_updated_at ?? task.updated_at) >= hours * 3_600_000;
}

export const awaitsReview = (task: Task) => task.status === 'done' && task.review_status === 'pending';
export const inActiveList = (task: Task) => task.status !== 'done' || awaitsReview(task);
// Everything the user has to act on: a blocker, a question, or a delivery to review.
export const needsAttention = (task: Task) => task.status === 'blocked' || awaitsReview(task) || (task.status !== 'done' && task.needs_input !== '');
export const matchesFilter = (task: Task, filter: Filter) => filter === 'all' || (filter === 'attention' ? needsAttention(task) : task.status === filter);
// Filters saved by v0.3 and earlier: blocked/review now live under attention, todo under all.
export const normalizeFilter = (value: string): Filter => value === 'blocked' || value === 'review' || value === 'attention' ? 'attention' : value === 'in_progress' ? 'in_progress' : 'all';
export const stepProgress = (task: Task) => task.steps.length ? `${task.steps.filter(step => step.status === 'done').length}/${task.steps.length}` : '';
export const reviewLabels = { none: '', pending: '待你验收', accepted: '已验收', changes_requested: '需修改' };

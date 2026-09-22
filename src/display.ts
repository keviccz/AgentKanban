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
export const matchesFilter = (task: Task, filter: Filter) => filter === 'all' || (filter === 'review' ? awaitsReview(task) : task.status === filter);
export const reviewLabels = { none: '', pending: '待你验收', accepted: '已验收', changes_requested: '需修改' };

export type Status = 'todo' | 'in_progress' | 'blocked' | 'done';
export type Filter = 'all' | Exclude<Status, 'done'>;
export interface Task {
  id: number; project_id: number; task_key: string; title: string; status: Status;
  progress: string; branch: string | null; updated_at: string; archived: boolean;
}
export interface Project { id: number; name: string; path: string; tasks: Task[] }
export interface Snapshot { revision: number; projects: Project[] }
export interface Preferences {
  theme: 'light' | 'dark'; always_on_top: boolean; compact: boolean; filter: Filter;
  collapsed_projects: number[]; expanded_projects: number[]; completed_projects: number[];
}
export const defaults: Preferences = {
  theme: 'light', always_on_top: true, compact: false, filter: 'all',
  collapsed_projects: [], expanded_projects: [], completed_projects: [],
};
export const labels: Record<Status, string> = { todo: '待办', in_progress: '进行中', blocked: '受阻', done: '已完成' };

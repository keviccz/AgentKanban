import type { ActivityStyle } from './Activity';
export type Status = 'todo' | 'in_progress' | 'blocked' | 'done';
export type Filter = 'all' | 'attention' | 'in_progress' | 'recent';
export type ReviewStatus = 'none' | 'pending' | 'accepted' | 'changes_requested';
export interface Deliverable { label: string; uri: string }
export interface Step { title: string; status: Status; note?: string }
export interface Task {
  id: number; project_id: number; task_key: string; title: string; status: Status;
  progress: string; branch: string | null; updated_at: string; archived: boolean;
  request: string; agent: string | null; next_action: string; needs_input: string;
  deliverables: Deliverable[]; review_status: ReviewStatus; user_note: string;
  agent_updated_at: string | null;
  steps: Step[]; review_withdrawn_at: string | null;
  goal: string; acceptance: string[];
}
/** One task_upsert as the Agent sent it (routing fields removed). */
export interface TaskReport { reported_at: string; payload: Record<string, unknown> }
export interface TaskReceipt { id: number; status: Status; updated_at: string }
export interface CaptureInput { project_path: string; task_key: string; title: string; request: string }
export interface ArchivedTask extends Task { project_name: string; project_path: string }
export interface ArchiveQuery { query?: string; project_id?: number; limit?: number; offset?: number }
export interface ArchivePage { items: ArchivedTask[]; next_offset: number | null }
export interface BlockedProject { id: number; name: string; path: string }
export interface Project { id: number; name: string; path: string; tasks: Task[]; archived_count: number }
export const isTutorialTask = (project: Pick<Project, 'name'>, task: Pick<Task, 'agent' | 'task_key'>) => project.name === '新手教程'
  && task.agent === '教学示例'
  && (task.task_key === 'tutorial:follow-progress' || task.task_key === 'tutorial:review-delivery');
export const isTutorialProject = (project: Project) => project.tasks.length > 0 && project.tasks.every(task => isTutorialTask(project, task));
export interface Snapshot { revision: number; projects: Project[] }
export interface Preferences {
  theme: 'light' | 'dark' | 'system'; always_on_top: boolean; compact: boolean; concise: boolean; filter: Filter;
  collapsed_projects: number[]; expanded_projects: number[]; completed_projects: number[];
  focused_project: number | null; pinned_projects: number[];
  stale_after_hours: number; shortcut_enabled: boolean;
  notify: boolean; auto_archive_days: number; start_hidden: boolean;
  font_scale: number; opacity: number;
  auto_check_updates: boolean; auto_download_updates: boolean;
  project_sort: 'recent' | 'name';
  language: 'auto' | 'zh' | 'en';
  activity_style: ActivityStyle; activity_minutes: number;
}
export const defaults: Preferences = {
  theme: 'light', always_on_top: true, compact: false, concise: false, filter: 'all',
  collapsed_projects: [], expanded_projects: [], completed_projects: [],
  focused_project: null, pinned_projects: [], stale_after_hours: 24, shortcut_enabled: true,
  notify: true, auto_archive_days: 7, start_hidden: true,
  font_scale: 100, opacity: 100,
  auto_check_updates: true, auto_download_updates: false,
  project_sort: 'recent',
  language: 'auto',
  activity_style: 'pulse', activity_minutes: 30,
};
export interface IntegrationInfo {
  app_version: string; mcp_path: string; mcp_exists: boolean; database_path: string;
  last_task_update: string | null;
}
export interface ClientStatus {
  id: string; name: string; detected: boolean;
  mcp: 'ok' | 'outdated' | 'missing' | 'unreadable'; rules: boolean | null;
  config_path: string; rules_path: string | null; manual: string;
}
export interface DesktopSettings {
  autostart_enabled: boolean; shortcut_enabled: boolean; shortcut: string;
  create_shortcut: string;
  autostart_error: string | null; shortcut_error: string | null;
}
export interface McpCheck { ok: boolean; message: string }
export interface SyncEvent {
  at: string; transport: 'mcp' | 'cli'; tool: string;
  outcome: 'ok' | 'paused' | 'error'; error: string | null;
}
export interface SyncHealth {
  paused: boolean; last_call: SyncEvent | null; last_success: SyncEvent | null; last_write: SyncEvent | null;
}
export interface UpdateStatus {
  phase: 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'installing' | 'blocked' | 'error';
  current_version: string; version: string | null; notes: string | null; checked_at: string | null;
  downloaded_bytes: number; total_bytes: number | null; message: string;
}
export const labels: Record<Status, string> = { todo: '待办', in_progress: '进行中', blocked: '受阻', done: '已完成' };

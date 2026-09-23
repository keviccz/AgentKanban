export type Status = 'todo' | 'in_progress' | 'blocked' | 'done';
export type Filter = 'all' | 'attention' | 'in_progress';
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
}
export interface TaskReceipt { id: number; status: Status; updated_at: string }
export interface CaptureInput { project_path: string; task_key: string; title: string; request: string }
export interface Project { id: number; name: string; path: string; tasks: Task[] }
export interface Snapshot { revision: number; projects: Project[] }
export interface Preferences {
  theme: 'light' | 'dark'; always_on_top: boolean; compact: boolean; filter: Filter;
  collapsed_projects: number[]; expanded_projects: number[]; completed_projects: number[];
  focused_project: number | null; pinned_projects: number[];
  stale_after_hours: number; shortcut_enabled: boolean;
}
export const defaults: Preferences = {
  theme: 'light', always_on_top: true, compact: false, filter: 'all',
  collapsed_projects: [], expanded_projects: [], completed_projects: [],
  focused_project: null, pinned_projects: [], stale_after_hours: 24, shortcut_enabled: true,
};
export interface IntegrationInfo {
  app_version: string; mcp_path: string; mcp_exists: boolean; database_path: string;
  last_task_update: string | null; configs: { codex: string; claude: string; cursor: string };
}
export interface DesktopSettings {
  autostart_enabled: boolean; shortcut_enabled: boolean; shortcut: string;
  create_shortcut: string;
  autostart_error: string | null; shortcut_error: string | null;
}
export interface McpCheck { ok: boolean; message: string }
export const labels: Record<Status, string> = { todo: '待办', in_progress: '进行中', blocked: '受阻', done: '已完成' };

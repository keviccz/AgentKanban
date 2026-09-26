import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { defaults, type CaptureInput, type ClientStatus, type TaskReport, type DesktopSettings, type IntegrationInfo, type McpCheck, type Preferences, type Snapshot, type TaskReceipt, type UpdateStatus, type SyncHealth, type ArchiveQuery, type ArchivePage, type BlockedProject } from './types';

export const native = isTauri();
// Browser mode is only a layout preview. It never creates sample tasks or pretends to save them.
export const readSnapshot = (): Promise<Snapshot> => native ? invoke('get_snapshot') : Promise.resolve({ revision: 0, projects: [] });
export const readRevision = (): Promise<number> => native ? invoke('get_revision') : Promise.resolve(0);
export const readPreferences = (): Promise<Preferences> => native ? invoke('get_preferences') : Promise.resolve(defaults);
export const savePreferences = (preferences: Preferences): Promise<Preferences> => native ? invoke('set_preferences', { preferences }) : Promise.resolve(preferences);
export const compactWindow = (compact: boolean): Promise<Preferences> => invoke('set_compact', { compact });
export const hideWindow = (): Promise<void> => invoke('hide_window');
export const onVisibility = (callback: (visible: boolean) => void) => native ? listen<boolean>('visibility-changed', e => callback(e.payload)) : Promise.resolve(() => {});
export const onError = (callback: (message: string) => void) => native ? listen<string>('app-error', e => callback(e.payload)) : Promise.resolve(() => {});
export const readIntegrationInfo = (): Promise<IntegrationInfo> => invoke('get_integration_info');
export const readDesktopSettings = (): Promise<DesktopSettings> => invoke('get_desktop_settings');
export const setAutostart = (enabled: boolean): Promise<DesktopSettings> => invoke('set_autostart', { enabled });
export const setShortcut = (enabled: boolean): Promise<DesktopSettings> => invoke('set_shortcut_enabled', { enabled });
export const checkMcp = (): Promise<McpCheck> => invoke('check_mcp');
export const readClients = (): Promise<ClientStatus[]> => invoke('get_clients');
export const setupClient = (id: string): Promise<ClientStatus> => invoke('setup_client', { id });
export const revealPath = (target: 'mcp' | 'database'): Promise<void> => invoke('reveal_path', { target });
export const backupDatabase = (): Promise<string> => invoke('backup_database');
export const previewAppearance = (fontScale: number, opacity: number): Promise<void> => native ? invoke('preview_appearance', { fontScale, opacity }) : Promise.resolve();
export const readTaskReports = (id: number): Promise<TaskReport[]> => invoke('get_task_reports', { id });
// The board may start in the tray at sign-in; polling waits until it is shown.
export const readWindowVisible = (): Promise<boolean> => native ? getCurrentWindow().isVisible() : Promise.resolve(true);
export const createTask = (input: CaptureInput): Promise<TaskReceipt> => invoke('create_task', { input });
export const reviewTask = (id: number, expected_updated_at: string, accepted: boolean, note: string): Promise<TaskReceipt> => invoke('review_task', { input: { id, expected_updated_at, accepted, note } });
export const sendFeedback = (id: number, expected_updated_at: string, note: string): Promise<TaskReceipt> => invoke('send_task_feedback', { input: { id, expected_updated_at, note } });
export const pickProjectFolder = (title: string, start?: string): Promise<string | null> => invoke('pick_project_folder', { title, start: start || null });
export const setProjectBlocked = (projectId: number, blocked: boolean): Promise<void> => invoke('set_project_blocked', { projectId, blocked });
export const readBlockedProjects = (): Promise<BlockedProject[]> => invoke('get_blocked_projects');
/**
 * Our own drag handle. Tauri's drag region toggles maximize on double-click, and the
 * board is not meant to maximize: afterwards the window could no longer be dragged.
 */
export function dragWindow(event: { button: number; target: EventTarget | null; preventDefault(): void }) {
  if (!native || event.button !== 0) return;
  if (event.target instanceof Element && event.target.closest('button, a, input, select, textarea, summary')) return;
  event.preventDefault();
  void getCurrentWindow().startDragging();
}
export const archiveProject = (projectId: number): Promise<number> => invoke('archive_project', { projectId });
export const archiveTask = (id: number, expected_updated_at: string): Promise<TaskReceipt> => invoke('archive_task', { input: { id, expected_updated_at } });
export const readTrackingPaused = (): Promise<boolean> => native ? invoke('get_tracking_paused') : Promise.resolve(false);
export const setTrackingPaused = (paused: boolean): Promise<boolean> => invoke('set_tracking_paused', { paused });
export const readHandoff = (id: number): Promise<string> => invoke('get_handoff', { id });
export const openExternalLink = (url: string): Promise<void> => invoke('open_external_link', { url });
export const onQuickCreate = (callback: (preferences: Preferences) => void) => native ? listen<Preferences>('quick-create', e => callback(e.payload)) : Promise.resolve(() => {});
export const readUpdateStatus = (): Promise<UpdateStatus> => invoke('get_update_status');
export const checkUpdates = (manual = true): Promise<UpdateStatus> => invoke('check_updates', { manual });
export const downloadUpdate = (): Promise<UpdateStatus> => invoke('download_update');
export const installUpdate = (): Promise<UpdateStatus> => invoke('install_update');
export const onUpdateStatus = (callback: (status: UpdateStatus) => void) => native ? listen<UpdateStatus>('update-status', e => callback(e.payload)) : Promise.resolve(() => {});
export const readSyncHealth = (): Promise<SyncHealth> => invoke('get_sync_health');
export const listArchivedTasks = (input: ArchiveQuery): Promise<ArchivePage> => invoke('list_archived_tasks', { input });
export const restoreArchivedTask = (id: number, expected_updated_at: string): Promise<TaskReceipt> => invoke('restore_archived_task', { input: { id, expected_updated_at } });

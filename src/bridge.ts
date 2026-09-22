import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { defaults, type DesktopSettings, type IntegrationInfo, type McpCheck, type Preferences, type Snapshot } from './types';

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

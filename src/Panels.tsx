import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { backupDatabase, checkMcp, native, readClients, readDesktopSettings, readIntegrationInfo, revealPath, setAutostart, setShortcut, setupClient } from './bridge';
import { type ClientStatus, type DesktopSettings, type IntegrationInfo, type McpCheck, type Preferences } from './types';
import codexRule from '../examples/codex-AGENTS-snippet.md?raw';
import harmonyLicense from './fonts/HarmonyOS-Sans-LICENSE.txt?raw';
import { Updates } from './Updates';
import { SyncHealth } from './SyncHealth';
import { ArchiveCenter } from './ArchiveCenter';

export function Panel({ title, children, onClose, initialFocus, busy = false }: { title: string; children: ReactNode; onClose: () => void; initialFocus?: RefObject<HTMLInputElement | null>; busy?: boolean }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const node = dialog.current;
    node?.showModal();
    initialFocus?.current?.focus();
    return () => node?.close();
  }, [initialFocus]);
  function close(pointer = false) {
    if (busy) return;
    dialog.current?.close();
    // Closing restores focus to the opener; after a mouse click that would leave a stray focus ring.
    if (pointer && document.activeElement instanceof HTMLElement) document.activeElement.blur();
    onClose();
  }
  return <dialog ref={dialog} className="panel" aria-label={title} onCancel={event => { event.preventDefault(); close(); }}>
    <div className="panel-heading"><h2>{title}</h2><button className="text-button" autoFocus={!initialFocus} disabled={busy} title="返回看板（Esc）" onClick={event => close(event.detail > 0)}>返回</button></div>
    {children}
  </dialog>;
}

export function CopyButton({ text, label = '复制', copied = '已复制', title, onFailure }: { text: string; label?: string; copied?: string; title?: string; onFailure?: () => void }) {
  const [message, setMessage] = useState('');
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setMessage(copied);
    } catch { setMessage('复制失败，请选择文字复制'); onFailure?.(); }
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setMessage(''), 2400);
  }
  return <span className="copy-control"><button type="button" className="text-button" title={title} onClick={() => void copy()}>{label}</button><span className="copy-result" role="status">{message}</span></span>;
}

const connected = (client: ClientStatus) => client.mcp === 'ok' && client.rules !== false;
const clientState = (client: ClientStatus) => connected(client) ? '已配置'
  : client.mcp === 'outdated' ? '配置待更新'
  : client.mcp === 'unreadable' ? '配置无法解析'
  : client.mcp === 'ok' ? '规则待更新'
  : client.detected ? '未接入' : '未检测到';

type Outcome = { ok: boolean; text: string } | null;

/** App owns the save queue, so leaving this panel never discards an appearance edit. */
function RangeSetting({ label, value, min, max, presets, unit = '%', disabled, commit }: {
  label: string; value: number; min: number; max: number; presets: [string, number][]; unit?: string;
  disabled: boolean; commit: (value: number) => void;
}) {
  return <div className="range-setting">
    <div className="setting-row"><span>{label}</span><span className="preset-group" role="group" aria-label={`${label}预设`}>{presets.map(([name, preset]) => <button key={name} type="button" disabled={disabled} aria-pressed={value === preset} onClick={() => commit(preset)}>{name}</button>)}</span></div>
    <div className="range-row"><input type="range" aria-label={label} min={min} max={max} step={5} disabled={disabled} value={value} onChange={event => commit(Number(event.target.value))} /><span className="range-value">{value}{unit}</span></div>
  </div>;
}
const PRIMARY_CLIENTS = ['codex', 'claude', 'dsh'];

export function Settings({ preferences, busy, disabled, saveError, update, onShortcutChanged, onClose }: { preferences: Preferences; busy: boolean; disabled: boolean; saveError: string; update: (patch: Partial<Preferences>) => void; onShortcutChanged: () => void; onClose: () => void }) {
  const [tab, setTab] = useState<'desktop' | 'integration' | 'updates'>('desktop');
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [archiveBusy, setArchiveBusy] = useState(false);
  const archiveEntry = useRef<HTMLButtonElement>(null);
  const [desktop, setDesktop] = useState<DesktopSettings | null>(null);
  const [info, setInfo] = useState<IntegrationInfo | null>(null);
  const [clients, setClients] = useState<ClientStatus[]>([]);
  const [manualId, setManualId] = useState('codex');
  const [allClients, setAllClients] = useState(false);
  const [settingUp, setSettingUp] = useState('');
  const [setupResult, setSetupResult] = useState<Outcome>(null);
  const [backup, setBackup] = useState<Outcome>(null);
  const [backingUp, setBackingUp] = useState(false);
  const backupInFlight = useRef(false);
  const [working, setWorking] = useState(false);
  const [checking, setChecking] = useState(false);
  const [check, setCheck] = useState<McpCheck | null>(null);
  const [error, setError] = useState('');

  async function reload() {
    if (!native) return;
    setWorking(true); setError('');
    const results = await Promise.allSettled([readDesktopSettings(), readIntegrationInfo(), readClients()]);
    const [system, connection, found] = results;
    if (system.status === 'fulfilled') setDesktop(system.value);
    if (connection.status === 'fulfilled') setInfo(connection.value);
    if (found.status === 'fulfilled') setClients(found.value);
    setError(results.filter(result => result.status === 'rejected').map(result => String(result.reason)).join('；'));
    setWorking(false);
  }
  useEffect(() => { void reload(); }, []);

  async function toggle(kind: 'autostart' | 'shortcut', enabled: boolean) {
    setWorking(true); setError('');
    try {
      setDesktop(await (kind === 'autostart' ? setAutostart(enabled) : setShortcut(enabled)));
      if (kind === 'shortcut') onShortcutChanged();
    } catch (e) { setError(String(e)); }
    finally { setWorking(false); }
  }
  async function runCheck() {
    setChecking(true); setCheck(null);
    try { setCheck(await checkMcp()); }
    catch (e) { setCheck({ ok: false, message: String(e) }); }
    finally { setChecking(false); }
  }
  async function connect(client: ClientStatus) {
    setSettingUp(client.id); setSetupResult(null);
    try {
      const next = await setupClient(client.id);
      setClients(list => list.map(item => item.id === next.id ? next : item));
      setSetupResult({ ok: true, text: `${next.name} 已配置，重启该客户端后再验证工具是否可用。` });
    } catch (e) { setSetupResult({ ok: false, text: String(e) }); setManualId(client.id); }
    finally { setSettingUp(''); }
  }
  async function runBackup() {
    if (backupInFlight.current) return;
    backupInFlight.current = true; setBackingUp(true); setBackup(null);
    try { setBackup({ ok: true, text: `已备份到 ${await backupDatabase()}` }); }
    catch (e) { setBackup({ ok: false, text: String(e) }); }
    finally { backupInFlight.current = false; setBackingUp(false); }
  }
  const reveal = (target: 'mcp' | 'database') => void revealPath(target).catch(e => setError(String(e)));
  const manual = clients.find(client => client.id === manualId);
  const primary = PRIMARY_CLIENTS.flatMap(id => clients.filter(client => client.id === id));
  const others = clients.filter(client => !PRIMARY_CLIENTS.includes(client.id));

  if (archiveOpen) return <Panel title="归档中心" onClose={onClose} busy={archiveBusy}><ArchiveCenter onBusyChange={setArchiveBusy} onBack={() => { setArchiveOpen(false); requestAnimationFrame(() => archiveEntry.current?.focus()); }} /></Panel>;

  return <Panel title="设置" onClose={onClose} busy={backingUp}>
    <nav className="panel-tabs" aria-label="设置分类"><button aria-pressed={tab === 'desktop'} onClick={() => setTab('desktop')}>桌面</button><button aria-pressed={tab === 'integration'} onClick={() => setTab('integration')}>Agent 接入</button><button aria-pressed={tab === 'updates'} onClick={() => setTab('updates')}>软件更新</button></nav>
    <div className="panel-body">
      {!native && <p className="hint">浏览器布局预览。系统设置与 MCP 诊断请在桌面版中使用。</p>}
      {error && <p className="panel-error" role="alert">{error}</p>}
      {saveError && <p className="panel-error" role="alert">{saveError}</p>}
      {tab === 'desktop' ? <>
        <section className="settings-section"><h3>外观</h3>
          <RangeSetting label="字号" value={preferences.font_scale} min={80} max={130} presets={[['小', 85], ['中', 100], ['大', 115]]} disabled={disabled} commit={font_scale => update({ font_scale })} />
          <RangeSetting label="不透明度" value={preferences.opacity} min={50} max={100} presets={[['不透明', 100], ['轻透', 90], ['半透', 75]]} disabled={disabled} commit={opacity => update({ opacity })} />
        </section>
        <section className="settings-section"><h3>随时查看</h3>
          <label className="setting-row"><span>登录 Windows 时启动</span><input type="checkbox" checked={desktop?.autostart_enabled ?? false} disabled={!desktop || working || Boolean(desktop.autostart_error)} onChange={event => void toggle('autostart', event.target.checked)} /></label>
          {desktop?.autostart_enabled && <label className="setting-row sub-setting"><span>启动时只在托盘，不弹出窗口</span><input type="checkbox" checked={preferences.start_hidden} disabled={disabled} onChange={event => update({ start_hidden: event.target.checked })} /></label>}
          {desktop?.autostart_error && <p className="panel-error">{desktop.autostart_error}</p>}
          <label className="setting-row"><span>全局快捷键<small>Ctrl + Alt + K　显示 / 隐藏<br />Ctrl + Alt + N　快速新建</small></span><input type="checkbox" checked={desktop?.shortcut_enabled ?? false} disabled={!desktop || working || busy} onChange={event => void toggle('shortcut', event.target.checked)} /></label>
          {desktop?.shortcut_error && <p className="panel-error">{desktop.shortcut_error}</p>}
        </section>
        <section className="settings-section"><h3>提醒</h3>
          <label className="setting-row"><span>需要我处理时通知<small>受阻或需要你补充</small></span><input type="checkbox" checked={preferences.notify} disabled={disabled} onChange={event => update({ notify: event.target.checked })} /></label>
          <label className="setting-row"><span>多久未更新时提示</span><select aria-label="久未更新阈值" value={preferences.stale_after_hours} disabled={disabled} onChange={event => update({ stale_after_hours: Number(event.target.value) })}>{[0, 1, 4, 8, 24, 48, 168].map(hours => <option key={hours} value={hours}>{hours ? hours === 168 ? '7 天' : `${hours} 小时` : '关闭'}</option>)}</select></label>
        </section>
        <section className="settings-section"><h3>整理</h3>
          <label className="setting-row"><span>已完成任务自动归档</span><select aria-label="自动归档" value={preferences.auto_archive_days} disabled={disabled} onChange={event => update({ auto_archive_days: Number(event.target.value) })}>{[0, 1, 3, 7, 30].map(days => <option key={days} value={days}>{days ? `${days} 天后` : '关闭'}</option>)}</select></label>
          <div className="setting-row"><span>归档任务</span><button ref={archiveEntry} className="text-button" disabled={!native || backingUp} onClick={() => setArchiveOpen(true)}>归档中心</button></div>
          <div className="setting-row"><span>数据</span><span className="row-actions"><button className="text-button" disabled={!native} onClick={() => reveal('database')}>打开目录</button><button className="text-button" disabled={!native || backingUp} onClick={() => void runBackup()}>{backingUp ? '正在备份…' : '立即备份'}</button></span></div>
          {backup && <p role="status" className={backup.ok ? 'connection-ok' : 'panel-error'}>{backup.text}</p>}
        </section>
      </> : tab === 'updates' ? <Updates preferences={preferences} disabled={disabled} currentVersion={info?.app_version ?? null} update={update} onLater={onClose} /> : <>
        <SyncHealth />
        <section className="settings-section"><div className="section-heading"><h3>本地连接</h3><button className="text-button" disabled={!native || working} onClick={() => void reload()}>刷新诊断</button></div>
          {info ? <><p className={info.mcp_exists ? 'connection-ok' : 'panel-error'}>{info.mcp_exists ? '已找到 MCP 程序' : '未找到 MCP 程序，请检查安装目录'}</p>
            <dl className="diagnostics"><dt>最近一次任务变更</dt><dd>{info.last_task_update ? new Date(info.last_task_update).toLocaleString('zh-CN') : '尚无任务记录'}</dd><dt>MCP 程序 <button className="text-button" onClick={() => reveal('mcp')}>打开位置</button></dt><dd>{info.mcp_path}</dd><dt>数据库 <button className="text-button" onClick={() => reveal('database')}>打开位置</button></dt><dd>{info.database_path}</dd></dl>
            <button className="outline-button" disabled={!info.mcp_exists || checking} onClick={() => void runCheck()}>{checking ? '正在检查…' : '检查本地 MCP'}</button>
            {check && <p role="status" className={check.ok ? 'connection-ok' : 'panel-error'}>{check.message}</p>}
          </> : <p className="hint">{native ? working ? '正在读取…' : '诊断信息不可用，请重试。' : '需要桌面版读取实际路径。'}</p>}
        </section>
        <section className="settings-section"><h3>接入客户端</h3>
          <p className="hint">一键写入 MCP 配置和自动记录规则，原文件先备份。</p>
          <ul className="client-list">{(allClients ? [...primary, ...others] : primary).map(client => <li key={client.id} className={client.detected ? '' : 'client-absent'}>
            <span className="client-name" title={client.config_path}>{client.name}</span>
            <span className={`client-state ${connected(client) ? 'ok' : ''}`}>{clientState(client)}</span>
            <button className="text-button" disabled={Boolean(settingUp)} onClick={() => void connect(client)}>{settingUp === client.id ? '正在接入…' : connected(client) ? '重新接入' : '一键接入'}</button>
          </li>)}</ul>
          {others.length > 0 && <button className="text-button" onClick={() => setAllClients(value => !value)}>{allClients ? '收起' : '查看更多'}</button>}
          {setupResult && <p role="status" className={setupResult.ok ? 'connection-ok' : 'panel-error'}>{setupResult.text}</p>}
          {manual && <details className="manual-setup"><summary>手动配置</summary>
            <label className="setting-row"><span>客户端</span><select aria-label="手动配置的客户端" value={manualId} onChange={event => setManualId(event.target.value)}>{clients.map(client => <option key={client.id} value={client.id}>{client.name}</option>)}</select></label>
            <p className="hint mono">{manual.config_path}</p>
            <pre className="config-code" tabIndex={0}>{manual.manual}</pre>
            <span className="row-actions"><CopyButton text={manual.manual} label="复制配置" />{manual.rules_path && <CopyButton text={codexRule} label="复制规则" />}</span>
            {manual.rules_path && <p className="hint mono">规则放入 {manual.rules_path}</p>}
          </details>}
        </section>
      </>}
      {/* The HarmonyOS Sans license asks for a visible notice and its full text in every copy. */}
      <div className="settings-footer">
        <p className="hint version">AgentKanban{info ? ` ${info.app_version}` : ''}</p>
        <details className="font-license"><summary>字体许可</summary><p className="hint">中文字体使用 HarmonyOS Sans。Manrope 与 Geist Mono 采用 SIL Open Font License。HarmonyOS Sans 按以下协议使用，字体文件未作任何修改。</p><pre className="guidance" tabIndex={0}>{harmonyLicense.trim()}</pre></details>
      </div>
    </div>
  </Panel>;
}

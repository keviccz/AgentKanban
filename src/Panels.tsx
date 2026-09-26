import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { backupDatabase, checkMcp, dragWindow, native, readBlockedProjects, setProjectBlocked, readClients, readDesktopSettings, readIntegrationInfo, revealPath, setAutostart, setShortcut, setupClient } from './bridge';
import { type BlockedProject, type ClientStatus, type DesktopSettings, type IntegrationInfo, type McpCheck, type Preferences } from './types';
import codexRule from '../examples/codex-AGENTS-snippet.md?raw';
import harmonyLicense from './fonts/HarmonyOS-Sans-LICENSE.txt?raw';
import { Updates } from './Updates';
import { SyncHealth } from './SyncHealth';
import { ArchiveCenter } from './ArchiveCenter';
import { ACTIVITY_STYLES, ActivityLabel, ActivityMark, activityLabels } from './Activity';
import { ACCENTS, accentLabels, swatch } from './accent';
import { Summary } from './Summary';
import { showCopied } from './copyFeedback';
import { demo } from './demo';
import { locale, t } from './i18n';
import { Icon } from './Icon';

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
    <div className="panel-heading" onMouseDown={dragWindow}><h2>{title}</h2><button className="text-button" autoFocus={!initialFocus} disabled={busy} title={t("返回看板（Esc）")} onClick={event => close(event.detail > 0)}>{t("返回")}</button></div>
    {children}
  </dialog>;
}

/** Projects the user excluded from the board; Agents working in them are not recorded. */
function BlockedProjects({ onChanged }: { onChanged: () => void }) {
  const [items, setItems] = useState<BlockedProject[] | null>(null);
  const [error, setError] = useState('');
  const [working, setWorking] = useState<number | null>(null);
  async function load() {
    if (!native) return;
    try { setItems(await readBlockedProjects()); setError(''); }
    catch (e) { setError(t("读取屏蔽项目失败：{0}", String(e))); }
  }
  useEffect(() => { void load(); }, []);
  async function unblock(project: BlockedProject) {
    setWorking(project.id); setError('');
    try { await setProjectBlocked(project.id, false); onChanged(); await load(); }
    catch (e) { setError(t("取消屏蔽失败：{0}", String(e))); }
    finally { setWorking(null); }
  }
  return <section className="settings-section"><h3>{t("屏蔽的项目")}</h3>
    <p className="hint">{t("在看板上右键项目可屏蔽。之后在该项目中工作的 Agent 不再记录到看板，已有任务保留，取消屏蔽后恢复显示。")}</p>
    {!native ? null : items === null ? <p className="hint">{t("正在读取…")}</p> : items.length === 0 ? <p className="hint">{t("暂无屏蔽的项目。")}</p> : <ul className="client-list blocked-list">{items.map(project => <li key={project.id}>
      <span className="client-name" title={project.path}>{project.name}<small>{project.path}</small></span>
      <button className="text-button" disabled={working !== null} onClick={() => void unblock(project)}>{working === project.id ? t("正在取消…") : t("取消屏蔽")}</button>
    </li>)}</ul>}
    {error && <p className="panel-error" role="alert">{error}</p>}
  </section>;
}

export function CopyButton({ text, label = t("复制"), copied = t("已复制"), title, onFailure }: { text: string; label?: string; copied?: string; title?: string; onFailure?: () => void }) {
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy(x: number, y: number) {
    try {
      await navigator.clipboard.writeText(text);
      showCopied(x, y, copied);
      setMessage({ ok: true, text: copied });
    } catch { setMessage({ ok: false, text: t("复制失败，请选择文字复制") }); onFailure?.(); }
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setMessage(null), 2400);
  }
  // Success shows as a bubble by the pointer; the status text stays for screen readers.
  return <span className="copy-control"><button type="button" className="text-button" title={title} onClick={event => void copy(event.clientX, event.clientY)}>{label}</button><span className={`copy-result ${message?.ok ? 'sr-only' : ''}`} role="status">{message?.text}</span></span>;
}

const connected = (client: ClientStatus) => client.mcp === 'ok' && client.rules !== false;
const clientState = (client: ClientStatus) => connected(client) ? t("已配置")
  : client.mcp === 'outdated' ? t("配置待更新")
  : client.mcp === 'unreadable' ? t("配置无法解析")
  : client.mcp === 'ok' ? t("规则待更新")
  : client.detected ? t("未接入") : t("未检测到");

type Outcome = { ok: boolean; text: string } | null;

/** App owns the save queue, so leaving this panel never discards an appearance edit. */
function RangeSetting({ label, value, min, max, presets, unit = '%', disabled, commit }: {
  label: string; value: number; min: number; max: number; presets: [string, number][]; unit?: string;
  disabled: boolean; commit: (value: number) => void;
}) {
  return <div className="range-setting">
    <div className="setting-row"><span>{label}</span><span className="preset-group" role="group" aria-label={t("{0}预设", label)}>{presets.map(([name, preset]) => <button key={name} type="button" disabled={disabled} aria-pressed={value === preset} onClick={() => commit(preset)}>{name}</button>)}</span></div>
    <div className="range-row"><input type="range" aria-label={label} min={min} max={max} step={5} disabled={disabled} value={value} onChange={event => commit(Number(event.target.value))} /><span className="range-value">{value}{unit}</span></div>
  </div>;
}
const PRIMARY_CLIENTS = ['codex', 'claude', 'dsh'];

export function Settings({ preferences, busy, disabled, saveError, update, onShortcutChanged, onBoardChanged, onClose }: { preferences: Preferences; busy: boolean; disabled: boolean; saveError: string; update: (patch: Partial<Preferences>) => void; onShortcutChanged: () => void; onBoardChanged: () => void; onClose: () => void }) {
  const [tab, setTab] = useState<'desktop' | 'integration' | 'updates'>('desktop');
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [summaryOpen, setSummaryOpen] = useState(false);
  const summaryEntry = useRef<HTMLButtonElement>(null);
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
    setError(results.filter(result => result.status === 'rejected').map(result => String(result.reason)).join(t("；")));
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
      setSetupResult({ ok: true, text: t("{0} 已配置，重启该客户端后再验证工具是否可用。", next.name) });
    } catch (e) { setSetupResult({ ok: false, text: String(e) }); setManualId(client.id); }
    finally { setSettingUp(''); }
  }
  async function runBackup() {
    if (backupInFlight.current) return;
    backupInFlight.current = true; setBackingUp(true); setBackup(null);
    try { setBackup({ ok: true, text: t("已备份到 {0}", await backupDatabase()) }); }
    catch (e) { setBackup({ ok: false, text: String(e) }); }
    finally { backupInFlight.current = false; setBackingUp(false); }
  }
  const reveal = (target: 'mcp' | 'database') => void revealPath(target).catch(e => setError(String(e)));
  const manual = clients.find(client => client.id === manualId);
  const primary = PRIMARY_CLIENTS.flatMap(id => clients.filter(client => client.id === id));
  const others = clients.filter(client => !PRIMARY_CLIENTS.includes(client.id));

  if (summaryOpen) return <Panel title={t("工作摘要")} onClose={onClose}><div className="archive-toolbar summary-back"><button className="icon-button" aria-label={t("返回设置")} title={t("返回设置")} onClick={() => { setSummaryOpen(false); requestAnimationFrame(() => summaryEntry.current?.focus()); }}><Icon name="back" /></button></div><Summary /></Panel>;
  if (archiveOpen) return <Panel title={t("归档中心")} onClose={onClose} busy={archiveBusy}><ArchiveCenter onBusyChange={setArchiveBusy} onBack={() => { setArchiveOpen(false); requestAnimationFrame(() => archiveEntry.current?.focus()); }} /></Panel>;

  return <Panel title={t("设置")} onClose={onClose} busy={backingUp}>
    <nav className="panel-tabs" aria-label={t("设置分类")}><button aria-pressed={tab === 'desktop'} onClick={() => setTab('desktop')}>{t("桌面")}</button><button aria-pressed={tab === 'integration'} onClick={() => setTab('integration')}>{t("Agent 接入")}</button><button aria-pressed={tab === 'updates'} onClick={() => setTab('updates')}>{t("软件更新")}</button></nav>
    <div className="panel-body">
      {!native && !demo && <p className="hint">{t("浏览器布局预览。系统设置与 MCP 诊断请在桌面版中使用。")}</p>}
      {error && <p className="panel-error" role="alert">{error}</p>}
      {saveError && <p className="panel-error" role="alert">{saveError}</p>}
      {tab === 'desktop' ? <>
        <section className="settings-section"><h3>{t("外观")}</h3>
          <div className="setting-row"><span>{t("主题色")}<small>{t(accentLabels[preferences.accent])}</small></span><span className="accent-swatches" role="radiogroup" aria-label={t("主题色")}>{ACCENTS.map(accent => <button key={accent} type="button" role="radio" aria-checked={preferences.accent === accent} aria-label={t(accentLabels[accent])} title={t(accentLabels[accent])} disabled={disabled} style={{ background: swatch(accent) }} onClick={() => update({ accent })} />)}</span></div>
          <label className="setting-row"><span>{t("语言")}<small>{locale() === 'en-US' ? '语言' : 'Language'}</small></span><select aria-label={t("语言")} value={preferences.language} disabled={disabled} onChange={event => update({ language: event.target.value === 'zh' || event.target.value === 'en' ? event.target.value : 'auto' })}><option value="auto">{t("跟随系统")}</option><option value="zh">中文</option><option value="en">English</option></select></label>
          <label className="setting-row"><span>{t("主题")}</span><select aria-label={t("主题")} value={preferences.theme} disabled={disabled} onChange={event => update({ theme: event.target.value === 'dark' || event.target.value === 'light' ? event.target.value : 'system' })}><option value="system">{t("跟随系统")}</option><option value="light">{t("浅色")}</option><option value="dark">{t("深色")}</option></select></label>
          <label className="setting-row"><span>{t("项目排序")}<small>{t("置顶项目始终在前")}</small></span><select aria-label={t("项目排序")} value={preferences.project_sort} disabled={disabled} onChange={event => update({ project_sort: event.target.value === 'name' ? 'name' : 'recent' })}><option value="recent">{t("最近有更新的在前")}</option><option value="name">{t("按名称首字母")}</option></select></label>
          <RangeSetting label={t("字号")} value={preferences.font_scale} min={80} max={130} presets={[[t("小"), 85], [t("中"), 100], [t("大"), 115]]} disabled={disabled} commit={font_scale => update({ font_scale })} />
          <RangeSetting label={t("不透明度")} value={preferences.opacity} min={50} max={100} presets={[[t("不透明"), 100], [t("轻透"), 90], [t("半透"), 75]]} disabled={disabled} commit={opacity => update({ opacity })} />
        </section>
        <section className="settings-section"><h3>{t("Agent 推进提示")}</h3>
          <label className="setting-row"><span>{t("动效")}<small className="activity-sample" aria-hidden="true"><span className="status in_progress advancing"><span className="status-dot" /><ActivityLabel /><ActivityMark /></span></small></span><select aria-label={t("推进动效")} value={preferences.activity_style} disabled={disabled} onChange={event => update({ activity_style: ACTIVITY_STYLES.find(style => style === event.target.value) ?? 'pulse' })}>{ACTIVITY_STYLES.map(style => <option key={style} value={style}>{t(activityLabels[style])}</option>)}</select></label>
          <label className="setting-row"><span>{t("判定为推进中")}<small>{t("进行中，且 Agent 在此时间内有上报")}</small></span><select aria-label={t("判定为推进中")} value={preferences.activity_minutes} disabled={disabled || preferences.activity_style === 'off'} onChange={event => update({ activity_minutes: Number(event.target.value) })}>{[10, 30, 60].map(minutes => <option key={minutes} value={minutes}>{t("{0} 分钟内", minutes)}</option>)}</select></label>
        </section>
        <section className="settings-section"><h3>{t("随时查看")}</h3>
          <label className="setting-row"><span>{t("登录 Windows 时启动")}</span><input type="checkbox" checked={desktop?.autostart_enabled ?? false} disabled={!desktop || working || Boolean(desktop.autostart_error)} onChange={event => void toggle('autostart', event.target.checked)} /></label>
          {desktop?.autostart_enabled && <label className="setting-row sub-setting"><span>{t("启动时只在托盘，不弹出窗口")}</span><input type="checkbox" checked={preferences.start_hidden} disabled={disabled} onChange={event => update({ start_hidden: event.target.checked })} /></label>}
          {desktop?.autostart_error && <p className="panel-error">{desktop.autostart_error}</p>}
          <label className="setting-row"><span>{t("全局快捷键")}<small>{t("Ctrl + Alt + K　显示 / 隐藏")}<br />{t("Ctrl + Alt + N　快速新建")}</small></span><input type="checkbox" checked={desktop?.shortcut_enabled ?? false} disabled={!desktop || working || busy} onChange={event => void toggle('shortcut', event.target.checked)} /></label>
          {desktop?.shortcut_error && <p className="panel-error">{desktop.shortcut_error}</p>}
          <div className="setting-row"><span>{t("看板快捷键")}<small>{t("↑↓ 选择　Enter 打开　A 验收　E 归档　/ 搜索")}</small></span></div>
        </section>
        <section className="settings-section"><h3>{t("提醒")}</h3>
          <label className="setting-row"><span>{t("需要我处理时通知")}<small>{t("受阻或需要你补充")}</small></span><input type="checkbox" checked={preferences.notify} disabled={disabled} onChange={event => update({ notify: event.target.checked })} /></label>
          <label className="setting-row"><span>{t("Agent 完成任务时通知")}<small>{t("默认关闭；完成的任务会直接变灰")}</small></span><input type="checkbox" checked={preferences.notify_done} disabled={disabled} onChange={event => update({ notify_done: event.target.checked })} /></label>
          <label className="setting-row"><span>{t("多久未更新时提示")}</span><select aria-label={t("久未更新阈值")} value={preferences.stale_after_hours} disabled={disabled} onChange={event => update({ stale_after_hours: Number(event.target.value) })}>{[0, 1, 4, 8, 24, 48, 168].map(hours => <option key={hours} value={hours}>{hours ? hours === 168 ? t("7 天") : t("{0} 小时", hours) : t("关闭")}</option>)}</select></label>
        </section>
        <section className="settings-section"><h3>{t("整理")}</h3>
          <label className="setting-row"><span>{t("已完成任务自动归档")}</span><select aria-label={t("自动归档")} value={preferences.auto_archive_days} disabled={disabled} onChange={event => update({ auto_archive_days: Number(event.target.value) })}>{[0, 1, 3, 7, 30].map(days => <option key={days} value={days}>{days ? t("{0} 天后", days) : t("关闭")}</option>)}</select></label>
          <div className="setting-row"><span>{t("工作摘要")}<small>{t("最近完成的任务，可复制为 Markdown")}</small></span><button ref={summaryEntry} className="text-button" disabled={!native} onClick={() => setSummaryOpen(true)}>{t("查看摘要")}</button></div>
          <div className="setting-row"><span>{t("归档任务")}</span><button ref={archiveEntry} className="text-button" disabled={!native || backingUp} onClick={() => setArchiveOpen(true)}>{t("归档中心")}</button></div>
          <div className="setting-row"><span>{t("数据")}</span><span className="row-actions"><button className="text-button" disabled={!native} onClick={() => reveal('database')}>{t("打开目录")}</button><button className="text-button" disabled={!native || backingUp} onClick={() => void runBackup()}>{backingUp ? t("正在备份…") : t("立即备份")}</button></span></div>
          {backup && <p role="status" className={backup.ok ? 'connection-ok' : 'panel-error'}>{backup.text}</p>}
        </section>
      </> : tab === 'updates' ? <Updates preferences={preferences} disabled={disabled} update={update} onLater={onClose} /> : <>
        <SyncHealth />
        <BlockedProjects onChanged={onBoardChanged} />
        <section className="settings-section"><div className="section-heading"><h3>{t("本地连接")}</h3><button className="text-button" disabled={!native || working} onClick={() => void reload()}>{t("刷新诊断")}</button></div>
          {info ? <><p className={info.mcp_exists ? 'connection-ok' : 'panel-error'}>{info.mcp_exists ? t("已找到 MCP 程序") : t("未找到 MCP 程序，请检查安装目录")}</p>
            <dl className="diagnostics"><dt>{t("最近一次任务变更")}</dt><dd>{info.last_task_update ? new Date(info.last_task_update).toLocaleString(locale()) : t("尚无任务记录")}</dd><dt>{t("MCP 程序")} <button className="text-button" onClick={() => reveal('mcp')}>{t("打开位置")}</button></dt><dd>{info.mcp_path}</dd><dt>{t("数据库")} <button className="text-button" onClick={() => reveal('database')}>{t("打开位置")}</button></dt><dd>{info.database_path}</dd></dl>
            <button className="outline-button" disabled={!info.mcp_exists || checking} onClick={() => void runCheck()}>{checking ? t("正在检查…") : t("检查本地 MCP")}</button>
            {check && <p role="status" className={check.ok ? 'connection-ok' : 'panel-error'}>{check.message}</p>}
          </> : <p className="hint">{native ? working ? t("正在读取…") : t("诊断信息不可用，请重试。") : t("需要桌面版读取实际路径。")}</p>}
        </section>
        <section className="settings-section"><h3>{t("接入客户端")}</h3>
          <p className="hint">{t("一键写入 MCP 配置和自动记录规则，原文件先备份。")}</p>
          <ul className="client-list">{(allClients ? [...primary, ...others] : primary).map(client => <li key={client.id} className={client.detected ? '' : 'client-absent'}>
            <span className="client-name" title={client.config_path}>{client.name}</span>
            <span className={`client-state ${connected(client) ? 'ok' : ''}`}>{clientState(client)}</span>
            <button className="text-button" disabled={Boolean(settingUp)} onClick={() => void connect(client)}>{settingUp === client.id ? t("正在接入…") : connected(client) ? t("重新接入") : t("一键接入")}</button>
          </li>)}</ul>
          {others.length > 0 && <button className="text-button" onClick={() => setAllClients(value => !value)}>{allClients ? t("收起") : t("查看更多")}</button>}
          {setupResult && <p role="status" className={setupResult.ok ? 'connection-ok' : 'panel-error'}>{setupResult.text}</p>}
          {manual && <details className="manual-setup"><summary>{t("手动配置")}</summary>
            <label className="setting-row"><span>{t("客户端")}</span><select aria-label={t("手动配置的客户端")} value={manualId} onChange={event => setManualId(event.target.value)}>{clients.map(client => <option key={client.id} value={client.id}>{client.name}</option>)}</select></label>
            <p className="hint mono">{manual.config_path}</p>
            <pre className="config-code" tabIndex={0}>{manual.manual}</pre>
            <span className="row-actions"><CopyButton text={manual.manual} label={t("复制配置")} />{manual.rules_path && <CopyButton text={codexRule} label={t("复制规则")} />}</span>
            {manual.rules_path && <p className="hint mono">{t("规则放入 {0}", manual.rules_path)}</p>}
          </details>}
        </section>
      </>}
      {/* The HarmonyOS Sans license asks for a visible notice and its full text in every copy. */}
      <div className="settings-footer">
        <p className="hint version">AgentKanban{info ? ` ${info.app_version}` : ''}</p>
        <details className="font-license"><summary>{t("字体许可")}</summary><p className="hint">{t("中文字体使用 HarmonyOS Sans。Manrope 与 Geist Mono 采用 SIL Open Font License。HarmonyOS Sans 按以下协议使用，字体文件未作任何修改。")}</p><pre className="guidance" tabIndex={0}>{harmonyLicense.trim()}</pre></details>
      </div>
    </div>
  </Panel>;
}

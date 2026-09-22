import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { checkMcp, native, readDesktopSettings, readIntegrationInfo, setAutostart, setShortcut } from './bridge';
import { type DesktopSettings, type IntegrationInfo, type McpCheck, type Preferences } from './types';
import guidance from '../docs/AGENT_RULES.md?raw';

export function Panel({ title, children, onClose, initialFocus, busy = false }: { title: string; children: ReactNode; onClose: () => void; initialFocus?: RefObject<HTMLInputElement | null>; busy?: boolean }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const node = dialog.current;
    node?.showModal();
    initialFocus?.current?.focus();
    return () => node?.close();
  }, [initialFocus]);
  function close() { if (!busy) { dialog.current?.close(); onClose(); } }
  return <dialog ref={dialog} className="panel" aria-label={title} onCancel={event => { event.preventDefault(); close(); }}>
    <div className="panel-heading"><h2>{title}</h2><button className="text-button" autoFocus={!initialFocus} disabled={busy} onClick={close}>返回看板</button></div>
    {children}
  </dialog>;
}

export function CopyButton({ text, label = '复制' }: { text: string; label?: string }) {
  const [message, setMessage] = useState('');
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setMessage('已复制');
    } catch { setMessage('复制失败，请选择文字复制'); }
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setMessage(''), 2400);
  }
  return <span className="copy-control"><button type="button" className="text-button" onClick={() => void copy()}>{label}</button><span className="copy-result" role="status">{message}</span></span>;
}

export function Settings({ preferences, busy, saveError, update, onShortcutChanged, onClose }: { preferences: Preferences; busy: boolean; saveError: string; update: (patch: Partial<Preferences>) => void; onShortcutChanged: () => void; onClose: () => void }) {
  const [tab, setTab] = useState<'desktop' | 'integration'>('desktop');
  const [desktop, setDesktop] = useState<DesktopSettings | null>(null);
  const [info, setInfo] = useState<IntegrationInfo | null>(null);
  const [client, setClient] = useState<keyof IntegrationInfo['configs']>('codex');
  const [working, setWorking] = useState(false);
  const [checking, setChecking] = useState(false);
  const [check, setCheck] = useState<McpCheck | null>(null);
  const [error, setError] = useState('');

  async function reload() {
    if (!native) return;
    setWorking(true); setError('');
    const results = await Promise.allSettled([readDesktopSettings(), readIntegrationInfo()]);
    const [system, connection] = results;
    if (system.status === 'fulfilled') setDesktop(system.value);
    if (connection.status === 'fulfilled') setInfo(connection.value);
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

  return <Panel title="设置与接入" onClose={onClose}>
    <nav className="panel-tabs" aria-label="设置分类"><button aria-pressed={tab === 'desktop'} onClick={() => setTab('desktop')}>桌面</button><button aria-pressed={tab === 'integration'} onClick={() => setTab('integration')}>Agent 接入</button></nav>
    <div className="panel-body">
      {!native && <p className="hint">浏览器布局预览。系统设置与 MCP 诊断请在桌面版中使用。</p>}
      {error && <p className="panel-error" role="alert">{error}</p>}
      {saveError && <p className="panel-error" role="alert">{saveError}</p>}
      {tab === 'desktop' ? <>
        <section className="settings-section"><h3>随时查看</h3>
          <label className="setting-row"><span>登录 Windows 时启动</span><input type="checkbox" checked={desktop?.autostart_enabled ?? false} disabled={!desktop || working || Boolean(desktop.autostart_error)} onChange={event => void toggle('autostart', event.target.checked)} /></label>
          <p className="hint">启动桌面浮窗。即使浮窗关闭，Agent 仍可通过 MCP 更新任务。</p>
          {desktop?.autostart_error && <p className="panel-error">{desktop.autostart_error}</p>}
          <label className="setting-row"><span>全局快捷键<small>Ctrl + Alt + K　显示 / 隐藏<br />Ctrl + Alt + N　快速新建</small></span><input type="checkbox" checked={desktop?.shortcut_enabled ?? false} disabled={!desktop || working || busy} onChange={event => void toggle('shortcut', event.target.checked)} /></label>
          <p className="hint">看板在托盘运行时也有效；退出程序后快捷键随之释放。</p>
          {desktop?.shortcut_error && <p className="panel-error">{desktop.shortcut_error}</p>}
        </section>
        <section className="settings-section"><h3>进展提醒</h3>
          <label className="setting-row"><span>多久未更新时提示</span><select aria-label="久未更新阈值" value={preferences.stale_after_hours} disabled={busy} onChange={event => update({ stale_after_hours: Number(event.target.value) })}>{[0, 1, 4, 8, 24, 48, 168].map(hours => <option key={hours} value={hours}>{hours ? hours === 168 ? '7 天' : `${hours} 小时` : '关闭'}</option>)}</select></label>
          <p className="hint">只提示进行中和受阻的任务。保留原状态，不自动推断完成，也不会弹出通知。</p>
        </section>
      </> : <>
        <section className="settings-section"><div className="section-heading"><h3>本地连接</h3><button className="text-button" disabled={!native || working} onClick={() => void reload()}>刷新诊断</button></div>
          {info ? <><p className={info.mcp_exists ? 'connection-ok' : 'panel-error'}>{info.mcp_exists ? '已找到 MCP 程序' : '未找到 MCP 程序，请检查安装目录'}</p>
            <dl className="diagnostics"><dt>最近一次任务变更</dt><dd>{info.last_task_update ? new Date(info.last_task_update).toLocaleString('zh-CN') : '尚无任务记录'}</dd><dt>MCP 程序 <CopyButton text={info.mcp_path} /></dt><dd>{info.mcp_path}</dd><dt>数据库 <CopyButton text={info.database_path} /></dt><dd>{info.database_path}</dd></dl>
            <button className="outline-button" disabled={!info.mcp_exists || checking} onClick={() => void runCheck()}>{checking ? '正在检查…' : '检查本地 MCP'}</button>
            {check && <p role="status" className={check.ok ? 'connection-ok' : 'panel-error'}>{check.message}</p>}
            <p className="hint">自检验证本地 MCP 协议，不代表客户端已连接。最近变更时间也不是 Agent 在线状态。</p>
          </> : <p className="hint">{native ? working ? '正在读取…' : '诊断信息不可用，请重试。' : '需要桌面版读取实际路径。'}</p>}
        </section>
        <section className="settings-section"><h3>接入客户端</h3>
          <label className="setting-row"><span>客户端</span><select aria-label="客户端" value={client} onChange={event => setClient(event.target.value as typeof client)}><option value="codex">Codex</option><option value="claude">Claude Code</option><option value="cursor">Cursor</option></select></label>
          <p className="hint">{client === 'codex' ? '合并到 ~/.codex/config.toml' : client === 'claude' ? '合并到项目根目录的 .mcp.json' : '合并到 ~/.cursor/mcp.json'}，保留已有配置，然后重载客户端并允许此 MCP。</p>
          {info && <><pre className="config-code" tabIndex={0}>{info.configs[client]}</pre><CopyButton text={info.configs[client]} label="复制接入配置" /></>}
          <p className="hint">连接后告诉 Agent：「查询当前项目的看板任务。」确认三个工具可用，再记录真实工作。</p>
        </section>
        <section className="settings-section"><div className="section-heading"><h3>让进展持续同步</h3><CopyButton text={guidance} label="复制同步规则" /></div><p className="hint">将规则交给 Agent，或放入项目的 Agent 指令。只有明确要求入板的任务才会记录。</p><details><summary>查看同步规则</summary><pre className="guidance">{guidance}</pre></details></section>
        {info && <p className="hint version">AgentKanban {info.app_version} · 本机保存</p>}
      </>}
    </div>
  </Panel>;
}

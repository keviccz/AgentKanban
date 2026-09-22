import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { compactWindow, hideWindow, native, onError, onVisibility, readPreferences, readRevision, readSnapshot, savePreferences } from './bridge';
import { defaults, labels, type Filter, type Preferences, type Project, type Snapshot, type Task } from './types';

type IconName = 'logo' | 'pin' | 'sun' | 'moon' | 'minus' | 'close' | 'chevron' | 'branch' | 'expand';
function Icon({ name, className = '' }: { name: IconName; className?: string }) {
  const paths: Record<Exclude<IconName, 'logo'>, React.ReactNode> = {
    pin: <g transform="rotate(35 12 12)"><path d="M9 3h6m-5 0v6l-3 4v2h10v-2l-3-4V3M12 15v6" /></g>,
    sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5" /></>,
    moon: <path d="M20.5 14A8.5 8.5 0 0 1 10 3.5 8.5 8.5 0 1 0 20.5 14Z" />,
    minus: <path d="M5 12h14" />,
    close: <path d="m6 6 12 12M18 6 6 18" />,
    chevron: <path d="m9 5 7 7-7 7" />,
    branch: <><circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="5" r="2" /><path d="M6 7v10m0-3c0-6 12 0 12-7" /></>,
    expand: <path d="m5 9 7 7 7-7" />,
  };
  return <svg className={`icon ${className}`} viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">{name === 'logo' ? <><rect x="3" y="3" width="7" height="18" rx="1.5" fill="currentColor" stroke="none" /><rect x="13" y="3" width="8" height="18" rx="1.5" fill="currentColor" stroke="none" opacity=".65" /></> : paths[name]}</svg>;
}

function relativeTime(timestamp: string, now: number) {
  const minutes = Math.max(0, Math.floor((now - Date.parse(timestamp)) / 60000));
  if (!Number.isFinite(minutes)) return '时间未知';
  if (minutes < 1) return '刚刚';
  if (minutes < 60) return `${minutes} 分钟前`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时前`;
  return `${Math.floor(minutes / 1440)} 天前`;
}

const TaskRow = memo(function TaskRow({ task, now }: { task: Task; now: number }) {
  return <li className={`task task-${task.status}`}>
    <div className="task-heading"><h3 title={task.title}>{task.title}</h3><span className={`status ${task.status}`}><span className="status-dot" />{labels[task.status]}</span></div>
    <p className="progress" title={task.progress}>{task.progress || '尚未补充进展'}</p>
    <div className="task-meta">{task.branch ? <span className="branch" title={task.branch}><Icon name="branch" /><span>{task.branch}</span></span> : <span />}<time dateTime={task.updated_at} title={`Agent 最后上报：${new Date(task.updated_at).toLocaleString('zh-CN')}`}>{relativeTime(task.updated_at, now)}</time></div>
  </li>;
});

function ProjectSection({ project, preferences, update, now }: { project: Project; preferences: Preferences; update: (p: Partial<Preferences>) => void; now: number }) {
  const active = project.tasks.filter(task => task.status !== 'done' && (preferences.filter === 'all' || task.status === preferences.filter));
  const done = project.tasks.filter(task => task.status === 'done');
  const collapsed = preferences.collapsed_projects.includes(project.id);
  const expanded = preferences.expanded_projects.includes(project.id);
  const completed = preferences.completed_projects.includes(project.id);
  const toggle = (key: 'collapsed_projects' | 'expanded_projects' | 'completed_projects') => update({ [key]: preferences[key].includes(project.id) ? preferences[key].filter(id => id !== project.id) : [...preferences[key], project.id] });
  if (preferences.filter !== 'all' && active.length === 0) return null;
  return <section className="project" aria-label={project.name}>
    <button className="project-heading" aria-expanded={!collapsed} title={project.path} onClick={() => toggle('collapsed_projects')}><Icon name="chevron" className={!collapsed ? 'rotated' : ''} /><h2>{project.name}</h2><span className="project-count">{active.length}</span></button>
    {!collapsed && <div className="project-content">
      <ul className="task-list">{(expanded ? active : active.slice(0, 3)).map(task => <TaskRow key={task.id} task={task} now={now} />)}</ul>
      {active.length > 3 && <button className="disclosure more" aria-expanded={expanded} onClick={() => toggle('expanded_projects')}>{expanded ? '收起为 3 项' : `展开其余 ${active.length - 3} 项`}<Icon name="chevron" className={expanded ? 'up' : ''} /></button>}
      {preferences.filter === 'all' && done.length > 0 && <div className="completed"><button className="disclosure completed-toggle" aria-expanded={completed} onClick={() => toggle('completed_projects')}><Icon name="chevron" className={completed ? 'rotated' : ''} />已完成 <span>{done.length}</span></button>{completed && <ul className="task-list">{done.map(task => <TaskRow key={task.id} task={task} now={now} />)}</ul>}</div>}
    </div>}
  </section>;
}

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot>({ revision: -1, projects: [] });
  const [preferences, setPreferences] = useState(defaults);
  const [visible, setVisible] = useState(true);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(Date.now());
  const revision = useRef(-1);
  const refreshBusy = useRef(false);
  const preferencesRef = useRef(preferences);
  preferencesRef.current = preferences;

  const refresh = useCallback(async (force = false) => {
    if (refreshBusy.current) return;
    refreshBusy.current = true;
    try {
      const nextRevision = force ? -1 : await readRevision();
      if (force || nextRevision !== revision.current) {
        const next = await readSnapshot();
        revision.current = next.revision;
        setSnapshot(next);
      }
      setError(previous => previous.startsWith('读取失败') || previous.startsWith('启动失败') ? '' : previous);
    } catch (e) { setError(`读取失败：${String(e)}`); }
    finally { refreshBusy.current = false; }
  }, []);

  useEffect(() => {
    let disposed = false;
    const subscriptions = [onVisibility(value => { setVisible(value); if (value) void refresh(true); }), onError(setError)];
    void Promise.all([readPreferences(), readSnapshot()]).then(([prefs, board]) => {
      if (disposed) return;
      setPreferences(prefs); setSnapshot(board); revision.current = board.revision; setReady(true);
    }).catch(e => { if (!disposed) { setError(`启动失败：${String(e)}`); setReady(true); } });
    return () => { disposed = true; subscriptions.forEach(p => void p.then(unlisten => unlisten())); };
  }, [refresh]);

  useEffect(() => {
    if (!ready || !visible) return;
    const timer = window.setInterval(() => { void refresh(); setNow(Date.now()); }, 1000);
    return () => window.clearInterval(timer);
  }, [ready, visible, refresh]);

  useEffect(() => { document.documentElement.dataset.theme = preferences.theme; }, [preferences.theme]);

  async function update(patch: Partial<Preferences>) {
    if (busy) return;
    setBusy(true);
    try { const next = await savePreferences({ ...preferencesRef.current, ...patch }); setPreferences(next); setError(''); }
    catch (e) { setError(`设置保存失败：${String(e)}`); }
    finally { setBusy(false); }
  }

  async function toggleCompact() {
    if (busy) return;
    setBusy(true);
    try { setPreferences(native ? await compactWindow(!preferences.compact) : { ...preferences, compact: !preferences.compact }); }
    catch (e) { setError(`窗口切换失败：${String(e)}`); }
    finally { setBusy(false); }
  }

  const allTasks = snapshot.projects.flatMap(project => project.tasks);
  const ongoing = allTasks.filter(task => task.status === 'in_progress').length;
  const blocked = allTasks.filter(task => task.status === 'blocked').length;
  const visibleProjects = snapshot.projects.filter(project => preferences.filter === 'all' || project.tasks.some(task => task.status === preferences.filter));
  const controls = <>
    {!preferences.compact && <><button className={`icon-button ${preferences.always_on_top ? 'is-pinned' : ''}`} title={preferences.always_on_top ? '取消置顶' : '窗口置顶'} aria-label={preferences.always_on_top ? '取消置顶' : '窗口置顶'} aria-pressed={preferences.always_on_top} disabled={busy} onClick={() => void update({ always_on_top: !preferences.always_on_top })}><Icon name="pin" /></button><button className="icon-button" title={preferences.theme === 'light' ? '切换深色' : '切换浅色'} aria-label={preferences.theme === 'light' ? '切换深色' : '切换浅色'} disabled={busy} onClick={() => void update({ theme: preferences.theme === 'light' ? 'dark' : 'light' })}><Icon name={preferences.theme === 'light' ? 'moon' : 'sun'} /></button></>}
    <button className="icon-button" aria-label={preferences.compact ? '展开看板' : '收成窄条'} title={preferences.compact ? '展开看板' : '收成窄条'} disabled={busy} onClick={() => void toggleCompact()}><Icon name={preferences.compact ? 'expand' : 'minus'} /></button>
    <button className="icon-button close-button" aria-label="隐藏到托盘" title={native ? '隐藏到托盘（从托盘恢复）' : '浏览器预览不能隐藏到托盘'} disabled={!native} onClick={() => void hideWindow().catch(e => setError(String(e)))}><Icon name="close" /></button>
  </>;

  return <main className={`app ${preferences.compact ? 'compact' : ''}`}>
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region><Icon name="logo" /><span data-tauri-drag-region>AgentKanban</span></div>
      {preferences.compact && <div className="strip-counts" data-tauri-drag-region><span className="in_progress">{ongoing} 进行中</span><span className="blocked">{blocked} 受阻</span></div>}
      <div className="window-actions">{controls}</div>
    </header>
    {!preferences.compact && <>
      <div className="summary" title="数量表示 Agent 最后上报的状态；意外退出不会自动完成任务。"><span className="in_progress">{ongoing} 进行中</span><span className="summary-separator">·</span><span className="blocked">{blocked} 受阻</span></div>
      <nav className="filters" aria-label="按状态筛选">{(['all', 'in_progress', 'blocked', 'todo'] as Filter[]).map(filter => <button key={filter} aria-pressed={preferences.filter === filter} className={preferences.filter === filter ? 'selected' : ''} onClick={() => void update({ filter })}>{filter === 'all' ? '全部' : labels[filter]}</button>)}</nav>
      {error && <div className="error" role="alert"><span title={error}>{error}</span><button onClick={() => void refresh(true)}>重试</button></div>}
      <div className="board" aria-label="项目任务" aria-busy={!ready}>
        {!ready ? <div className="empty"><p>正在读取看板…</p></div> : visibleProjects.length ? visibleProjects.map(project => <ProjectSection key={project.id} project={project} preferences={preferences} update={p => void update(p)} now={now} />) : <div className="empty"><Icon name="logo" /><h2>{snapshot.projects.length ? `没有${preferences.filter === 'all' ? '' : labels[preferences.filter]}任务` : '把正在推进的事，交给看板。'}</h2><p>{snapshot.projects.length ? '切换筛选，查看其他任务。' : '告诉 Agent：「把这个功能加入看板，后续同步进展。」'}</p>{!native && <p className="preview-note">浏览器布局预览 · 请启动桌面版连接本地看板</p>}</div>}
      </div>
      <footer><span>由 Agent 更新</span><span title={error || '任务状态取自 Agent 最后一次上报。'}>{error ? '同步异常' : native ? '本地保存' : '布局预览'}</span></footer>
    </>}
    {preferences.compact && error && <span className="compact-error" title={error} role="alert">!</span>}
  </main>;
}

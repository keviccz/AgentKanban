import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { compactWindow, hideWindow, native, onError, onQuickCreate, onVisibility, readPreferences, readRevision, readSnapshot, readTrackingPaused, readWindowVisible, savePreferences, setTrackingPaused } from './bridge';
import { defaults, isTutorialProject, isTutorialTask, labels, type CaptureInput, type Filter, type Preferences, type Project, type Snapshot, type Task, type TaskReceipt } from './types';
import { awaitsReview, changedAt, inActiveList, isStale, matchesFilter, matchesSearch, needsAttention, normalizeFilter, relativeTime, reviewLabels, stepProgress } from './display';
import { Settings } from './Panels';
import { CapturePanel, TaskDetails, type FeedbackDraft } from './Workflows';

const filterLabels: Record<Filter, string> = { all: '全部', attention: '等你处理', in_progress: '进行中', recent: '最近变更' };
const SLOGAN = 'Agent 推进，你来验收。';

type IconName = 'logo' | 'pin' | 'sun' | 'moon' | 'list' | 'search' | 'minus' | 'close' | 'chevron' | 'branch' | 'expand';
function Icon({ name, className = '' }: { name: IconName; className?: string }) {
  const paths: Record<Exclude<IconName, 'logo'>, React.ReactNode> = {
    pin: <g transform="rotate(35 12 12)"><path d="M9 3h6m-5 0v6l-3 4v2h10v-2l-3-4V3M12 15v6" /></g>,
    sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5" /></>,
    moon: <path d="M20.5 14A8.5 8.5 0 0 1 10 3.5 8.5 8.5 0 1 0 20.5 14Z" />,
    list: <><path d="M8 6h12M8 12h12M8 18h12" /><path d="M4 6h.01M4 12h.01M4 18h.01" strokeWidth="3" /></>,
    search: <><circle cx="10" cy="10" r="6" /><path d="m15 15 6 6" /></>,
    minus: <path d="M5 12h14" />,
    close: <path d="m6 6 12 12M18 6 6 18" />,
    chevron: <path d="m9 5 7 7-7 7" />,
    branch: <><circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="5" r="2" /><path d="M6 7v10m0-3c0-6 12 0 12-7" /></>,
    expand: <path d="m5 9 7 7 7-7" />,
  };
  return <svg className={`icon ${className}`} viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">{name === 'logo' ? <><rect x="3" y="3" width="7" height="18" rx="1.5" fill="currentColor" stroke="none" /><rect x="13" y="3" width="8" height="18" rx="1.5" fill="currentColor" stroke="none" opacity=".65" /></> : paths[name]}</svg>;
}

const TaskRow = memo(function TaskRow({ task, tutorial, concise, recent, now, staleHours, onOpen }: { task: Task; tutorial: boolean; concise: boolean; recent: boolean; now: number; staleHours: number; onOpen: (id: number) => void }) {
  const timestamp = recent ? task.updated_at : task.agent_updated_at ?? task.updated_at;
  const timeTitle = `${recent ? '最近变更' : !tutorial && task.agent_updated_at ? 'Agent 最后上报' : '记录时间'}：${new Date(timestamp).toLocaleString('zh-CN')}`;
  const pending = awaitsReview(task);
  const steps = concise ? '' : stepProgress(task);
  const statusLabel = concise && pending ? '待你验收' : concise && task.review_status === 'accepted' ? '已验收' : labels[task.status];
  return <li className={`task task-${task.status} ${pending ? 'task-review' : ''} ${concise ? 'task-concise' : ''} ${recent ? 'task-recent' : ''}`}>
    <button className="task-open" aria-label={`查看任务：${task.title}`} onClick={() => onOpen(task.id)}>
      <span className="task-heading"><span className="task-title" title={recent ? `${task.title}\n${timeTitle}` : task.title}>{task.title}</span><span className={`status ${task.status} ${concise && pending ? 'review-pending' : ''}`}><span className="status-dot" />{statusLabel}</span></span>
      {!concise && <>
      <span className="progress" title={task.progress}>{task.progress || '尚未补充进展'}</span>
      {(task.agent || task.review_status !== 'none' || task.needs_input || steps || task.review_withdrawn_at) && <span className="task-signals">{steps && <span className="step-count" title="计划步骤完成数">{steps} 步</span>}{task.review_withdrawn_at && <span className="review-badge withdrawn" title="Agent 在你验收前重新打开了任务">已撤回验收</span>}{task.review_status !== 'none' && <span className={`review-badge ${task.review_status}`}>{reviewLabels[task.review_status]}</span>}{task.needs_input && <span className="input-signal">需要你补充</span>}{task.agent && <span className="agent-name" title={tutorial ? '教学示例' : `最后上报：${task.agent}`}>{task.agent}</span>}</span>}
      <span className="task-meta">{task.branch ? <span className="branch" title={task.branch}><Icon name="branch" /><span>{task.branch}</span></span> : <span>{!tutorial && !task.agent_updated_at ? '等待 Agent 接手' : ''}</span>}<span className="update-time">{!tutorial && isStale(task, staleHours, now) && <span className="stale" title="已超过设置的时间未收到更新；任务状态保持不变。">较久未更新</span>}<time dateTime={timestamp} title={timeTitle}>{recent && '最近变更 '}{relativeTime(timestamp, now)}</time></span></span>
      </>}
    </button>
  </li>;
});

function ProjectSection({ project, preferences, searching, update, now, busy, onOpen }: { project: Project; preferences: Preferences; searching: boolean; update: (p: Partial<Preferences>) => void; now: number; busy: boolean; onOpen: (id: number) => void }) {
  const recent = preferences.filter === 'recent';
  const forceExpanded = searching || recent;
  const active = project.tasks.filter(inActiveList);
  const rows = forceExpanded ? project.tasks : active;
  const done = project.tasks.filter(task => task.status === 'done' && !awaitsReview(task));
  const collapsed = !forceExpanded && preferences.collapsed_projects.includes(project.id);
  const expanded = forceExpanded || preferences.expanded_projects.includes(project.id);
  const completed = preferences.completed_projects.includes(project.id);
  const pinned = preferences.pinned_projects.includes(project.id);
  const toggle = (key: 'collapsed_projects' | 'expanded_projects' | 'completed_projects' | 'pinned_projects') => update({ [key]: preferences[key].includes(project.id) ? preferences[key].filter(id => id !== project.id) : [...preferences[key], project.id] });
  const heading = <><Icon name="chevron" className={!collapsed ? 'rotated' : ''} /><h2>{project.name}</h2><span className="project-count">{rows.length}</span></>;
  return <section className="project" aria-label={project.name}>
    <div className="project-header">{forceExpanded ? <div className="project-heading project-heading-static" title={`${project.path}\n${recent ? '最近变更' : '搜索'}视图临时展开，原折叠设置保留`}>{heading}</div> : <button className="project-heading" disabled={busy} aria-expanded={!collapsed} title={project.path} onClick={() => toggle('collapsed_projects')}>{heading}</button>}<button className={`icon-button project-pin ${pinned ? 'is-pinned' : ''}`} aria-pressed={pinned} aria-label={`${pinned ? '取消置顶项目' : '置顶项目'}：${project.name}`} title={recent ? '最近变更视图按时间排序，原置顶设置保留' : pinned ? '取消项目置顶' : '将项目排在前面'} disabled={busy || recent} onClick={() => toggle('pinned_projects')}><Icon name="pin" /></button></div>
    {!collapsed && <div className="project-content">
      <ul className="task-list">{(preferences.concise || expanded ? rows : rows.slice(0, 3)).map(task => <TaskRow key={task.id} task={task} tutorial={isTutorialTask(project, task)} concise={preferences.concise} recent={recent} now={preferences.concise ? 0 : now} staleHours={preferences.stale_after_hours} onOpen={onOpen} />)}</ul>
      {!forceExpanded && !preferences.concise && active.length > 3 && <button className="disclosure more" disabled={busy} aria-expanded={expanded} onClick={() => toggle('expanded_projects')}>{expanded ? '收起为 3 项' : `展开其余 ${active.length - 3} 项`}<Icon name="chevron" className={expanded ? 'up' : ''} /></button>}
      {!forceExpanded && preferences.filter === 'all' && done.length > 0 && <div className="completed"><button className="disclosure completed-toggle" disabled={busy} aria-expanded={completed} onClick={() => toggle('completed_projects')}><Icon name="chevron" className={completed ? 'rotated' : ''} />已完成 <span>{done.length}</span></button>{completed && <ul className="task-list">{done.map(task => <TaskRow key={task.id} task={task} tutorial={isTutorialTask(project, task)} concise={preferences.concise} recent={false} now={preferences.concise ? 0 : now} staleHours={preferences.stale_after_hours} onOpen={onOpen} />)}</ul>}</div>}
    </div>}
  </section>;
}

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot>({ revision: -1, projects: [] });
  const [preferences, setPreferences] = useState(defaults);
  const [visible, setVisible] = useState(true);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [compactBusy, setCompactBusy] = useState(false);
  const [preferencesReady, setPreferencesReady] = useState(false);
  const [now, setNow] = useState(Date.now());
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [selectedTaskId, setSelectedTaskId] = useState<number | null>(null);
  const [captureOpen, setCaptureOpen] = useState(false);
  const [captureDraft, setCaptureDraft] = useState<CaptureInput | null>(null);
  const [feedbackDrafts, setFeedbackDrafts] = useState<Record<number, FeedbackDraft>>({});
  const [trackingPaused, setPausedState] = useState(false);
  const [pauseReady, setPauseReady] = useState(false);
  const [pauseError, setPauseError] = useState('');
  const [pauseBusy, setPauseBusy] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchText, setSearchText] = useState('');
  const searchInput = useRef<HTMLInputElement>(null);
  const searchButton = useRef<HTMLButtonElement>(null);
  const revision = useRef(-1);
  const refreshBusy = useRef(false);
  const preferencesLoaded = useRef(false);
  const preferencesGeneration = useRef(0);
  const confirmedPreferences = useRef(defaults);
  const queuedPreferences = useRef<Partial<Preferences>>({});
  const activePreferencePatch = useRef<Partial<Preferences>>({});
  const preferenceWriteBusy = useRef(false);
  const taskActionBusy = useRef(false);
  const busy = saving || compactBusy || !preferencesReady;
  const locked = compactBusy || !preferencesReady;
  const preferencesRef = useRef(preferences);
  preferencesRef.current = preferences;
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;

  const applyPreferences = useCallback((next: Preferences) => {
    preferencesGeneration.current += 1;
    next = { ...next, filter: normalizeFilter(next.filter) };
    confirmedPreferences.current = next;
    const displayed = { ...next, ...activePreferencePatch.current, ...queuedPreferences.current };
    preferencesRef.current = displayed; setPreferences(displayed);
    preferencesLoaded.current = true; setPreferencesReady(true);
  }, []);

  const reloadPreferences = useCallback(async () => {
    const generation = preferencesGeneration.current;
    const next = await readPreferences();
    if (generation === preferencesGeneration.current) applyPreferences(next);
  }, [applyPreferences]);

  const applySnapshot = useCallback((next: Snapshot) => {
    // A polling response may finish after a user write and its immediate refresh.
    if (next.revision >= revision.current) { revision.current = next.revision; setSnapshot(next); }
  }, []);

  const openCapture = useCallback(() => {
    if (taskActionBusy.current) return;
    const prefs = preferencesRef.current;
    const projects = snapshotRef.current.projects.filter(project => !isTutorialProject(project));
    const focused = projects.find(project => project.id === prefs.focused_project);
    setCaptureDraft(draft => draft ?? { project_path: focused?.path ?? (projects.length === 1 ? projects[0].path : ''), task_key: `capture:${crypto.randomUUID()}`, title: '', request: '' });
    setSelectedTaskId(null); setSettingsOpen(false); setCaptureOpen(true);
  }, []);

  const revealSearch = useCallback(() => {
    setSearchOpen(true);
    requestAnimationFrame(() => { searchInput.current?.focus(); searchInput.current?.select(); });
  }, []);
  function closeSearch() {
    setSearchText(''); setSearchOpen(false);
    requestAnimationFrame(() => searchButton.current?.focus());
  }

  const refresh = useCallback(async (force = false) => {
    if (refreshBusy.current) return;
    refreshBusy.current = true;
    try {
      if (!preferencesLoaded.current) {
        await reloadPreferences();
      }
      const nextRevision = force ? -1 : await readRevision();
      if (force || nextRevision !== revision.current) {
        const next = await readSnapshot();
        applySnapshot(next);
      }
      setError(previous => previous.startsWith('读取失败') || previous.startsWith('启动失败') || previous.startsWith('任务已保存，读取失败') ? '' : previous);
    } catch (e) { setError(`读取失败：${String(e)}`); }
    finally { refreshBusy.current = false; }
  }, [applySnapshot, reloadPreferences]);

  const refreshAfterWrite = useCallback(async () => {
    try { applySnapshot(await readSnapshot()); setError(''); }
    catch (e) { setError(`读取失败：${String(e)}`); }
  }, [applySnapshot]);

  async function onCreated(receipt: TaskReceipt) {
    try {
      const next = await readSnapshot(); applySnapshot(next);
      setSelectedTaskId(receipt.id); setError('');
    } catch (e) { setError(`任务已保存，读取失败：${String(e)}`); }
    finally { setCaptureOpen(false); setCaptureDraft(null); }
  }

  useEffect(() => {
    let disposed = false;
    const generation = preferencesGeneration.current;
    void readWindowVisible().then(setVisible, () => {});
    const subscriptions = [onVisibility(value => { setVisible(value); if (value) { setNow(Date.now()); void refresh(true); } }), onError(setError), onQuickCreate(prefs => {
      applyPreferences(prefs);
      openCapture();
    })];
    void Promise.allSettled([readPreferences(), readSnapshot()]).then(([prefs, board]) => {
      if (disposed) return;
      if (prefs.status === 'fulfilled' && generation === preferencesGeneration.current) applyPreferences(prefs.value);
      if (board.status === 'fulfilled') applySnapshot(board.value);
      const failures = [prefs, board].filter(result => result.status === 'rejected').map(result => String(result.reason));
      if (failures.length) setError(`启动失败：${failures.join('；')}`);
      setReady(true);
    });
    return () => { disposed = true; subscriptions.forEach(p => void p.then(unlisten => unlisten())); };
  }, [refresh, applySnapshot, applyPreferences, openCapture]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'n' && !preferencesRef.current.compact) {
        event.preventDefault(); openCapture();
      }
      if (event.ctrlKey && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'f' && !preferencesRef.current.compact && !settingsOpen && !captureOpen && selectedTaskId === null) {
        event.preventDefault(); revealSearch();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [openCapture, revealSearch, settingsOpen, captureOpen, selectedTaskId]);

  useEffect(() => {
    if (!ready || !visible) return;
    const timer = window.setInterval(() => { void refresh(); setNow(Date.now()); }, 1000);
    return () => window.clearInterval(timer);
  }, [ready, visible, refresh]);

  useEffect(() => { document.documentElement.dataset.theme = preferences.theme; }, [preferences.theme]);

  const reloadPause = useCallback(async () => {
    try { setPausedState(await readTrackingPaused()); setPauseReady(true); setPauseError(''); }
    catch (e) { setPauseError(`读取记录状态失败：${String(e)}`); }
  }, []);

  useEffect(() => { void reloadPause(); }, [reloadPause]);

  async function retryRead() {
    await Promise.all([refresh(true), reloadPause()]);
  }

  async function togglePause() {
    if (pauseBusy || !pauseReady) return;
    setPauseBusy(true);
    try { setPausedState(await setTrackingPaused(!trackingPaused)); setPauseError(''); }
    catch (e) { setPauseError(`切换记录状态失败：${String(e)}`); }
    finally { setPauseBusy(false); }
  }

  async function update(patch: Partial<Preferences>) {
    if (!preferencesLoaded.current) return;
    queuedPreferences.current = { ...queuedPreferences.current, ...patch };
    const displayed = { ...preferencesRef.current, ...patch };
    preferencesRef.current = displayed; setPreferences(displayed);
    if (preferenceWriteBusy.current) return;
    preferenceWriteBusy.current = true; setSaving(true);
    let failed = false;
    try {
      // Keep pending edits in App: closing Settings must not cancel a user's last input.
      while (Object.keys(queuedPreferences.current).length) {
        const pending = queuedPreferences.current;
        queuedPreferences.current = {};
        activePreferencePatch.current = pending;
        const generation = preferencesGeneration.current;
        try {
          const next = await savePreferences({ ...confirmedPreferences.current, ...pending });
          activePreferencePatch.current = {};
          if (generation === preferencesGeneration.current) applyPreferences(next);
          else await reloadPreferences();
          if (!failed) setError(previous => previous.startsWith('设置保存失败') ? '' : previous);
        } catch (e) {
          failed = true; activePreferencePatch.current = {};
          applyPreferences(confirmedPreferences.current);
          setError(`设置保存失败：${String(e)}`);
        }
      }
    } finally { preferenceWriteBusy.current = false; setSaving(false); }
  }

  async function toggleCompact() {
    if (busy) return;
    setCompactBusy(true);
    const generation = preferencesGeneration.current;
    try { const next = native ? await compactWindow(!preferences.compact) : { ...preferences, compact: !preferences.compact }; if (generation === preferencesGeneration.current) applyPreferences(next); else await reloadPreferences(); }
    catch (e) { setError(`窗口切换失败：${String(e)}`); }
    finally { setCompactBusy(false); }
  }

  const focusedProject = snapshot.projects.find(project => project.id === preferences.focused_project);
  const focusActive = preferences.focused_project !== null && !preferences.compact;
  const searchQuery = searchText.trim().toLowerCase();
  const searching = searchQuery !== '' && !preferences.compact;
  const recent = preferences.filter === 'recent';
  const projectsInScope = useMemo(() => {
    const scoped = searching ? snapshot.projects : focusActive ? focusedProject ? [focusedProject] : [] : snapshot.projects;
    return searching ? scoped.map(project => ({ ...project, tasks: project.tasks.filter(task => matchesSearch(project, task, searchQuery)) })) : scoped;
  }, [snapshot.projects, searching, searchQuery, focusActive, focusedProject]);
  const allTasks = useMemo(() => projectsInScope.flatMap(project => project.tasks), [projectsInScope]);
  const ongoing = allTasks.filter(task => task.status === 'in_progress').length;
  const attention = allTasks.filter(needsAttention).length;
  const filterCounts: Record<Filter, number | null> = { all: null, attention, in_progress: ongoing, recent: null };
  const visibleProjects = useMemo(() => projectsInScope.map(project => {
    const tasks = project.tasks.filter(task => matchesFilter(task, preferences.filter));
    if (recent) tasks.sort((a, b) => changedAt(b) - changedAt(a) || b.id - a.id);
    return { ...project, tasks };
  }).filter(project => project.tasks.length > 0).sort((a, b) => recent
    ? changedAt(b.tasks[0]) - changedAt(a.tasks[0]) || a.id - b.id
    : Number(preferences.pinned_projects.includes(b.id)) - Number(preferences.pinned_projects.includes(a.id))), [projectsInScope, preferences.filter, preferences.pinned_projects, recent]);
  const matchingCount = visibleProjects.reduce((count, project) => count + project.tasks.length, 0);
  const selectedProject = snapshot.projects.find(project => project.tasks.some(task => task.id === selectedTaskId));
  const selectedTask = selectedProject?.tasks.find(task => task.id === selectedTaskId);
  useEffect(() => {
    if (ready && selectedTaskId !== null && !selectedTask) setSelectedTaskId(null);
  }, [ready, selectedTaskId, selectedTask]);
  const controls = <>
    {!preferences.compact && <><button className={`icon-button ${preferences.always_on_top ? 'is-pinned' : ''}`} title={preferences.always_on_top ? '取消置顶' : '窗口置顶'} aria-label={preferences.always_on_top ? '取消置顶' : '窗口置顶'} aria-pressed={preferences.always_on_top} disabled={locked} onClick={() => void update({ always_on_top: !preferences.always_on_top })}><Icon name="pin" /></button><button className="icon-button" title={preferences.theme === 'light' ? '切换深色' : '切换浅色'} aria-label={preferences.theme === 'light' ? '切换深色' : '切换浅色'} disabled={locked} onClick={() => void update({ theme: preferences.theme === 'light' ? 'dark' : 'light' })}><Icon name={preferences.theme === 'light' ? 'moon' : 'sun'} /></button><button className={`icon-button view-toggle ${preferences.concise ? 'is-active' : ''}`} title={preferences.concise ? '切换详细模式' : '切换简洁模式：仅标题和状态'} aria-label={preferences.concise ? '切换详细模式' : '切换简洁模式'} aria-pressed={preferences.concise} disabled={locked} onClick={() => void update({ concise: !preferences.concise })}><Icon name="list" /></button></>}
    <button className="icon-button" aria-label={preferences.compact ? '展开看板' : '收成窄条'} title={preferences.compact ? '展开看板' : '收成窄条'} disabled={busy} onClick={() => void toggleCompact()}><Icon name={preferences.compact ? 'expand' : 'minus'} /></button>
    <button className="icon-button close-button" aria-label="隐藏到托盘" title={native ? '隐藏到托盘（从托盘恢复）' : '浏览器预览不能隐藏到托盘'} disabled={!native} onClick={() => void hideWindow().catch(e => setError(String(e)))}><Icon name="close" /></button>
  </>;

  return <main className={`app ${preferences.compact ? 'compact' : ''} ${preferences.concise ? 'concise' : ''}`}>
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region><Icon name="logo" /><span data-tauri-drag-region>AgentKanban</span></div>
      {preferences.compact && <div className="strip-counts" data-tauri-drag-region><span className="in_progress">{ongoing} 进行中</span><span className="blocked">{attention} 等你处理</span>{trackingPaused && <span className="paused-chip" title="Agent 记录已暂停">已暂停</span>}</div>}
      <div className="window-actions">{controls}</div>
    </header>
    {!preferences.compact && <>
      {(snapshot.projects.length > 0 || focusActive || searchOpen) && <div className="project-focus"><select aria-label="聚焦项目" value={searching ? '' : preferences.focused_project ?? ''} title={searching ? '搜索期间查找全部项目，清空搜索后恢复原聚焦' : undefined} disabled={locked || searching} onChange={event => void update({ focused_project: event.target.value ? Number(event.target.value) : null })}><option value="">{searching ? '搜索全部项目' : '全部项目'} · {snapshot.projects.length}</option>{focusActive && !focusedProject && <option value={preferences.focused_project!}>聚焦的项目暂无任务</option>}{snapshot.projects.map(project => <option value={project.id} key={project.id}>{project.name}</option>)}</select>{focusActive && !searching && <button className="text-button" disabled={locked} onClick={() => void update({ focused_project: null })}>查看全部</button>}<button ref={searchButton} className={`icon-button search-toggle ${searchOpen ? 'is-active' : ''}`} aria-label={searchOpen ? '收起查找' : '查找任务'} aria-expanded={searchOpen} aria-controls="board-search" title={searchOpen ? '收起查找并清空搜索' : '查找任务（Ctrl+F）'} disabled={!ready} onClick={() => searchOpen ? closeSearch() : revealSearch()}><Icon name="search" /></button></div>}
      {searchOpen && <div id="board-search" className="board-search"><div className="board-search-row"><input ref={searchInput} type="search" maxLength={160} aria-label="搜索全部项目" placeholder="搜索任务或项目" value={searchText} onChange={event => setSearchText(event.target.value)} onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeSearch(); } }} /><button className="text-button" disabled={!searchText} onClick={() => { setSearchText(''); searchInput.current?.focus(); }}>清空搜索</button></div><p className="search-scope">{searching ? `搜索全部项目（不含归档） · ${matchingCount} 项匹配` : '输入后搜索全部项目；仍按当前状态筛选。'}</p></div>}
      <nav className="filters" aria-label="按状态筛选" title="等你处理：受阻、需要你补充或等你验收。最近变更含 Agent 和用户操作，按任务最近变更时间排列。">{(['all', 'attention', 'in_progress', 'recent'] as Filter[]).map(filter => <button key={filter} disabled={locked} aria-pressed={preferences.filter === filter} className={preferences.filter === filter ? 'selected' : ''} onClick={() => void update({ filter })}>{filterLabels[filter]}{filterCounts[filter] !== null && <span className={`filter-count ${filter === 'attention' && attention > 0 ? 'blocked' : ''}`}>{filterCounts[filter]}</span>}</button>)}</nav>
      {trackingPaused && <div className="paused-banner" role="status"><span>Agent 记录已暂停，Agent 照常工作。恢复后在下个正常里程碑或新任务恢复尝试，不回补暂停期间。</span><button disabled={pauseBusy} onClick={() => void togglePause()}>恢复</button></div>}
      {(error || pauseError) && <div className="error" role="alert"><span title={[error, pauseError].filter(Boolean).join('；')}>{[error, pauseError].filter(Boolean).join('；')}</span><button onClick={() => void retryRead()}>重试</button></div>}
      <div className="board" aria-label="项目任务" aria-busy={!ready}>
        {recent && <p className="board-view-hint">按最近变更排序并临时展开，含 Agent 和用户操作；此视图不按置顶排序。</p>}
        {!ready ? <div className="empty"><p>正在读取看板…</p></div> : visibleProjects.length ? visibleProjects.map(project => <ProjectSection key={project.id} project={project} preferences={preferences} searching={searching} update={p => void update(p)} now={now} busy={locked} onOpen={setSelectedTaskId} />) : <div className="empty"><Icon name="logo" /><h2>{searching ? '没有匹配的任务' : snapshot.projects.length ? `没有${preferences.filter === 'all' ? '' : filterLabels[preferences.filter]}的任务` : SLOGAN}</h2>{searching ? <><p>已搜索全部项目，当前状态筛选为“{filterLabels[preferences.filter]}”。</p><button className="outline-button empty-create" onClick={() => { setSearchText(''); searchInput.current?.focus(); }}>清空搜索</button></> : snapshot.projects.length && preferences.filter !== 'all' ? <button className="outline-button empty-create" disabled={locked} onClick={() => void update({ filter: 'all' })}>查看全部</button> : <button className="outline-button empty-create" disabled={!native} onClick={openCapture}>新建任务</button>}{!native && <p className="preview-note">浏览器布局预览 · 请启动桌面版连接本地看板</p>}</div>}
      </div>
      <footer><button className="footer-create" disabled={!native} title="新建任务（Ctrl+Alt+N；窗口内 Ctrl+N）" onClick={openCapture}>＋ 新建</button>{!native ? <span title="浏览器预览不连接本地看板。">布局预览</span> : <button className={`footer-pause ${trackingPaused ? 'is-paused' : ''}`} aria-pressed={trackingPaused} disabled={pauseBusy || !pauseReady} title={trackingPaused ? '下个正常里程碑或新任务恢复尝试，不回补暂停期间' : '暂停后看板工具停止读写，Agent 照常工作'} onClick={() => void togglePause()}>{!pauseReady ? '记录状态未读取' : trackingPaused ? '记录已暂停 · 恢复' : '暂停记录'}</button>}<button className="footer-settings" onClick={() => setSettingsOpen(true)}>设置</button></footer>
    </>}
    {preferences.compact && (error || pauseError) && <span className="compact-error" title={[error, pauseError].filter(Boolean).join('；')} role="alert">!</span>}
    {settingsOpen && <Settings preferences={preferences} busy={busy} disabled={locked} saveError={error} update={patch => void update(patch)} onShortcutChanged={() => void reloadPreferences().catch(e => setError(String(e)))} onClose={() => setSettingsOpen(false)} />}
    {captureOpen && captureDraft && <CapturePanel draft={captureDraft} projects={snapshot.projects} onChange={setCaptureDraft} onCreated={onCreated} onClose={() => setCaptureOpen(false)} />}
    {selectedTask && selectedProject && <TaskDetails key={selectedTask.id} task={selectedTask} project={selectedProject} preferences={preferences} now={now} draft={feedbackDrafts[selectedTask.id]} onDraftChange={draft => setFeedbackDrafts(previous => { const next = { ...previous }; if (draft) next[selectedTask.id] = draft; else delete next[selectedTask.id]; return next; })} onBusyChange={value => { taskActionBusy.current = value; }} onChanged={refreshAfterWrite} onClose={() => setSelectedTaskId(null)} />}
  </main>;
}

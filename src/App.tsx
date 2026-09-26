import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { archiveProject, archiveTask, compactWindow, hideWindow, listArchivedTasks, native, onError, onQuickCreate, onVisibility, readPreferences, readRevision, readSnapshot, readTrackingPaused, readWindowVisible, restoreArchivedTask, reviewTask, savePreferences, setTrackingPaused } from './bridge';
import { defaults, isTutorialProject, isTutorialTask, labels, type ArchivedTask, type CaptureInput, type Filter, type Preferences, type Project, type Snapshot, type Task, type TaskReceipt } from './types';
import { awaitsReview, changedAt, inActiveList, isStale, matchesFilter, matchesSearch, needsAttention, normalizeFilter, relativeTime, reviewLabels, stepProgress } from './display';
import { Settings } from './Panels';
import { Icon } from './Icon';
import { CapturePanel, TaskDetails, type FeedbackDraft } from './Workflows';
import { locale, resolveLanguage, setLanguage, t, type Language } from './i18n';

const filterLabels: Record<Filter, string> = { all: '全部', attention: '等你处理', in_progress: '进行中', recent: '最近变更' };
const SLOGAN = 'Agent 推进，你来验收。';

type OpenMenu = (id: number, x: number, y: number) => void;

// `language` is only a memo key: rows must re-render when the interface language changes.
const TaskRow = memo(function TaskRow({ task, tutorial, concise, recent, now, staleHours, onOpen, onMenu }: { language: Language; task: Task; tutorial: boolean; concise: boolean; recent: boolean; now: number; staleHours: number; onOpen: (id: number) => void; onMenu: OpenMenu }) {
  const timestamp = recent ? task.updated_at : task.agent_updated_at ?? task.updated_at;
  const timeTitle = t("{0}：{1}", recent ? t("最近变更") : !tutorial && task.agent_updated_at ? t("Agent 最后上报") : t("记录时间"), new Date(timestamp).toLocaleString(locale()));
  const pending = awaitsReview(task);
  const steps = concise ? '' : stepProgress(task);
  const statusLabel = concise && pending ? t("未验收") : concise && task.review_status === 'accepted' ? t("已验收") : t(labels[task.status]);
  return <li className={`task task-${task.status} ${concise ? 'task-concise' : ''} ${recent ? 'task-recent' : ''}`} onContextMenu={event => {
    event.preventDefault();
    // The keyboard menu key reports no pointer position; anchor to the row instead.
    const box = event.currentTarget.getBoundingClientRect();
    onMenu(task.id, event.clientX || box.left + 24, event.clientY || box.top + 24);
  }}>
    <button className="task-open" aria-label={t("查看任务：{0}", task.title)} title={pending ? t("Agent 已完成，你尚未验收。右键可一键验收或归档") : undefined} onClick={() => onOpen(task.id)}>
      <span className="task-heading"><span className="task-title" title={recent ? `${task.title}\n${timeTitle}` : task.title}>{task.title}</span><span className={`status ${task.status} ${concise && pending ? 'review-pending' : ''}`}><span className="status-dot" />{statusLabel}</span></span>
      {!concise && <>
      <span className="progress" title={task.progress}>{task.progress || t("尚未补充进展")}</span>
      {(task.agent || task.review_status !== 'none' || task.needs_input || steps || task.review_withdrawn_at) && <span className="task-signals">{steps && <span className="step-count" title={t("计划步骤完成数")}>{steps} {t("步")}</span>}{task.review_withdrawn_at && <span className="review-badge withdrawn" title={t("Agent 在你验收前重新打开了任务")}>{t("已撤回验收")}</span>}{task.review_status !== 'none' && <span className={`review-badge ${task.review_status}`}>{t(reviewLabels[task.review_status])}</span>}{task.needs_input && <span className="input-signal">{t("需要你补充")}</span>}{task.agent && <span className="agent-name" title={tutorial ? t("教学示例") : t("最后上报：{0}", task.agent)}>{task.agent}</span>}</span>}
      <span className="task-meta">{task.branch ? <span className="branch" title={task.branch}><Icon name="branch" /><span>{task.branch}</span></span> : <span>{!tutorial && !task.agent_updated_at ? t("等待 Agent 接手") : ''}</span>}<span className="update-time">{!tutorial && isStale(task, staleHours, now) && <span className="stale" title={t("已超过设置的时间未收到更新；任务状态保持不变。")}>{t("较久未更新")}</span>}<time dateTime={timestamp} title={timeTitle}>{recent && t("最近变更 ")}{relativeTime(timestamp, now)}</time></span></span>
      </>}
    </button>
  </li>;
});

function ProjectMenu({ project, x, y, onArchive, onClose }: { project: Project; x: number; y: number; onArchive: () => void; onClose: () => void }) {
  // Archiving a whole project is a bigger step than one task, so it asks once more.
  const [confirming, setConfirming] = useState(false);
  return <FloatingMenu label={t("项目操作：{0}", project.name)} x={x} y={y} onClose={onClose}>
    <button role="menuitem" className={confirming ? 'danger' : ''} title={t("该项目的任务移到归档，可在归档中心逐条恢复")} onClick={() => confirming ? onArchive() : setConfirming(true)}>{confirming ? t("确认归档 {0} 个任务？", project.tasks.length) : t("归档项目")}</button>
  </FloatingMenu>;
}

function FloatingMenu({ label, x, y, onClose, children }: { label: string; x: number; y: number; onClose: () => void; children: React.ReactNode }) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const node = menu.current;
    if (!node) return;
    setPosition({ left: Math.max(4, Math.min(x, window.innerWidth - node.offsetWidth - 4)), top: Math.max(4, Math.min(y, window.innerHeight - node.offsetHeight - 4)) });
    node.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }, [x, y]);
  useEffect(() => {
    const outside = (event: Event) => { if (!(event.target instanceof Node && menu.current?.contains(event.target))) onClose(); };
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.preventDefault(); onClose(); } };
    window.addEventListener('pointerdown', outside, true);
    window.addEventListener('keydown', key);
    window.addEventListener('blur', onClose);
    window.addEventListener('resize', onClose);
    document.addEventListener('scroll', onClose, true);
    return () => {
      window.removeEventListener('pointerdown', outside, true);
      window.removeEventListener('keydown', key);
      window.removeEventListener('blur', onClose);
      window.removeEventListener('resize', onClose);
      document.removeEventListener('scroll', onClose, true);
    };
  }, [onClose]);
  return <div ref={menu} className="task-menu" role="menu" aria-label={label} style={position} onContextMenu={event => event.preventDefault()}>{children}</div>;
}

function TaskMenu({ task, x, y, onAct, onClose }: { task: Task; x: number; y: number; onAct: (action: 'accept' | 'archive') => void; onClose: () => void }) {
  const reviewable = awaitsReview(task);
  return <FloatingMenu label={t("任务操作：{0}", task.title)} x={x} y={y} onClose={onClose}>
    <button role="menuitem" disabled={!reviewable} title={reviewable ? t("直接通过验收，不填写意见") : task.status === 'done' ? t("该任务无需验收") : t("任务尚未完成")} onClick={() => onAct('accept')}>{t("一键验收")}</button>
    <button role="menuitem" title={t("移到本项目的「已归档」，可随时恢复")} onClick={() => onAct('archive')}>{t("直接归档")}</button>
  </FloatingMenu>;
}

/** Archived tasks of one project, loaded only while the group is open. */
function ArchivedGroup({ project, busy, onChanged }: { project: Project; busy: boolean; onChanged: () => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<ArchivedTask[]>([]);
  const [next, setNext] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [restoring, setRestoring] = useState<number | null>(null);
  const [error, setError] = useState('');
  const request = useRef(0);
  const load = useCallback(async (offset: number) => {
    const current = ++request.current;
    setLoading(true); setError('');
    try {
      const page = await listArchivedTasks({ project_id: project.id, limit: 20, offset });
      if (current !== request.current) return;
      setItems(previous => offset ? [...previous, ...page.items] : page.items); setNext(page.next_offset);
    } catch (e) { if (current === request.current) setError(t("读取归档失败：{0}", String(e))); }
    finally { if (current === request.current) setLoading(false); }
  }, [project.id]);
  // A new archive or restore changes the count; reload from the start.
  useEffect(() => { if (open) void load(0); }, [open, load, project.archived_count]);
  async function restore(task: ArchivedTask) {
    setRestoring(task.id); setError('');
    try { await restoreArchivedTask(task.id, task.updated_at); await onChanged(); }
    catch (e) { setError(t("恢复失败：{0}", String(e))); void load(0); }
    finally { setRestoring(null); }
  }
  return <div className="completed archived-group">
    <button className="disclosure completed-toggle" disabled={busy} aria-expanded={open} onClick={() => setOpen(value => !value)}><Icon name="chevron" className={open ? 'rotated' : ''} />{t("已归档")} <span>{project.archived_count}</span></button>
    {open && <>
      <ul className="archived-list">{items.map(task => <li key={task.id}>
        <span className="archived-title" title={task.title}>{task.title}</span>
        <span className="archived-state">{t(labels[task.status])}{task.status === 'done' && task.review_status !== 'none' ? ` · ${t(reviewLabels[task.review_status])}` : ''}</span>
        <button className="text-button" disabled={restoring !== null} onClick={() => void restore(task)}>{restoring === task.id ? t("恢复中…") : t("恢复")}</button>
      </li>)}</ul>
      {loading && !items.length && <p className="archived-note">{t("正在读取…")}</p>}
      {next !== null && <button className="disclosure more" disabled={loading} onClick={() => void load(next)}>{loading ? t("正在读取…") : t("加载更多")}</button>}
      {error && <p className="panel-error" role="alert">{error}</p>}
    </>}
  </div>;
}

function ProjectSection({ project, preferences, searching, update, now, busy, onOpen, onMenu, onProjectMenu, onChanged }: { project: Project; preferences: Preferences; searching: boolean; update: (p: Partial<Preferences>) => void; now: number; busy: boolean; onOpen: (id: number) => void; onMenu: OpenMenu; onProjectMenu: OpenMenu; onChanged: () => Promise<void> }) {
  const language = resolveLanguage(preferences.language);
  const recent = preferences.filter === 'recent';
  const forceExpanded = searching || recent;
  const active = project.tasks.filter(inActiveList);
  const rows = forceExpanded ? project.tasks : active;
  const done = project.tasks.filter(task => task.status === 'done');
  const collapsed = !forceExpanded && preferences.collapsed_projects.includes(project.id);
  const completed = preferences.completed_projects.includes(project.id);
  const pinned = preferences.pinned_projects.includes(project.id);
  const toggle = (key: 'collapsed_projects' | 'completed_projects' | 'pinned_projects') => update({ [key]: preferences[key].includes(project.id) ? preferences[key].filter(id => id !== project.id) : [...preferences[key], project.id] });
  const heading = <><Icon name="chevron" className={!collapsed ? 'rotated' : ''} /><h2>{project.name}</h2><span className="project-count">{rows.length}</span></>;
  return <section className="project" aria-label={project.name}>
    <div className="project-header" onContextMenu={event => {
      event.preventDefault();
      const box = event.currentTarget.getBoundingClientRect();
      onProjectMenu(project.id, event.clientX || box.left + 24, event.clientY || box.bottom);
    }}>{forceExpanded ? <div className="project-heading project-heading-static" title={t("{0}\n{1}视图临时展开，原折叠设置保留", project.path, recent ? t("最近变更") : t("搜索"))}>{heading}</div> : <button className="project-heading" disabled={busy} aria-expanded={!collapsed} title={project.path} onClick={() => toggle('collapsed_projects')}>{heading}</button>}<button className={`icon-button project-pin ${pinned ? 'is-pinned' : ''}`} aria-pressed={pinned} aria-label={t("{0}：{1}", pinned ? t("取消置顶项目") : t("置顶项目"), project.name)} title={recent ? t("最近变更视图按时间排序，原置顶设置保留") : pinned ? t("取消项目置顶") : t("将项目排在前面")} disabled={busy || recent} onClick={() => toggle('pinned_projects')}><Icon name="pin" /></button></div>
    {!collapsed && <div className="project-content">
      <ul className="task-list">{rows.map(task => <TaskRow key={task.id} language={language} task={task} tutorial={isTutorialTask(project, task)} concise={preferences.concise} recent={recent} now={preferences.concise ? 0 : now} staleHours={preferences.stale_after_hours} onOpen={onOpen} onMenu={onMenu} />)}</ul>
      {!forceExpanded && preferences.filter === 'all' && done.length > 0 && <div className="completed"><button className="disclosure completed-toggle" disabled={busy} aria-expanded={completed} onClick={() => toggle('completed_projects')}><Icon name="chevron" className={completed ? 'rotated' : ''} />{t("已完成")} <span>{done.length}</span></button>{completed && <ul className="task-list">{done.map(task => <TaskRow key={task.id} language={language} task={task} tutorial={isTutorialTask(project, task)} concise={preferences.concise} recent={false} now={preferences.concise ? 0 : now} staleHours={preferences.stale_after_hours} onOpen={onOpen} onMenu={onMenu} />)}</ul>}</div>}
      {native && !forceExpanded && preferences.filter === 'all' && project.archived_count > 0 && <ArchivedGroup project={project} busy={busy} onChanged={onChanged} />}
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
  const [menu, setMenu] = useState<{ id: number; x: number; y: number } | null>(null);
  const [projectMenu, setProjectMenu] = useState<{ id: number; x: number; y: number } | null>(null);
  const [reloading, setReloading] = useState(false);
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
  setLanguage(resolveLanguage(preferences.language));
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
      setError(previous => previous.startsWith(t("读取失败")) || previous.startsWith(t("启动失败")) || previous.startsWith(t("任务已保存，读取失败")) ? '' : previous);
    } catch (e) { setError(t("读取失败：{0}", String(e))); }
    finally { refreshBusy.current = false; }
  }, [applySnapshot, reloadPreferences]);

  const refreshAfterWrite = useCallback(async () => {
    try { applySnapshot(await readSnapshot()); setError(''); }
    catch (e) { setError(t("读取失败：{0}", String(e))); }
  }, [applySnapshot]);

  const openMenu = useCallback<OpenMenu>((id, x, y) => { if (native) setMenu({ id, x, y }); }, []);
  const closeMenu = useCallback(() => setMenu(null), []);
  const openProjectMenu = useCallback<OpenMenu>((id, x, y) => { if (native) { setMenu(null); setProjectMenu({ id, x, y }); } }, []);
  const closeProjectMenu = useCallback(() => setProjectMenu(null), []);
  async function archiveWholeProject(projectId: number) {
    setProjectMenu(null);
    if (taskActionBusy.current) return;
    taskActionBusy.current = true;
    try { await archiveProject(projectId); await refreshAfterWrite(); }
    catch (e) { setError(t("归档项目失败：{0}", String(e))); }
    finally { taskActionBusy.current = false; }
  }
  async function menuAction(task: Task, action: 'accept' | 'archive') {
    setMenu(null);
    if (taskActionBusy.current) return;
    taskActionBusy.current = true;
    try {
      await (action === 'accept' ? reviewTask(task.id, task.updated_at, true, '') : archiveTask(task.id, task.updated_at));
      await refreshAfterWrite();
    } catch (e) { setError(t("{0}失败：{1}", action === 'accept' ? t("验收") : t("归档"), String(e))); }
    finally { taskActionBusy.current = false; }
  }

  async function onCreated(receipt: TaskReceipt) {
    try {
      const next = await readSnapshot(); applySnapshot(next);
      setSelectedTaskId(receipt.id); setError('');
    } catch (e) { setError(t("任务已保存，读取失败：{0}", String(e))); }
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
      if (failures.length) setError(t("启动失败：{0}", failures.join(t("；"))));
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
    const timer = window.setInterval(() => { void refresh(); setNow(previous => Date.now() - previous >= 30_000 ? Date.now() : previous); }, 1000);
    return () => window.clearInterval(timer);
  }, [ready, visible, refresh]);

  useEffect(() => { document.documentElement.dataset.theme = preferences.theme; }, [preferences.theme]);

  const reloadPause = useCallback(async () => {
    try { setPausedState(await readTrackingPaused()); setPauseReady(true); setPauseError(''); }
    catch (e) { setPauseError(t("读取记录状态失败：{0}", String(e))); }
  }, []);

  useEffect(() => { void reloadPause(); }, [reloadPause]);

  async function retryRead() {
    await Promise.all([refresh(true), reloadPause()]);
  }

  async function manualRefresh() {
    if (reloading) return;
    setReloading(true); setNow(Date.now());
    // Keep the spin visible long enough to register as feedback.
    try { await Promise.all([retryRead(), new Promise(resolve => setTimeout(resolve, 450))]); }
    finally { setReloading(false); }
  }

  async function togglePause() {
    if (pauseBusy || !pauseReady) return;
    setPauseBusy(true);
    try { setPausedState(await setTrackingPaused(!trackingPaused)); setPauseError(''); }
    catch (e) { setPauseError(t("切换记录状态失败：{0}", String(e))); }
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
          if (!failed) setError(previous => previous.startsWith(t("设置保存失败")) ? '' : previous);
        } catch (e) {
          failed = true; activePreferencePatch.current = {};
          applyPreferences(confirmedPreferences.current);
          setError(t("设置保存失败：{0}", String(e)));
        }
      }
    } finally { preferenceWriteBusy.current = false; setSaving(false); }
  }

  async function toggleCompact() {
    if (busy) return;
    setCompactBusy(true);
    const generation = preferencesGeneration.current;
    try { const next = native ? await compactWindow(!preferences.compact) : { ...preferences, compact: !preferences.compact }; if (generation === preferencesGeneration.current) applyPreferences(next); else await reloadPreferences(); }
    catch (e) { setError(t("窗口切换失败：{0}", String(e))); }
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
  // Recent order follows the project's newest task, whatever the status filter shows.
  const latestChange = useMemo(() => new Map(projectsInScope.map(project => [project.id, Math.max(0, ...project.tasks.map(changedAt))])), [projectsInScope]);
  const ongoing = allTasks.filter(task => task.status === 'in_progress').length;
  const attention = allTasks.filter(needsAttention).length;
  const filterCounts: Record<Filter, number | null> = { all: null, attention, in_progress: ongoing, recent: null };
  const visibleProjects = useMemo(() => projectsInScope.map(project => {
    const tasks = project.tasks.filter(task => matchesFilter(task, preferences.filter));
    if (recent) tasks.sort((a, b) => changedAt(b) - changedAt(a) || b.id - a.id);
    return { ...project, tasks };
  }).filter(project => project.tasks.length > 0).sort((a, b) => recent
    ? changedAt(b.tasks[0]) - changedAt(a.tasks[0]) || a.id - b.id
    : Number(preferences.pinned_projects.includes(b.id)) - Number(preferences.pinned_projects.includes(a.id))
      || (preferences.project_sort === 'name'
        ? a.name.localeCompare(b.name, locale(), { sensitivity: 'base', numeric: true }) || a.id - b.id
        : latestChange.get(b.id)! - latestChange.get(a.id)! || a.id - b.id)), [projectsInScope, preferences.filter, preferences.pinned_projects, preferences.project_sort, recent, latestChange]);
  const matchingCount = visibleProjects.reduce((count, project) => count + project.tasks.length, 0);
  const selectedProject = snapshot.projects.find(project => project.tasks.some(task => task.id === selectedTaskId));
  const selectedTask = selectedProject?.tasks.find(task => task.id === selectedTaskId);
  const menuProject = projectMenu ? snapshot.projects.find(project => project.id === projectMenu.id) : undefined;
  const menuTask = menu ? snapshot.projects.flatMap(project => project.tasks).find(task => task.id === menu.id) : undefined;
  useEffect(() => {
    if (ready && selectedTaskId !== null && !selectedTask) setSelectedTaskId(null);
  }, [ready, selectedTaskId, selectedTask]);
  const controls = <>
    {!preferences.compact && <><button className={`icon-button ${preferences.always_on_top ? 'is-pinned' : ''}`} title={preferences.always_on_top ? t("取消置顶") : t("窗口置顶")} aria-label={preferences.always_on_top ? t("取消置顶") : t("窗口置顶")} aria-pressed={preferences.always_on_top} disabled={locked} onClick={() => void update({ always_on_top: !preferences.always_on_top })}><Icon name="pin" /></button><button className="icon-button" title={preferences.theme === 'light' ? t("切换深色") : t("切换浅色")} aria-label={preferences.theme === 'light' ? t("切换深色") : t("切换浅色")} disabled={locked} onClick={() => void update({ theme: preferences.theme === 'light' ? 'dark' : 'light' })}><Icon name={preferences.theme === 'light' ? 'moon' : 'sun'} /></button><button className={`icon-button view-toggle ${preferences.concise ? 'is-active' : ''}`} title={preferences.concise ? t("切换详细模式") : t("切换简洁模式：仅标题和状态")} aria-label={preferences.concise ? t("切换详细模式") : t("切换简洁模式")} aria-pressed={preferences.concise} disabled={locked} onClick={() => void update({ concise: !preferences.concise })}><Icon name="list" /></button></>}
    <button className="icon-button" aria-label={preferences.compact ? t("展开看板") : t("收成窄条")} title={preferences.compact ? t("展开看板") : t("收成窄条")} disabled={busy} onClick={() => void toggleCompact()}><Icon name={preferences.compact ? 'expand' : 'minus'} /></button>
    <button className="icon-button close-button" aria-label={t("隐藏到托盘")} title={native ? t("隐藏到托盘（从托盘恢复）") : t("浏览器预览不能隐藏到托盘")} disabled={!native} onClick={() => void hideWindow().catch(e => setError(String(e)))}><Icon name="close" /></button>
  </>;

  return <main className={`app ${preferences.compact ? 'compact' : ''} ${preferences.concise ? 'concise' : ''}`}>
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region><Icon name="logo" /><span data-tauri-drag-region>AgentKanban</span></div>
      {preferences.compact && <div className="strip-counts" data-tauri-drag-region><span className="in_progress">{ongoing} {t("进行中")}</span><span className="blocked">{attention} {t("等你处理")}</span>{trackingPaused && <span className="paused-chip" title={t("Agent 记录已暂停")}>{t("已暂停")}</span>}</div>}
      <div className="window-actions">{controls}</div>
    </header>
    {!preferences.compact && <>
      {(snapshot.projects.length > 0 || focusActive || searchOpen) && <div className="project-focus"><select aria-label={t("聚焦项目")} value={searching ? '' : preferences.focused_project ?? ''} title={searching ? t("搜索期间查找全部项目，清空搜索后恢复原聚焦") : undefined} disabled={locked || searching} onChange={event => void update({ focused_project: event.target.value ? Number(event.target.value) : null })}><option value="">{searching ? t("搜索全部项目") : t("全部项目")} · {snapshot.projects.length}</option>{focusActive && !focusedProject && <option value={preferences.focused_project!}>{t("聚焦的项目暂无任务")}</option>}{snapshot.projects.map(project => <option value={project.id} key={project.id}>{project.name}</option>)}</select>{focusActive && !searching && <button className="text-button" disabled={locked} onClick={() => void update({ focused_project: null })}>{t("查看全部")}</button>}<button ref={searchButton} className={`icon-button search-toggle ${searchOpen ? 'is-active' : ''}`} aria-label={searchOpen ? t("收起查找") : t("查找任务")} aria-expanded={searchOpen} aria-controls="board-search" title={searchOpen ? t("收起查找并清空搜索") : t("查找任务（Ctrl+F）")} disabled={!ready} onClick={() => searchOpen ? closeSearch() : revealSearch()}><Icon name="search" /></button><button className={`icon-button refresh-button ${reloading ? 'is-spinning' : ''}`} aria-label={t("刷新看板")} title={t("刷新任务状态")} disabled={!ready || reloading} onClick={() => void manualRefresh()}><Icon name="refresh" /></button></div>}
      {searchOpen && <div id="board-search" className="board-search"><div className="board-search-row"><input ref={searchInput} type="search" maxLength={160} aria-label={t("搜索全部项目")} placeholder={t("搜索任务或项目")} value={searchText} onChange={event => setSearchText(event.target.value)} onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeSearch(); } }} /><button className="text-button" disabled={!searchText} onClick={() => { setSearchText(''); searchInput.current?.focus(); }}>{t("清空搜索")}</button></div>{searching && <p className="search-scope">{t("搜索全部项目（不含归档） · {0} 项匹配", matchingCount)}</p>}</div>}
      <nav className="filters" aria-label={t("按状态筛选")} title={t("等你处理：受阻或需要你补充。已完成的任务直接变灰，可右键一键验收。最近变更含 Agent 和用户操作，按任务最近变更时间排列。")}>{(['all', 'attention', 'in_progress', 'recent'] as Filter[]).map(filter => <button key={filter} disabled={locked} aria-pressed={preferences.filter === filter} className={preferences.filter === filter ? 'selected' : ''} onClick={() => void update({ filter })}>{t(filterLabels[filter])}{filterCounts[filter] !== null && <span className={`filter-count ${filter === 'attention' && attention > 0 ? 'blocked' : ''}`}>{filterCounts[filter]}</span>}</button>)}</nav>
      {trackingPaused && <div className="paused-banner" role="status" title={t("Agent 照常工作；恢复后在下个正常里程碑或新任务恢复记录，不回补暂停期间。")}><span className="paused-dot" aria-hidden="true" /><span className="paused-text"><strong>{t("记录已暂停")}</strong> {t("· Agent 照常工作")}</span><button disabled={pauseBusy} onClick={() => void togglePause()}>{t("恢复记录")}</button></div>}
      {(error || pauseError) && <div className="error" role="alert"><span title={[error, pauseError].filter(Boolean).join(t("；"))}>{[error, pauseError].filter(Boolean).join(t("；"))}</span><button onClick={() => void retryRead()}>{t("重试")}</button></div>}
      <div className="board" aria-label={t("项目任务")} aria-busy={!ready}>
        {recent && <p className="board-view-hint">{t("按最近变更排序并临时展开，含 Agent 和用户操作；此视图不按置顶排序。")}</p>}
        {!ready ? <div className="empty"><p>{t("正在读取看板…")}</p></div> : visibleProjects.length ? visibleProjects.map(project => <ProjectSection key={project.id} project={project} preferences={preferences} searching={searching} update={p => void update(p)} now={now} busy={locked} onOpen={setSelectedTaskId} onMenu={openMenu} onProjectMenu={openProjectMenu} onChanged={refreshAfterWrite} />) : <div className="empty"><Icon name="logo" /><h2>{searching ? t("没有匹配的任务") : snapshot.projects.length ? t("没有{0}的任务", preferences.filter === 'all' ? '' : t(filterLabels[preferences.filter])) : t(SLOGAN)}</h2>{searching ? <><p>{t("已搜索全部项目，当前状态筛选为“{0}”。", t(filterLabels[preferences.filter]))}</p><button className="outline-button empty-create" onClick={() => { setSearchText(''); searchInput.current?.focus(); }}>{t("清空搜索")}</button></> : snapshot.projects.length && preferences.filter !== 'all' ? <button className="outline-button empty-create" disabled={locked} onClick={() => void update({ filter: 'all' })}>{t("查看全部")}</button> : <button className="outline-button empty-create" disabled={!native} onClick={openCapture}>{t("新建任务")}</button>}{!native && <p className="preview-note">{t("浏览器布局预览 · 请启动桌面版连接本地看板")}</p>}</div>}
      </div>
      <footer><button className="footer-create" disabled={!native} title={t("新建任务（Ctrl+Alt+N；窗口内 Ctrl+N）")} onClick={openCapture}>{t("＋ 新建")}</button>{!native ? <span title={t("浏览器预览不连接本地看板。")}>{t("布局预览")}</span> : <button className={`footer-pause ${trackingPaused ? 'is-paused' : ''}`} aria-pressed={trackingPaused} disabled={pauseBusy || !pauseReady} title={trackingPaused ? t("下个正常里程碑或新任务恢复尝试，不回补暂停期间") : t("暂停后看板工具停止读写，Agent 照常工作")} onClick={() => void togglePause()}>{!pauseReady ? t("记录状态未读取") : trackingPaused ? t("继续记录") : t("暂停记录")}</button>}<button className="footer-settings" onClick={() => setSettingsOpen(true)}>{t("设置")}</button></footer>
    </>}
    {preferences.compact && (error || pauseError) && <span className="compact-error" title={[error, pauseError].filter(Boolean).join(t("；"))} role="alert">!</span>}
    {projectMenu && menuProject && !preferences.compact && <ProjectMenu key={`${projectMenu.id}:${projectMenu.x}:${projectMenu.y}`} project={menuProject} x={projectMenu.x} y={projectMenu.y} onArchive={() => void archiveWholeProject(menuProject.id)} onClose={closeProjectMenu} />}
    {menu && menuTask && !preferences.compact && <TaskMenu key={`${menu.id}:${menu.x}:${menu.y}`} task={menuTask} x={menu.x} y={menu.y} onAct={action => void menuAction(menuTask, action)} onClose={closeMenu} />}
    {settingsOpen && <Settings preferences={preferences} busy={busy} disabled={locked} saveError={error} update={patch => void update(patch)} onShortcutChanged={() => void reloadPreferences().catch(e => setError(String(e)))} onClose={() => setSettingsOpen(false)} />}
    {captureOpen && captureDraft && <CapturePanel draft={captureDraft} projects={snapshot.projects} onChange={setCaptureDraft} onCreated={onCreated} onClose={() => setCaptureOpen(false)} />}
    {selectedTask && selectedProject && <TaskDetails key={selectedTask.id} task={selectedTask} project={selectedProject} preferences={preferences} now={now} draft={feedbackDrafts[selectedTask.id]} onDraftChange={draft => setFeedbackDrafts(previous => { const next = { ...previous }; if (draft) next[selectedTask.id] = draft; else delete next[selectedTask.id]; return next; })} onBusyChange={value => { taskActionBusy.current = value; }} onChanged={refreshAfterWrite} onClose={() => setSelectedTaskId(null)} />}
  </main>;
}

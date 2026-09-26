import { useEffect, useRef, useState, type FormEvent } from 'react';
import { archiveTask, createTask, native, openExternalLink, readHandoff, readTaskReports, reviewTask, sendFeedback } from './bridge';
import { awaitsReview, isStale, relativeTime, reviewLabels, stepProgress } from './display';
import { CopyButton, Panel } from './Panels';
import { isTutorialProject, isTutorialTask, labels, type CaptureInput, type Deliverable, type Preferences, type Project, type Status, type Step, type Task, type TaskReceipt, type TaskReport } from './types';

export function CapturePanel({ draft, projects, onChange, onCreated, onClose }: {
  draft: CaptureInput; projects: Project[]; onChange: (draft: CaptureInput) => void;
  onCreated: (receipt: TaskReceipt) => Promise<void>; onClose: () => void;
}) {
  const titleInput = useRef<HTMLInputElement>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const inFlight = useRef(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (inFlight.current || !native) return;
    inFlight.current = true; setWorking(true); setError('');
    try {
      const receipt = await createTask({ ...draft, title: draft.title.trim(), project_path: draft.project_path.trim() });
      await onCreated(receipt);
    } catch (e) { setError(`创建失败：${String(e)}`); }
    finally { inFlight.current = false; setWorking(false); }
  }
  return <Panel title="新建任务" onClose={onClose} initialFocus={titleInput} busy={working}>
    <form className="panel-body capture-form" onSubmit={event => void submit(event)} onKeyDown={event => {
      if (event.ctrlKey && event.key === 'Enter' && !event.nativeEvent.isComposing) { event.preventDefault(); event.currentTarget.requestSubmit(); }
    }}>
      <fieldset disabled={working}>
      <p className="hint">先记下要做的事，再把开工说明交给 Agent。</p>
      <label className="form-field">任务标题<input ref={titleInput} required maxLength={200} value={draft.title} onChange={event => onChange({ ...draft, title: event.target.value })} placeholder="例如：补齐导出功能的边界情况" /></label>
      <label className="form-field">项目目录<input required list="project-paths" value={draft.project_path} onChange={event => onChange({ ...draft, project_path: event.target.value })} placeholder="选择已有项目，或输入完整目录路径" spellCheck={false} /></label>
      <datalist id="project-paths">{projects.filter(project => !isTutorialProject(project)).map(project => <option key={project.id} value={project.path}>{project.name}</option>)}</datalist>
      <p className="field-hint">使用本机已有目录；同一仓库的 worktree 会归入同一项目。</p>
      <label className="form-field">需求与完成标准 <span className="optional">可选</span><textarea aria-label="需求与完成标准" maxLength={2000} rows={5} value={draft.request} onChange={event => onChange({ ...draft, request: event.target.value })} placeholder="要达到什么效果？有什么限制？如何确认完成？" /></label>
      {error && <p className="panel-error" role="alert">{error}</p>}
      {!native && <p className="hint">浏览器只预览布局，创建任务请使用桌面版。</p>}
      <div className="form-actions"><button className="primary-button" type="submit" disabled={!native || working || !draft.title.trim() || !draft.project_path.trim()}>{working ? '正在保存…' : '创建待办'}</button><span className="hint">Ctrl + Enter 保存</span></div>
      <p className="hint">关闭面板会保留本次未提交的内容，退出程序后不保留。</p>
      </fieldset>
    </form>
  </Panel>;
}

const isWebLink = (value: string) => {
  try { const url = new URL(value); return url.protocol === 'https:' || url.protocol === 'http:'; }
  catch { return false; }
};

export interface FeedbackDraft { note: string; expected_updated_at: string }

// Field order and names for the formatted report; anything else is shown under its raw key.
const reportFields: [string, string][] = [['title', '标题'], ['progress', '进展'], ['needs_input', '需要你补充'], ['next_action', '下一步'], ['goal', '要做什么'], ['acceptance', '验收标准'], ['steps', '计划步骤'], ['step_updates', '步骤更新'], ['deliverables', '交付成果'], ['branch', '分支'], ['agent', 'Agent']];

function reportItem(field: string, item: unknown): string {
  if (field === 'step_updates' && item && typeof item === 'object') {
    const update = item as { index: number; status?: Status | null; note?: string | null };
    const parts = [`第 ${update.index + 1} 步`];
    if (update.status !== undefined) parts.push(update.status === null ? '状态：null' : labels[update.status] ?? update.status);
    if (update.note !== undefined) parts.push(`备注：${update.note === '' ? '（清空）' : update.note}`);
    return parts.join(' · ');
  }
  if (field === 'steps') {
    const step = item as Step;
    return `${labels[step.status] ?? step.status} · ${step.title}${step.note ? `（${step.note}）` : ''}`;
  }
  if (field === 'deliverables') {
    const deliverable = item as Deliverable;
    return `${deliverable.label}：${deliverable.uri}`;
  }
  return typeof item === 'object' ? JSON.stringify(item) : String(item);
}

function ReportValue({ field, value }: { field: string; value: unknown }) {
  if (value === null) return <span className="report-empty">null</span>;
  if (value === '') return <span className="report-empty">（清空）</span>;
  if (!Array.isArray(value)) return <span>{typeof value === 'string' ? value : JSON.stringify(value)}</span>;
  if (!value.length) return <span className="report-empty">（清空）</span>;
  return <ul>{value.map((item, index) => <li key={index}>{reportItem(field, item)}</li>)}</ul>;
}

function AgentReports({ task, now }: { task: Task; now: number }) {
  const [reports, setReports] = useState<TaskReport[] | null>(null);
  const [error, setError] = useState('');
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open || !native) return;
    let disposed = false;
    void readTaskReports(task.id).then(list => { if (!disposed) { setReports(list); setError(''); } }, e => { if (!disposed) setError(String(e)); });
    return () => { disposed = true; };
  }, [open, task.id, task.agent_updated_at]);
  return <details className="agent-reports" onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>Agent 上报记录{reports ? `（${reports.length}）` : ''}</summary>
    <p className="hint">按字段展示；早期版本记录可能经过规范化。</p>
    {error && <p className="panel-error">{error}</p>}
    {reports && !reports.length && <p className="hint">还没有 Agent 上报。</p>}
    {reports?.map(report => {
      // Avoid repeating the current title; preserve explicitly reported values such as branch: null.
      const shown = (key: string) => key !== 'status' && !(key === 'title' && report.payload.title === task.title);
      const keys = [...reportFields.map(([key]) => key).filter(key => key in report.payload), ...Object.keys(report.payload).filter(key => !reportFields.some(([known]) => known === key))].filter(shown);
      const status = report.payload.status as Status | undefined;
      return <article key={report.reported_at} className="report">
        <header><time dateTime={report.reported_at} title={new Date(report.reported_at).toLocaleString('zh-CN')}>{relativeTime(report.reported_at, now)}</time>{status && <span className={`status ${status}`}><span className="status-dot" />{labels[status]}</span>}</header>
        <dl>{keys.map(key => <div key={key}><dt>{reportFields.find(([known]) => known === key)?.[1] ?? key}</dt><dd><ReportValue field={key} value={report.payload[key]} /></dd></div>)}</dl>
      </article>;
    })}
  </details>;
}

export function TaskDetails({ task, project, preferences, now, draft, onDraftChange, onBusyChange, onChanged, onClose }: {
  task: Task; project: Project; preferences: Preferences; now: number;
  draft?: FeedbackDraft; onDraftChange: (draft: FeedbackDraft | null) => void;
  onBusyChange: (busy: boolean) => void;
  onChanged: () => Promise<void>; onClose: () => void;
}) {
  const tutorial = isTutorialTask(project, task);
  const [handoff, setHandoff] = useState('');
  const [handoffError, setHandoffError] = useState('');
  const [handoffAttempt, setHandoffAttempt] = useState(0);
  const [handoffOpen, setHandoffOpen] = useState(false);
  // A review starts blank: an old note must not become the reason for sending work back.
  const [note, setNote] = useState(draft?.note ?? (awaitsReview(task) ? '' : task.user_note));
  const [reviewVersion, setReviewVersion] = useState(draft?.expected_updated_at ?? task.updated_at);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [confirmArchive, setConfirmArchive] = useState(false);
  const inFlight = useRef(false);
  const changed = task.updated_at !== reviewVersion;
  const pending = awaitsReview(task);
  const [previousPending, setPreviousPending] = useState(pending);
  if (previousPending !== pending) {
    setPreviousPending(pending);
    // Keep deliberate drafts, but never recycle an automatically loaded old review note.
    if (pending && !draft) setNote('');
  }
  const steps = stepProgress(task);
  useEffect(() => {
    if (!confirmArchive) return;
    const reset = setTimeout(() => setConfirmArchive(false), 4000);
    return () => clearTimeout(reset);
  }, [confirmArchive]);
  useEffect(() => {
    setHandoff(''); setHandoffError('');
    if (tutorial) return;
    let disposed = false;
    void readHandoff(task.id).then(text => { if (!disposed) setHandoff(text); }, e => { if (!disposed) setHandoffError(String(e)); });
    return () => { disposed = true; };
  }, [task.id, task.updated_at, handoffAttempt, tutorial]);

  async function act(action: 'feedback' | 'accept' | 'reject' | 'archive') {
    if (inFlight.current || changed) return;
    if (action === 'reject' && !note.trim()) { setError('请写下需要修改的内容，再退回任务。'); return; }
    if (action === 'archive' && !confirmArchive) { setConfirmArchive(true); return; }
    inFlight.current = true; setWorking(true); onBusyChange(true); setError(''); setMessage('');
    try {
      if (action === 'archive') {
        // The board hides archived tasks, so the panel closes once the refresh drops it.
        await archiveTask(task.id, reviewVersion);
        onDraftChange(null);
        await onChanged();
        return;
      }
      const receipt = action === 'feedback'
        ? await sendFeedback(task.id, reviewVersion, note)
        : await reviewTask(task.id, reviewVersion, action === 'accept', note);
      setReviewVersion(receipt.updated_at);
      onDraftChange(null);
      setMessage(action === 'feedback' ? tutorial ? '教学示例的补充已保存。' : '已保存，Agent 下次接手时会读到。' : action === 'accept' ? '已验收通过。' : '已退回待办。');
      await onChanged();
    } catch (e) { setError(String(e)); await onChanged(); }
    finally { inFlight.current = false; setWorking(false); onBusyChange(false); setConfirmArchive(false); }
  }

  return <Panel title="任务详情" onClose={onClose} busy={working}>
    <div className="panel-body task-details">
      <div className="detail-status"><span className={`status ${task.status}`}><span className="status-dot" />{labels[task.status]}</span>{task.review_status !== 'none' && <span className={`review-badge ${task.review_status}`}>{reviewLabels[task.review_status]}</span>}
        <span className="detail-actions">{!tutorial && handoff && <CopyButton text={handoff} label="交给 Agent" copied="已复制，粘贴到 Agent 会话" title="复制一段提示词，粘贴到任意已接入的 Agent 会话，它会接着做这个任务并更新这条记录" onFailure={() => setHandoffOpen(true)} />}<button className={`text-button ${confirmArchive ? 'danger' : ''}`} disabled={working || changed} title="从看板隐藏，数据保留" onClick={() => void act('archive')}>{confirmArchive ? '确认归档？' : '归档'}</button></span></div>
      <h3>{task.title}</h3>
      {tutorial && <p className="hint tutorial-notice">教学示例：可体验补充、验收通过或退回修改。真实工作请从“新建”选择实际项目目录开始；示例用完可归档。</p>}
      <p className="detail-progress">{task.progress || '尚未补充进展'}</p>
      <div className="agent-attribution"><span>{tutorial ? '教学示例' : task.agent ? `${task.agent} · 最后上报` : task.agent_updated_at ? 'Agent 最后上报' : '等待 Agent 接手'}</span><time dateTime={task.agent_updated_at ?? task.updated_at} title={`${tutorial ? '示例记录时间：' : ''}${new Date(task.agent_updated_at ?? task.updated_at).toLocaleString('zh-CN')}`}>{relativeTime(task.agent_updated_at ?? task.updated_at, now)}</time></div>
      {!tutorial && isStale(task, preferences.stale_after_hours, now) && <p className="hint stale">较久未收到 Agent 更新</p>}
      {task.review_withdrawn_at && <p className="hint withdrawn-notice">Agent 在你验收前重新打开了任务</p>}
      {!tutorial && handoffError && <p className="panel-error" role="alert">{handoffError} <button className="text-button" onClick={() => setHandoffAttempt(attempt => attempt + 1)}>重试</button></p>}
      {!tutorial && handoff && <details className="handoff-content" open={handoffOpen} onToggle={event => setHandoffOpen(event.currentTarget.open)}><summary>查看开工说明</summary><pre className="guidance" tabIndex={0}>{handoff}</pre></details>}
      {task.needs_input && <section className="workflow-section needs-input"><h4>需要你补充</h4><p>{task.needs_input}</p></section>}
      <section className="workflow-section"><h4>要做什么</h4><p>{task.goal || task.request || 'Agent 尚未写明目标，下次更新时会补上。'}</p>{task.goal && task.request && <details><summary>你的原始需求</summary><p>{task.request}</p></details>}</section>
      {task.acceptance.length > 0 && <section className="workflow-section"><h4>验收标准</h4><ul className="acceptance">{task.acceptance.map((item, index) => <li key={`${index}-${item}`}>{item}</li>)}</ul></section>}
      {task.steps.length > 0 && <section className="workflow-section"><h4>计划步骤 <span className="optional">{steps}</span></h4><ol className="steps">{task.steps.map((step, index) => <li key={`${index}-${step.title}`} className={`step step-${step.status}`}><span className={`status ${step.status}`}><span className="status-dot" />{labels[step.status]}</span><span className="step-title">{step.title}</span>{step.note && <span className="step-note">{step.note}</span>}</li>)}</ol></section>}
      {task.next_action && <section className="workflow-section"><h4>下一步</h4><p>{task.next_action}</p></section>}
      {task.deliverables.length > 0 && <section className="workflow-section"><h4>交付成果</h4>
        <ul className="deliverables">{task.deliverables.map((item, index) => <li key={`${index}-${item.uri}`}><strong>{item.label}</strong><code>{item.uri}</code><div className="link-actions">{isWebLink(item.uri) && <button className="text-button" onClick={() => void openExternalLink(item.uri).catch(e => setError(String(e)))}>打开链接</button>}<CopyButton text={item.uri} label="复制地址" /></div></li>)}</ul>
      </section>}
      {!pending && task.status === 'done' && task.user_note && <section className="workflow-section"><h4>已保存的用户备注</h4><p>{task.user_note}</p></section>}
      {(pending || task.status !== 'done') && <section className="workflow-section"><h4>{pending ? '验收' : '补充给 Agent'}</h4>
        <textarea aria-label={pending ? '修改要求' : '补充给 Agent'} rows={3} maxLength={2000} disabled={working} value={note} onChange={event => { setNote(event.target.value); onDraftChange({ note: event.target.value, expected_updated_at: reviewVersion }); setMessage(''); }} placeholder={pending ? '对照上方验收标准检查成果；退回时写明要改什么' : '补充信息或调整要求，Agent 下次查看这个任务时会读到并照做'} />
        {task.user_note && task.user_note !== note && <details><summary>{pending ? '之前的补充' : '已保存的补充'}</summary><p>{task.user_note}</p></details>}
        {changed && <div className="version-notice" role="status"><p>Agent 刚更新了任务，先看一眼上方内容</p><button className="text-button" onClick={() => { setReviewVersion(task.updated_at); if (draft) onDraftChange({ note, expected_updated_at: task.updated_at }); setError(''); setMessage(''); }}>知道了</button></div>}
        {error && <p className="panel-error" role="alert">{error}</p>}
        {message && <p className="connection-ok" role="status">{message}</p>}
        <div className="form-actions end">{pending ? <><button className="primary-button" disabled={working || changed} onClick={() => void act('accept')}>验收通过</button><button className="outline-button" disabled={working || changed || !note.trim()} onClick={() => void act('reject')}>退回修改</button></> : <button className="outline-button" disabled={working || changed || note === task.user_note} onClick={() => void act('feedback')}>{working ? '正在保存…' : '保存'}</button>}</div>
      </section>}
      {!(pending || task.status !== 'done') && error && <p className="panel-error" role="alert">{error}</p>}
      <AgentReports task={task} now={now} />
      <details className="task-identifiers"><summary>项目与任务信息</summary><dl><dt>项目</dt><dd>{project.name}</dd><dt>目录 <CopyButton text={project.path} /></dt><dd className="mono">{project.path}</dd><dt>任务标识 <CopyButton text={task.task_key} /></dt><dd className="mono">{task.task_key}</dd>{task.branch && <><dt>分支</dt><dd>{task.branch}</dd></>}<dt>最近变更</dt><dd>{new Date(task.updated_at).toLocaleString('zh-CN')}</dd></dl></details>
    </div>
  </Panel>;
}

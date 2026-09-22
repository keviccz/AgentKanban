import { useEffect, useRef, useState, type FormEvent } from 'react';
import { createTask, native, openExternalLink, readHandoff, reviewTask, sendFeedback } from './bridge';
import { awaitsReview, isStale, relativeTime, reviewLabels } from './display';
import { CopyButton, Panel } from './Panels';
import { labels, type CaptureInput, type Preferences, type Project, type Task, type TaskReceipt } from './types';

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
      <datalist id="project-paths">{projects.map(project => <option key={project.id} value={project.path}>{project.name}</option>)}</datalist>
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

export function TaskDetails({ task, project, preferences, now, draft, onDraftChange, onBusyChange, onChanged, onClose }: {
  task: Task; project: Project; preferences: Preferences; now: number;
  draft?: FeedbackDraft; onDraftChange: (draft: FeedbackDraft | null) => void;
  onBusyChange: (busy: boolean) => void;
  onChanged: () => Promise<void>; onClose: () => void;
}) {
  const [handoff, setHandoff] = useState('');
  const [handoffError, setHandoffError] = useState('');
  const [handoffAttempt, setHandoffAttempt] = useState(0);
  const [note, setNote] = useState(draft?.note ?? task.user_note);
  const [reviewVersion, setReviewVersion] = useState(draft?.expected_updated_at ?? task.updated_at);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const inFlight = useRef(false);
  const changed = task.updated_at !== reviewVersion;
  const pending = awaitsReview(task);
  useEffect(() => {
    let disposed = false;
    setHandoff(''); setHandoffError('');
    void readHandoff(task.id).then(text => { if (!disposed) setHandoff(text); }, e => { if (!disposed) setHandoffError(String(e)); });
    return () => { disposed = true; };
  }, [task.id, task.updated_at, handoffAttempt]);

  async function act(action: 'feedback' | 'accept' | 'reject') {
    if (inFlight.current || changed) return;
    if (action === 'reject' && !note.trim()) { setError('请写下需要修改的内容，再退回任务。'); return; }
    inFlight.current = true; setWorking(true); onBusyChange(true); setError(''); setMessage('');
    try {
      const receipt = action === 'feedback'
        ? await sendFeedback(task.id, reviewVersion, note)
        : await reviewTask(task.id, reviewVersion, action === 'accept', note);
      setReviewVersion(receipt.updated_at);
      onDraftChange(null);
      setMessage(action === 'feedback' ? '补充已保存。把最新开工说明交给 Agent 后继续。' : action === 'accept' ? '已验收通过。' : '已退回待办，修改要求会随任务交给 Agent。');
      await onChanged();
    } catch (e) { setError(String(e)); await onChanged(); }
    finally { inFlight.current = false; setWorking(false); onBusyChange(false); }
  }

  return <Panel title="任务详情" onClose={onClose} busy={working}>
    <div className="panel-body task-details">
      <div className="detail-status"><span className={`status ${task.status}`}><span className="status-dot" />{labels[task.status]}</span>{task.review_status !== 'none' && <span className={`review-badge ${task.review_status}`}>{reviewLabels[task.review_status]}</span>}</div>
      <h3>{task.title}</h3><p className="detail-progress">{task.progress || '尚未补充进展'}</p>
      <div className="agent-attribution"><span>{task.agent ? `${task.agent} · 最后上报` : task.agent_updated_at ? 'Agent 最后上报' : '已记录 · 等待 Agent 接手'}</span><time dateTime={task.agent_updated_at ?? task.updated_at} title={new Date(task.agent_updated_at ?? task.updated_at).toLocaleString('zh-CN')}>{relativeTime(task.agent_updated_at ?? task.updated_at, now)}</time></div>
      {handoff && <div className="handoff-toolbar"><CopyButton text={handoff} label="复制开工说明" /></div>}
      {isStale(task, preferences.stale_after_hours, now) && <p className="hint stale">较久未收到 Agent 更新，当前状态保留。</p>}
      {task.next_action && <section className="workflow-section"><h4>下一步</h4><p>{task.next_action}</p></section>}
      {task.needs_input && <section className="workflow-section needs-input"><h4>需要你补充</h4><p>{task.needs_input}</p></section>}
      {task.request && <section className="workflow-section"><h4>原始需求</h4><p>{task.request}</p></section>}
      <section className="workflow-section"><h4>交付成果 {task.deliverables.length > 0 && <span className="optional">{task.deliverables.length}</span>}</h4>
        {task.deliverables.length ? <ul className="deliverables">{task.deliverables.map((item, index) => <li key={`${index}-${item.uri}`}><strong>{item.label}</strong><code>{item.uri}</code><div className="link-actions">{isWebLink(item.uri) && <button className="text-button" onClick={() => void openExternalLink(item.uri).catch(e => setError(String(e)))}>打开链接</button>}<CopyButton text={item.uri} label="复制地址" /></div></li>)}</ul> : <p className="hint">Agent 尚未提供成果链接或路径。</p>}
      </section>
      <section className="workflow-section"><h4>{pending ? '检查成果后验收' : '给 Agent 的补充'}</h4>
        {pending && <p className="hint">Agent 已报告完成。查看成果后确认通过，或写下修改要求退回待办。</p>}
        <label className="form-field">补充或修改要求<textarea aria-label="补充或修改要求" rows={3} maxLength={2000} disabled={working} value={note} onChange={event => { setNote(event.target.value); onDraftChange({ note: event.target.value, expected_updated_at: reviewVersion }); setMessage(''); }} placeholder="补充所需信息，或说明哪里还需要修改" /></label>
        {task.user_note && task.user_note !== note && <details><summary>当前已保存的补充</summary><p>{task.user_note}</p></details>}
        {changed && <div className="version-notice" role="status"><p>任务已有新进展。请检查上方最新内容，再提交补充或验收。</p><button className="text-button" onClick={() => { setReviewVersion(task.updated_at); if (draft) onDraftChange({ note, expected_updated_at: task.updated_at }); setError(''); setMessage(''); }}>已查看最新内容</button></div>}
        {error && <p className="panel-error" role="alert">{error}</p>}
        {message && <p className="connection-ok" role="status">{message}</p>}
        <div className="form-actions">{pending ? <><button className="primary-button" disabled={working || changed} onClick={() => void act('accept')}>验收通过</button><button className="outline-button" disabled={working || changed || !note.trim()} onClick={() => void act('reject')}>退回修改</button></> : <button className="outline-button" disabled={working || changed || note === task.user_note} onClick={() => void act('feedback')}>{working ? '正在保存…' : '保存补充'}</button>}</div>
      </section>
      <section className="workflow-section"><h4>交给 Agent 继续</h4><p className="hint">将开工说明复制到已接入看板的 Agent 会话，它会查询并更新同一条任务。这里不会自动启动 Agent。</p>
        {handoffError && <p className="panel-error" role="alert">{handoffError} <button className="text-button" onClick={() => setHandoffAttempt(attempt => attempt + 1)}>重试</button></p>}
        {handoff && <details><summary>查看开工说明</summary><pre className="guidance" tabIndex={0}>{handoff}</pre></details>}
      </section>
      <details className="task-identifiers"><summary>项目与任务信息</summary><dl><dt>项目</dt><dd>{project.name}</dd><dt>目录 <CopyButton text={project.path} /></dt><dd>{project.path}</dd><dt>任务标识 <CopyButton text={task.task_key} /></dt><dd>{task.task_key}</dd>{task.branch && <><dt>分支</dt><dd>{task.branch}</dd></>}<dt>最近变更</dt><dd>{new Date(task.updated_at).toLocaleString('zh-CN')}</dd></dl></details>
    </div>
  </Panel>;
}

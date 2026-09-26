import { useEffect, useRef, useState } from 'react';
import { listArchivedTasks, native, restoreArchivedTask } from './bridge';
import { labels, type ArchivedTask } from './types';
import { reviewLabels } from './display';

const uniqueTasks = (tasks: ArchivedTask[]) => [...new Map(tasks.map(task => [task.id, task])).values()];

export function ArchiveCenter({ onBack, onBusyChange }: { onBack: () => void; onBusyChange: (busy: boolean) => void }) {
  const [draft, setDraft] = useState('');
  const [query, setQuery] = useState('');
  const [items, setItems] = useState<ArchivedTask[]>([]);
  const [nextOffset, setNextOffset] = useState<number | null>(null);
  const [loading, setLoading] = useState<'first' | 'more' | null>(native ? 'first' : null);
  const [loaded, setLoaded] = useState(false);
  const [listError, setListError] = useState('');
  const [restoreError, setRestoreError] = useState<{ id: number; text: string } | null>(null);
  const [restoring, setRestoring] = useState<number | null>(null);
  const [notice, setNotice] = useState('');
  const queryRef = useRef('');
  const requestVersion = useRef(0);
  const restoreInFlight = useRef(false);
  const mounted = useRef(false);
  const searchInput = useRef<HTMLInputElement>(null);

  async function load(offset = 0, search = queryRef.current) {
    if (!native) return;
    const version = ++requestVersion.current;
    const first = offset === 0;
    queryRef.current = search;
    setQuery(search); setLoading(first ? 'first' : 'more'); setListError(''); setRestoreError(null);
    if (first) { setItems([]); setNextOffset(null); setLoaded(false); }
    try {
      const page = await listArchivedTasks({ query: search || undefined, limit: 20, offset });
      if (!mounted.current || version !== requestVersion.current) return;
      setItems(previous => uniqueTasks(first ? page.items : [...previous, ...page.items]));
      setNextOffset(page.next_offset); setLoaded(true);
    } catch (e) {
      if (mounted.current && version === requestVersion.current) setListError(`读取归档失败：${String(e)}`);
    } finally {
      if (mounted.current && version === requestVersion.current) setLoading(null);
    }
  }
  useEffect(() => {
    mounted.current = true;
    searchInput.current?.focus();
    void load();
    return () => { mounted.current = false; requestVersion.current += 1; };
  }, []);

  async function restore(task: ArchivedTask) {
    if (restoreInFlight.current) return;
    restoreInFlight.current = true;
    setRestoring(task.id); setRestoreError(null); setNotice(''); onBusyChange(true);
    try {
      await restoreArchivedTask(task.id, task.updated_at);
      if (!mounted.current) return;
      const completed = task.status === 'done' && task.review_status !== 'pending';
      setNotice(`“${task.title}”已恢复到原项目“${task.project_name}”。${completed ? '可在该项目的“已完成”分组查看。' : '返回看板后可查看，任务状态保持不变。'}`);
      setItems(previous => previous.filter(item => item.id !== task.id));
      // Restoring shifts offset pages; start again to avoid skipping an archived task.
      void load(0);
    } catch (e) {
      if (mounted.current) setRestoreError({ id: task.id, text: `恢复失败：${String(e)}` });
    } finally {
      restoreInFlight.current = false;
      if (mounted.current) { setRestoring(null); onBusyChange(false); }
    }
  }
  function search(value: string) { setNotice(''); void load(0, value.trim()); }

  return <div className="panel-body archive-center">
    <div className="archive-toolbar"><button className="text-button" disabled={restoring !== null} onClick={onBack}>返回设置</button><button className="text-button" disabled={!native || loading !== null || restoring !== null} onClick={() => { setNotice(''); void load(0); }}>刷新列表</button></div>
    <p className="hint">恢复到原项目并保留任务状态。这里显示最近变更时间。</p>
    <form className="archive-search" onSubmit={event => { event.preventDefault(); search(draft); }}>
      <label className="form-field"><span>搜索归档</span><input ref={searchInput} type="search" value={draft} maxLength={160} aria-label="搜索归档" placeholder="任务标题、项目或关键词" disabled={!native || restoring !== null} onChange={event => setDraft(event.target.value)} /></label>
      <button className="outline-button" type="submit" disabled={!native || restoring !== null}>搜索</button>
      {query && <button className="text-button" type="button" disabled={restoring !== null} onClick={() => { setDraft(''); search(''); }}>清除搜索</button>}
    </form>
    {notice && <p className="connection-ok archive-notice" role="status">{notice}</p>}
    {listError && <div className="archive-error" role="alert"><p className="panel-error">{listError}</p><button className="text-button" disabled={loading !== null || restoring !== null} onClick={() => void load(0)}>重新读取归档</button></div>}
    {!native ? <p className="hint">请在桌面版查看和恢复归档任务。</p> : <>
      {loading === 'first' && <p className="hint" role="status">正在读取归档…</p>}
      {loaded && <p className="hint archive-count">{query ? `“${query}”：` : ''}已显示 {items.length} 项{nextOffset !== null ? '，可继续加载' : ''}</p>}
      {loaded && !loading && items.length === 0 && <p className="archive-empty">{query ? '没有匹配的归档任务。' : '暂无归档任务。'}</p>}
      <ul className="archive-list" aria-label="归档任务" aria-busy={loading !== null}>
        {items.map(task => <li key={task.id} className="archive-task">
          <div className="archive-project"><span>{task.project_name}</span><small title={task.project_path}>{task.project_path}</small></div>
          <h3>{task.title}</h3>
          <p className="archive-state">原状态：<span className={task.status}>{labels[task.status]}</span>{reviewLabels[task.review_status] && ` · ${reviewLabels[task.review_status]}`}</p>
          {task.progress && <p className="archive-progress" title={task.progress}>{task.progress}</p>}
          <div className="archive-task-footer"><span>最近变更 <time dateTime={task.updated_at}>{new Date(task.updated_at).toLocaleString('zh-CN')}</time></span><button className="text-button" disabled={loading !== null || restoring !== null || restoreError?.id === task.id} onClick={() => void restore(task)} aria-label={`恢复任务：${task.title}`}>{restoring === task.id ? '正在恢复…' : '恢复'}</button></div>
          {restoreError?.id === task.id && <div className="archive-error" role="alert"><p className="panel-error">{restoreError.text}</p><button className="text-button" disabled={loading !== null || restoring !== null} onClick={() => void load(0)}>刷新后重试</button></div>}
        </li>)}
      </ul>
      {nextOffset !== null && <button className="outline-button archive-more" disabled={loading !== null || restoring !== null} onClick={() => void load(nextOffset)}>{loading === 'more' ? '正在加载…' : '加载更多'}</button>}
    </>}
  </div>;
}

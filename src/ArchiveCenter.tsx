import { useEffect, useRef, useState } from 'react';
import { Icon } from './Icon';
import { listArchivedTasks, native, restoreArchivedTask } from './bridge';
import { labels, type ArchivedTask } from './types';
import { reviewLabels } from './display';
import { locale, t } from './i18n';

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
      if (mounted.current && version === requestVersion.current) setListError(t("读取归档失败：{0}", String(e)));
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
      setNotice(t("“{0}”已恢复到原项目“{1}”。{2}", task.title, task.project_name, completed ? t("可在该项目的“已完成”分组查看。") : t("返回看板后可查看，任务状态保持不变。")));
      setItems(previous => previous.filter(item => item.id !== task.id));
      // Restoring shifts offset pages; start again to avoid skipping an archived task.
      void load(0);
    } catch (e) {
      if (mounted.current) setRestoreError({ id: task.id, text: t("恢复失败：{0}", String(e)) });
    } finally {
      restoreInFlight.current = false;
      if (mounted.current) { setRestoring(null); onBusyChange(false); }
    }
  }
  function search(value: string) { setNotice(''); void load(0, value.trim()); }

  return <div className="panel-body archive-center">
    <div className="archive-toolbar"><button className="icon-button" aria-label={t("返回设置")} title={t("返回设置")} disabled={restoring !== null} onClick={onBack}><Icon name="back" /></button><button className={`icon-button ${loading !== null ? 'is-spinning' : ''}`} aria-label={t("刷新列表")} title={t("刷新列表")} disabled={!native || loading !== null || restoring !== null} onClick={() => { setNotice(''); void load(0); }}><Icon name="refresh" /></button></div>
    <p className="hint">{t("恢复到原项目并保留任务状态。这里显示最近变更时间。")}</p>
    <form className="archive-search" onSubmit={event => { event.preventDefault(); search(draft); }}>
      <label className="form-field"><span>{t("搜索归档")}</span><input ref={searchInput} type="search" value={draft} maxLength={160} aria-label={t("搜索归档")} placeholder={t("任务标题、项目或关键词")} disabled={!native || restoring !== null} onChange={event => setDraft(event.target.value)} /></label>
      <button className="outline-button" type="submit" disabled={!native || restoring !== null}>{t("搜索")}</button>
      {query && <button className="text-button" type="button" disabled={restoring !== null} onClick={() => { setDraft(''); search(''); }}>{t("清除搜索")}</button>}
    </form>
    {notice && <p className="connection-ok archive-notice" role="status">{notice}</p>}
    {listError && <div className="archive-error" role="alert"><p className="panel-error">{listError}</p><button className="text-button" disabled={loading !== null || restoring !== null} onClick={() => void load(0)}>{t("重新读取归档")}</button></div>}
    {!native ? <p className="hint">{t("请在桌面版查看和恢复归档任务。")}</p> : <>
      {loading === 'first' && <p className="hint" role="status">{t("正在读取归档…")}</p>}
      {loaded && <p className="hint archive-count">{query ? t("“{0}”：", query) : ''}{t("已显示 {0} 项", items.length)}{nextOffset !== null ? t("，可继续加载") : ''}</p>}
      {loaded && !loading && items.length === 0 && <p className="archive-empty">{query ? t("没有匹配的归档任务。") : t("暂无归档任务。")}</p>}
      <ul className="archive-list" aria-label={t("归档任务")} aria-busy={loading !== null}>
        {items.map(task => <li key={task.id} className="archive-task">
          <div className="archive-project"><span>{task.project_name}</span><small title={task.project_path}>{task.project_path}</small></div>
          <h3>{task.title}</h3>
          <p className="archive-state">{t("原状态：")}<span className={task.status}>{t(labels[task.status])}</span>{t(reviewLabels[task.review_status]) && ` · ${t(reviewLabels[task.review_status])}`}</p>
          {task.progress && <p className="archive-progress" title={task.progress}>{task.progress}</p>}
          <div className="archive-task-footer"><span>{t("最近变更")} <time dateTime={task.updated_at}>{new Date(task.updated_at).toLocaleString(locale())}</time></span><button className="text-button" disabled={loading !== null || restoring !== null || restoreError?.id === task.id} onClick={() => void restore(task)} aria-label={t("恢复任务：{0}", task.title)}>{restoring === task.id ? t("正在恢复…") : t("恢复")}</button></div>
          {restoreError?.id === task.id && <div className="archive-error" role="alert"><p className="panel-error">{restoreError.text}</p><button className="text-button" disabled={loading !== null || restoring !== null} onClick={() => void load(0)}>{t("刷新后重试")}</button></div>}
        </li>)}
      </ul>
      {nextOffset !== null && <button className="outline-button archive-more" disabled={loading !== null || restoring !== null} onClick={() => void load(nextOffset)}>{loading === 'more' ? t("正在加载…") : t("加载更多")}</button>}
    </>}
  </div>;
}

import { useEffect, useMemo, useState } from 'react';
import { native, readFinishedSince } from './bridge';
import { CopyButton } from './Panels';
import { locale, t } from './i18n';
import type { ArchivedTask } from './types';

const RANGES = [['今天', 0], ['近 7 天', 7], ['近 30 天', 30]] as const;

function since(days: number) {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - (days ? days - 1 : 0));
  return start.toISOString();
}

/** What Agents finished recently, grouped by project, with a Markdown copy for reports. */
export function Summary() {
  const [days, setDays] = useState(7);
  const [items, setItems] = useState<ArchivedTask[] | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    if (!native) return;
    let current = true;
    setItems(null); setError('');
    readFinishedSince(since(days)).then(rows => { if (current) setItems(rows); }, e => { if (current) setError(t("读取摘要失败：{0}", String(e))); });
    return () => { current = false; };
  }, [days]);
  const groups = useMemo(() => {
    const byProject = new Map<string, ArchivedTask[]>();
    for (const item of items ?? []) byProject.set(item.project_name, [...byProject.get(item.project_name) ?? [], item]);
    return [...byProject];
  }, [items]);
  const range = t(RANGES.find(([, value]) => value === days)![0]);
  const markdown = [
    `## ${t("AgentKanban 工作摘要（{0}）", range)}`, '',
    t("共完成 {0} 个任务，涉及 {1} 个项目。", items?.length ?? 0, groups.length), '',
    ...groups.flatMap(([project, tasks]) => [`### ${project} (${tasks.length})`, ...tasks.map(task => `- ${task.title}${task.agent ? ` · ${task.agent}` : ''}`), '']),
  ].join('\n');
  return <div className="panel-body summary">
    <div className="summary-toolbar">
      <span className="preset-group" role="group" aria-label={t("时间范围")}>{RANGES.map(([label, value]) => <button key={value} type="button" aria-pressed={days === value} onClick={() => setDays(value)}>{t(label)}</button>)}</span>
      {items && items.length > 0 && <CopyButton text={markdown} label={t("复制为 Markdown")} />}
    </div>
    {!native ? <p className="hint">{t("请在桌面版查看工作摘要。")}</p> : error ? <p className="panel-error" role="alert">{error}</p> : items === null ? <p className="hint">{t("正在读取…")}</p> : <>
      <div className="summary-stats"><div><strong>{items.length}</strong><span>{t("完成的任务")}</span></div><div><strong>{groups.length}</strong><span>{t("涉及项目")}</span></div><div><strong>{new Set(items.map(item => item.agent).filter(Boolean)).size}</strong><span>{t("参与的 Agent")}</span></div></div>
      {items.length === 0 ? <p className="archive-empty">{t("这段时间没有完成的任务。")}</p> : groups.map(([project, tasks]) => <section key={project} className="summary-group">
        <h3>{project} <span>{tasks.length}</span></h3>
        <ul>{tasks.map(task => <li key={task.id}><span className="summary-title" title={task.progress}>{task.title}</span><time dateTime={task.agent_updated_at ?? task.updated_at}>{new Date(task.agent_updated_at ?? task.updated_at).toLocaleDateString(locale(), { month: 'numeric', day: 'numeric' })}</time></li>)}</ul>
      </section>)}
    </>}
  </div>;
}

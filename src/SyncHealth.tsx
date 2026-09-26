import { useEffect, useRef, useState } from 'react';
import { native, readSyncHealth } from './bridge';
import type { SyncEvent, SyncHealth as Health } from './types';

const transportLabel = (event: SyncEvent) => event.transport === 'mcp' ? 'MCP' : '备用入口（CLI）';
const outcomeLabel: Record<SyncEvent['outcome'], string> = { ok: '成功', paused: '暂停时跳过', error: '失败' };

function EventRecord({ event, showOutcome = false }: { event: SyncEvent | null; showOutcome?: boolean }) {
  if (!event) return <>暂无记录</>;
  return <><span className={showOutcome && event.outcome !== 'ok' ? 'panel-error' : ''}>{showOutcome && `${outcomeLabel[event.outcome]} · `}{transportLabel(event)}</span><time dateTime={event.at}>{new Date(event.at).toLocaleString('zh-CN')}</time></>;
}

export function SyncHealth() {
  const [health, setHealth] = useState<Health | null>(null);
  const [loading, setLoading] = useState(native);
  const [readError, setReadError] = useState('');
  const [copyResult, setCopyResult] = useState('');
  const requestVersion = useRef(0);
  const diagnostics = useRef<HTMLDetailsElement>(null);
  const reasonText = useRef<HTMLPreElement>(null);

  async function refresh() {
    if (!native) return;
    const version = ++requestVersion.current;
    setLoading(true); setReadError(''); setCopyResult('');
    try {
      const next = await readSyncHealth();
      if (version === requestVersion.current) setHealth(next);
    } catch (e) {
      if (version === requestVersion.current) setReadError(`读取同步健康失败：${String(e)}`);
    } finally {
      if (version === requestVersion.current) setLoading(false);
    }
  }
  useEffect(() => {
    void refresh();
    return () => { requestVersion.current += 1; };
  }, []);

  const call = health?.last_call;
  const errorReason = readError || (call?.outcome === 'error' ? call.error || '未提供错误原因。' : '');
  const briefReason = errorReason.replace(/\s+/g, ' ').trim();
  async function copyReason() {
    const version = requestVersion.current;
    try {
      await navigator.clipboard.writeText(errorReason);
      if (version === requestVersion.current) setCopyResult('已复制');
    } catch {
      if (version !== requestVersion.current) return;
      setCopyResult('复制失败，请选择下方完整文字复制。');
      if (diagnostics.current) diagnostics.current.open = true;
      reasonText.current?.focus();
    }
  }

  return <section className="settings-section sync-health" aria-labelledby="sync-health-title" aria-busy={loading}>
    <div className="section-heading"><h3 id="sync-health-title">同步健康</h3><button className="text-button" disabled={!native || loading} onClick={() => void refresh()}>{loading ? '正在读取…' : '刷新同步状态'}</button></div>
    {!native ? <p className="hint">请在桌面版查看最近的任务处理记录。</p> : <>
      {health?.paused && <p className="panel-error sync-paused" role="status">任务记录已暂停。恢复后在下个正常里程碑或新任务继续尝试，不回补暂停期间的记录。</p>}
      {loading && !health && <p className="hint">正在读取本地处理记录…</p>}
      {readError && <p className="panel-error" role="alert">{briefReason.length > 180 ? `${briefReason.slice(0, 180)}…` : briefReason}{health && ' 以下保留上次读取的记录。'}</p>}
      {health && <>
        {!call && <p className="hint">尚无调用记录。配置并重启客户端后，让 Agent 查询或保存任务，再刷新此处。</p>}
        <dl className="sync-records">
          <div><dt>最近调用</dt><dd><EventRecord event={call ?? null} showOutcome /></dd></div>
          <div><dt>最近成功调用</dt><dd><EventRecord event={health.last_success} /></dd></div>
          <div><dt>最近成功保存</dt><dd><EventRecord event={health.last_write} /></dd></div>
        </dl>
        <p className="hint sync-scope">这是最近处理记录，不代表客户端当前在线；久未更新不表示断线。</p>
      </>}
      {errorReason && <div className="sync-error">
        {!readError && <p className="panel-error">{briefReason.length > 180 ? `${briefReason.slice(0, 180)}…` : briefReason}</p>}
        <span className="copy-control"><button className="text-button" onClick={() => void copyReason()}>复制错误原因</button><span className="copy-result" role="status">{copyResult}</span></span>
      </div>}
      {(call || health?.last_success || health?.last_write || errorReason) && <details className="sync-diagnostics" ref={diagnostics}><summary>更多诊断</summary>
        <dl className="diagnostics">
          {call && <><dt>最近调用工具</dt><dd className="mono">{call.tool}</dd></>}
          {health?.last_success && <><dt>最近成功调用工具</dt><dd className="mono">{health.last_success.tool}</dd></>}
          {health?.last_write && <><dt>最近成功保存工具</dt><dd className="mono">{health.last_write.tool}</dd></>}
        </dl>
        {errorReason && <pre ref={reasonText} className="config-code" tabIndex={0} aria-label="完整错误原因">{errorReason}</pre>}
      </details>}
    </>}
  </section>;
}

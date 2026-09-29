import { useState } from 'react';
import { t } from './i18n';
import { Icon } from './Icon';
import { connectAll, connectAllMessage, pendingClients, type ConnectAllResult } from './Panels';
import type { ClientStatus } from './types';

/** First-run card on the board: connects the detected Agent clients or leads to Settings. */
export function AgentGuide({ clients, onClients, onConnected, onOpenSettings, onDismiss }: {
  clients: ClientStatus[];
  onClients: (clients: ClientStatus[]) => void;
  /** Keeps the card on screen after a connect, so its result stays readable. */
  onConnected: () => void;
  /** Opens Settings → Agents, with the manual snippet of `client` selected when given. */
  onOpenSettings: (client?: string) => void;
  onDismiss: () => void;
}) {
  const [working, setWorking] = useState(false);
  const [result, setResult] = useState<ConnectAllResult | null>(null);
  const pending = pendingClients(clients);
  const detected = clients.filter(client => client.detected);

  async function connect() {
    setWorking(true);
    try {
      const next = await connectAll(clients);
      const updated = new Map(next.done.map(client => [client.id, client]));
      onClients(clients.map(client => updated.get(client.id) ?? client));
      if (next.done.length) onConnected();
      setResult(next);
    } finally { setWorking(false); }
  }

  const failed = result?.failed ?? [];
  return <section className="agent-guide" aria-labelledby="agent-guide-title">
    <div className="agent-guide-head">
      <Icon name="logo" />
      <h2 id="agent-guide-title">{t("接入你的 Agent")}</h2>
      <button className="icon-button agent-guide-close" aria-label={t("关闭引导")} title={t("不再显示；之后可在设置 → Agent 接入中接入")} onClick={onDismiss}><Icon name="close" /></button>
    </div>
    {result ? <>
      {result.done.length > 0 && <p role="status" className="agent-guide-done">{connectAllMessage({ done: result.done, failed: [] })} {t("之后 Agent 开工时，任务会自动出现在这里。")}</p>}
      {failed.length > 0 && <p role="alert" className="panel-error">{connectAllMessage({ done: [], failed })}</p>}
    </> : <>
      <p>{t("接入后，Agent 做会改文件的任务时，会自动把目标、步骤和进度记到看板。")}</p>
      {detected.length > 0
        ? <div className="agent-guide-clients" aria-label={t("已检测到的客户端")}><span>{t("已检测到")}</span>{detected.map(client => <span key={client.id} className="agent-guide-chip">{client.name}</span>)}</div>
        : <p>{t("还没检测到支持的客户端。安装 Codex、Claude Code 等之后回到这里，或在设置中手动配置。")}</p>}
    </>}
    <div className="agent-guide-actions">
      {result ? <>
        <button className={failed.length ? 'text-button' : 'agent-guide-primary'} onClick={onDismiss}>{t("知道了")}</button>
        {failed.length > 0 && <button className="agent-guide-primary" onClick={() => onOpenSettings(failed[0].client.id)}>{t("去设置手动配置")}</button>}
      </> : <>
        {pending.length > 0 && <button className="agent-guide-primary" disabled={working} onClick={() => void connect()}>{working ? t("正在接入…") : t("一键接入全部")}</button>}
        {detected.length === 0 && <button className="text-button" onClick={() => onOpenSettings()}>{t("打开接入设置")}</button>}
      </>}
    </div>
  </section>;
}

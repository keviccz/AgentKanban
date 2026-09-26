import { useEffect, useRef, useState } from 'react';
import { checkUpdates, downloadUpdate, installUpdate, native, onUpdateStatus, openExternalLink, readUpdateStatus } from './bridge';
import type { Preferences, UpdateStatus } from './types';
import { locale, t } from './i18n';

const phaseLabels: Record<UpdateStatus['phase'], string> = {
  idle: '更新检查', checking: '正在检查更新…', available: '发现新版本',
  downloading: '正在后台下载…', ready: '更新已下载，可以安装',
  installing: '正在启动安装程序…', blocked: '更新已下载，安装暂不可用', error: '更新未完成',
};

const RELEASES_URL = 'https://github.com/keviccz/AgentKanban/releases';
const GITHUB_MARK = 'M12 .3a12 12 0 0 0-3.8 23.4c.6.1.8-.3.8-.6v-2c-3.3.7-4-1.6-4-1.6-.6-1.4-1.4-1.8-1.4-1.8-1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2a11.5 11.5 0 0 1 6 0C17.3 4.7 18.3 5 18.3 5c.6 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6A12 12 0 0 0 12 .3';

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${Math.max(0, Math.floor(bytes))} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function Updates({ preferences, disabled, update, onLater }: {
  preferences: Preferences; disabled: boolean;
  update: (patch: Partial<Preferences>) => void; onLater: () => void;
}) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [reading, setReading] = useState(native);
  const [readError, setReadError] = useState('');
  const [listenError, setListenError] = useState('');
  const [actionError, setActionError] = useState('');
  const [attempt, setAttempt] = useState(0);
  const [action, setAction] = useState<'check' | 'download' | 'install' | null>(null);
  const mounted = useRef(false);
  const eventVersion = useRef(0);
  const inFlight = useRef(false);

  useEffect(() => {
    mounted.current = true;
    if (!native) return () => { mounted.current = false; };
    let disposed = false;
    let unlisten: (() => void) | undefined;
    setReading(true); setReadError('');
    async function read() {
      const version = eventVersion.current;
      try {
        const next = await readUpdateStatus();
        if (!disposed && version === eventVersion.current) { setStatus(next); setReadError(''); }
      } catch (e) { if (!disposed && version === eventVersion.current) setReadError(t("读取更新状态失败：{0}", String(e))); }
      finally { if (!disposed) setReading(false); }
    }
    // Native owns checks and downloads; leaving Settings only removes this listener.
    void onUpdateStatus(next => {
      if (disposed) return;
      eventVersion.current += 1;
      setStatus(next); setReading(false); setReadError(''); setActionError('');
    }).then(stop => {
      if (disposed) { stop(); return; }
      unlisten = stop; setListenError(''); void read();
    }, e => {
      if (disposed) return;
      setListenError(t("更新状态自动刷新不可用：{0}", String(e))); void read();
    });
    return () => { disposed = true; mounted.current = false; unlisten?.(); };
  }, [attempt]);

  async function run(kind: 'check' | 'download' | 'install') {
    if (inFlight.current) return;
    inFlight.current = true; setAction(kind); setActionError('');
    const version = eventVersion.current;
    try {
      const next = await (kind === 'check' ? checkUpdates(true) : kind === 'download' ? downloadUpdate() : installUpdate());
      if (mounted.current && version === eventVersion.current) setStatus(next);
    } catch (e) {
      if (mounted.current) { setActionError(t("操作失败：{0}", String(e))); setAttempt(value => value + 1); }
    } finally {
      inFlight.current = false;
      if (mounted.current) setAction(null);
    }
  }

  const phase = status?.phase ?? 'idle';
  const active = action !== null || phase === 'checking' || phase === 'downloading' || phase === 'installing';
  const total = status?.total_bytes && status.total_bytes > 0 ? status.total_bytes : null;
  const downloaded = Math.max(0, status?.downloaded_bytes ?? 0);
  const errorState = phase === 'error' || phase === 'blocked';
  const canDownload = phase === 'available' || (phase === 'error' && Boolean(status?.version));
  const canInstall = phase === 'ready' || phase === 'blocked';

  return <div className="updates">
    <section className="settings-section">
      <h3>{t("软件更新")}</h3>
      {!native ? <p className="hint">{t("请在桌面版检查 GitHub Releases 和安装更新。")}</p> : <>
        <p className={`update-status ${errorState ? 'panel-error' : phase === 'available' || phase === 'ready' ? 'connection-ok' : ''}`} role="status">{reading && !status ? t("正在读取更新状态…") : t(phaseLabels[phase])}</p>
        {status?.message && <p className={errorState ? 'panel-error' : 'hint'}>{t(status.message)}</p>}
        {status && !status.message && phase === 'idle' && <p className="hint">{status.checked_at ? t("本次检查没有可安装的新版本。") : t("尚未检查更新。")}</p>}
        {status?.checked_at && <p className="hint update-checked">{t("上次检查：")}<time dateTime={status.checked_at}>{new Date(status.checked_at).toLocaleString(locale())}</time></p>}
        {phase === 'downloading' && <div className="update-progress">
          <progress aria-label={t("更新下载进度")} max={total ?? 1} value={total ? Math.min(downloaded, total) : undefined} />
          <p>{total ? t("{0} / {1}（{2}%）", formatBytes(downloaded), formatBytes(total), Math.min(100, Math.floor(downloaded / total * 100))) : t("已下载 {0}，总大小未知", formatBytes(downloaded))}</p>
          <p className="hint">{t("返回看板后会继续下载，安装前仍需你确认。")}</p>
        </div>}
        {phase === 'ready' && <p className="hint">{t("点击安装后会退出看板并启动安装程序；任务数据保留。")}</p>}
        {phase === 'blocked' && <p className="hint">{t("可稍后重试安装；完全退出应用后需要重新下载。")}</p>}
        {phase === 'installing' && <p className="hint">{t("请等待安装程序完成；当前显示的版本尚未更新。")}</p>}
        {(readError || listenError || actionError) && <div className="update-error" role="alert">
          {readError && <p className="panel-error">{readError}</p>}{listenError && <p className="panel-error">{listenError}</p>}{actionError && <p className="panel-error">{actionError}</p>}
          {(readError || listenError) && <button className="text-button" disabled={reading} onClick={() => setAttempt(value => value + 1)}>{t("重新读取更新状态")}</button>}
        </div>}
        <div className="form-actions end">
          <button className="outline-button" disabled={reading || active} onClick={() => void run('check')}>{phase === 'checking' || action === 'check' ? t("正在检查…") : phase === 'error' && !status?.version ? t("重试检查") : t("检查更新")}</button>
          {canDownload && <button className="primary-button" disabled={reading || active} onClick={() => void run('download')}>{phase === 'error' ? t("重试下载") : t("后台下载")}</button>}
          {canInstall && <button className="primary-button" disabled={reading || active} onClick={() => void run('install')}>{phase === 'blocked' ? t("重试安装") : t("安装更新")}</button>}
          {phase === 'blocked' && <button className="text-button" onClick={onLater}>{t("稍后")}</button>}
          <button className="github-button" title={RELEASES_URL} onClick={() => void openExternalLink(RELEASES_URL).catch(e => setActionError(String(e)))}><svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d={GITHUB_MARK} /></svg>GitHub</button>
        </div>
        {status?.version && <section className="update-release"><h4>{t("最新版本")} {status.version}</h4><details open><summary>{t("更新说明")}</summary><p>{status.notes || t("此版本未提供更新说明。")}</p></details></section>}
      </>}
    </section>
    <section className="settings-section"><h3>{t("更新偏好")}</h3>
      <label className="setting-row"><span>{t("自动检查更新")}<small>{t("后台检查 GitHub Releases")}</small></span><input type="checkbox" checked={preferences.auto_check_updates ?? true} disabled={!native || disabled} onChange={event => update({ auto_check_updates: event.target.checked })} /></label>
      <label className="setting-row"><span>{t("自动后台下载")}<small>{t("发现新版本时下载，安装仍由你确认")}</small></span><input type="checkbox" checked={preferences.auto_download_updates ?? false} disabled={!native || disabled} onChange={event => update({ auto_download_updates: event.target.checked })} /></label>
    </section>
  </div>;
}

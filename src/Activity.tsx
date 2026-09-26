import { t } from './i18n';

export const ACTIVITY_STYLES = ['pulse', 'orbit', 'dots', 'wave', 'shimmer', 'flow', 'glow', 'off'] as const;
export type ActivityStyle = typeof ACTIVITY_STYLES[number];
export const activityLabels: Record<ActivityStyle, string> = {
  pulse: '呼吸光点', orbit: '旋转光环', dots: '跳动三点', wave: '心电波形',
  shimmer: '文字流光', flow: '卡片左侧流光', glow: '卡片呼吸', off: '关闭',
};

/**
 * Extra shapes some activity styles draw. All are rendered and CSS shows only the
 * one the current style (data-activity on .app) needs.
 */
export function ActivityMark() {
  return <>
    <span className="activity-dots" aria-hidden="true"><i /><i /><i /></span>
    <svg className="activity-wave" viewBox="0 0 20 10" aria-hidden="true"><path d="M0 5h5l2-4 3 8 2.5-6 1.5 2H20" /></svg>
  </>;
}

/** The "推进中" label; wrapped so the shimmer style can paint across the text. */
export const ActivityLabel = () => <span className="activity-label">{t("推进中")}</span>;

import type { ReactNode } from 'react';

export type IconName = 'logo' | 'back' | 'folder' | 'settings' | 'pause' | 'play' | 'pin' | 'sun' | 'moon' | 'list' | 'search' | 'refresh' | 'minus' | 'close' | 'chevron' | 'branch' | 'expand';
export function Icon({ name, className = '' }: { name: IconName; className?: string }) {
  const paths: Record<Exclude<IconName, 'logo'>, ReactNode> = {
    back: <path d="M15 5l-7 7 7 7" />,
    folder: <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5Z" />,
    pause: <><rect x="6.5" y="4.5" width="3.8" height="15" rx="1.2" /><rect x="13.7" y="4.5" width="3.8" height="15" rx="1.2" /></>,
    play: <path d="M7 4.9v14.2a1 1 0 0 0 1.5.9l11.2-7.1a1 1 0 0 0 0-1.8L8.5 4a1 1 0 0 0-1.5.9Z" />,
    settings: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z" /></>,
    pin: <g transform="rotate(35 12 12)"><path d="M9 3h6m-5 0v6l-3 4v2h10v-2l-3-4V3M12 15v6" /></g>,
    sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5" /></>,
    moon: <path d="M20.5 14A8.5 8.5 0 0 1 10 3.5 8.5 8.5 0 1 0 20.5 14Z" />,
    list: <><path d="M8 6h12M8 12h12M8 18h12" /><path d="M4 6h.01M4 12h.01M4 18h.01" strokeWidth="3" /></>,
    search: <><circle cx="10" cy="10" r="6" /><path d="m15 15 6 6" /></>,
    refresh: <><path d="M20 11a8 8 0 0 0-14.3-4.9L4 8" /><path d="M4 3.5V8h4.5" /><path d="M4 13a8 8 0 0 0 14.3 4.9L20 16" /><path d="M20 20.5V16h-4.5" /></>,
    minus: <path d="M5 12h14" />,
    close: <path d="m6 6 12 12M18 6 6 18" />,
    chevron: <path d="m9 5 7 7-7 7" />,
    branch: <><circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="5" r="2" /><path d="M6 7v10m0-3c0-6 12 0 12-7" /></>,
    expand: <path d="m5 9 7 7 7-7" />,
  };
  return <svg className={`icon ${className}`} viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">{name === 'logo' ? <><rect x="3" y="3" width="7" height="18" rx="1.5" fill="currentColor" stroke="none" /><rect x="13" y="3" width="8" height="18" rx="1.5" fill="currentColor" stroke="none" opacity=".65" /></> : paths[name]}</svg>;
}

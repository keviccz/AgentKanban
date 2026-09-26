import { t } from './i18n';

/**
 * A small "已复制" that pops up beside the pointer, rises and fades. It is visual
 * only; callers keep their own role="status" text for screen readers.
 */
export function showCopied(x: number, y: number, text = t("已复制")) {
  const bubble = document.createElement('span');
  bubble.className = 'copy-bubble';
  bubble.textContent = text;
  bubble.setAttribute('aria-hidden', 'true');
  // Keyboard clicks report 0,0; fall back to the focused control.
  if (!x && !y && document.activeElement instanceof HTMLElement) {
    const box = document.activeElement.getBoundingClientRect();
    x = box.left + box.width / 2;
    y = box.top;
  }
  bubble.style.left = `${Math.min(x + 10, window.innerWidth - 70)}px`;
  bubble.style.top = `${Math.max(y - 26, 4)}px`;
  // Inside an open dialog the page body sits under the top layer; attach there instead.
  const host = document.querySelector('dialog[open]') ?? document.body;
  host.appendChild(bubble);
  bubble.addEventListener('animationend', () => bubble.remove(), { once: true });
  window.setTimeout(() => bubble.remove(), 1500);
}

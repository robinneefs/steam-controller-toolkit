// Writes for things updated with every snapshot (~60 per second). The browser treats any write as
// a change, even of the same value, and restyles for it; with the window open that cost about
// 14% of a CPU core on a tab that showed nothing new. So write only what changed.

export function setText(el: Element, text: string): void {
  if (el.textContent !== text) el.textContent = text;
}

export function setHtml(el: Element, html: string): void {
  // innerHTML reads back normalised, so compare with what was last written instead.
  const e = el as Element & { lastHtml?: string };
  if (e.lastHtml !== html) el.innerHTML = e.lastHtml = html;
}

export function setAttr(el: Element, name: string, value: string): void {
  if (el.getAttribute(name) !== value) el.setAttribute(name, value);
}

export function setHidden(el: HTMLElement, hidden: boolean): void {
  if (el.hidden !== hidden) el.hidden = hidden;
}

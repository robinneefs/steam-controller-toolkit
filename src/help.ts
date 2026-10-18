// Help tooltips: hover or focus any element with `data-help="<key>"` to show that key's text
// from a registered table. One floating box on <body>, so scrolling tiles never clip it.

const texts = new Map<string, string>();
const box = document.createElement("div");
box.className = "help-tip";
box.setAttribute("role", "tooltip");
box.hidden = true;
document.body.appendChild(box);
let current: HTMLElement | null = null;

/** Register help texts (HTML) by key. */
export function addHelp(table: Record<string, string>): void {
  for (const [k, v] of Object.entries(table)) texts.set(k, v);
}

/** A small "?" button that shows the help text for `key`. */
export function helpIcon(key: string, label = "Help"): string {
  return `<button type="button" class="help" data-help="${key}" aria-label="${label}">?</button>`;
}

function show(el: HTMLElement): void {
  const html = texts.get(el.dataset.help!);
  if (!html) return;
  current = el;
  box.innerHTML = html;
  box.hidden = false;
  // Below the icon if it fits, else above; kept inside the window horizontally.
  const r = el.getBoundingClientRect();
  const b = box.getBoundingClientRect();
  const left = Math.min(Math.max(8, r.left + r.width / 2 - b.width / 2), innerWidth - b.width - 8);
  const below = r.bottom + 8 + b.height <= innerHeight - 8;
  box.style.left = `${left}px`;
  box.style.top = `${below ? r.bottom + 8 : Math.max(8, r.top - 8 - b.height)}px`;
}

function hide(): void {
  current = null;
  box.hidden = true;
}

const target = (e: Event) => (e.target as Element | null)?.closest?.<HTMLElement>("[data-help]") ?? null;
document.addEventListener("mouseover", (e) => {
  const el = target(e);
  if (el && el !== current) show(el);
  else if (!el && current && !box.contains(e.target as Node)) hide();
});
document.addEventListener("focusin", (e) => {
  const el = target(e);
  if (el) show(el);
});
document.addEventListener("focusout", (e) => {
  if (target(e) === current) hide();
});
// Clicking an icon keeps its tip open (handy on touch screens); any other click closes it.
document.addEventListener("click", (e) => {
  const el = target(e);
  if (el) show(el);
  else hide();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") hide();
});
document.addEventListener("scroll", hide, true);

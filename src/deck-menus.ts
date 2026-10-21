// Draws Command Deck menus: a stick's wheel (SVG) and a trackpad's grid. The Command Deck tab
// edits them in this form and the on-screen menu (overlay.ts) shows them the same way.
import type { DeckItem, DeckMenu } from "./controller";
import { actionLabel, escape } from "./tabs/actions";

/** What an item shows: its name, else its action's. */
export function itemLabel(item: DeckItem | undefined): string {
  if (!item) return "";
  if (item.label) return item.label;
  return item.action.kind === "none" ? "" : actionLabel(item.action).replace(" ⟳", "");
}

const R_OUT = 116;
const R_IN = 40;
/** Degrees left between slices. */
const GAP = 1.6;

/** A point `r` from the centre, `deg` clockwise from the top. */
function at(r: number, deg: number): string {
  const a = (deg * Math.PI) / 180;
  return `${(r * Math.sin(a)).toFixed(2)} ${(-r * Math.cos(a)).toFixed(2)}`;
}

function short(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** A wheel; slices carry `data-item="<key>:<i>"` and their labels `data-label-of` the same. */
export function wheelSvg(items: DeckItem[], key: string, centre: string): string {
  const n = Math.max(items.length, 1);
  const span = 360 / n;
  const slices = items.map((item, i) => {
    const [a0, a1] = [i * span - span / 2 + GAP / 2, i * span + span / 2 - GAP / 2];
    const large = a1 - a0 > 180 ? 1 : 0;
    const d = `M ${at(R_OUT, a0)} A ${R_OUT} ${R_OUT} 0 ${large} 1 ${at(R_OUT, a1)} L ${at(R_IN, a1)} A ${R_IN} ${R_IN} 0 ${large} 0 ${at(R_IN, a0)} Z`;
    const label = itemLabel(item);
    const [x, y] = at((R_IN + R_OUT) / 2, i * span).split(" ");
    const max = n <= 4 ? 14 : n <= 6 ? 11 : 9;
    return `<g class="slice${label ? "" : " empty"}" data-i="${i}">
      <path d="${d}" data-item="${key}:${i}"><title>${escape(label || "Nothing")}</title></path>
      <text x="${x}" y="${y}" data-label-of="${key}:${i}">${escape(short(label || "–", max))}</text></g>`;
  });
  return `<svg class="deck-wheel" viewBox="-120 -120 240 240" role="img">
    ${slices.join("")}
    <circle class="hub" r="${R_IN - 5}" /><text class="hub-text" y="1">${escape(centre)}</text></svg>`;
}

/** A grid; cells carry `data-item="<key>:<i>"`. `tag`: button in the editor, div on screen. */
export function gridHtml(cols: number, rows: number, items: DeckItem[], key: string, tag: "button" | "div"): string {
  const cells = Array.from({ length: cols * rows }, (_, i) => {
    const label = itemLabel(items[i]);
    return `<${tag} class="cell${label ? "" : " empty"}" data-i="${i}" data-item="${key}:${i}" title="${escape(label || "Nothing")}"><span data-label-of="${key}:${i}">${escape(short(label || "–", 18))}</span></${tag}>`;
  });
  return `<div class="deck-grid" style="grid-template-columns: repeat(${cols}, minmax(0, 1fr))">${cells.join("")}</div>`;
}

/** A menu as it looks; `centre` is the wheel's middle text. */
export function menuHtml(menu: DeckMenu, key: string, tag: "button" | "div", centre: string): string {
  switch (menu.kind) {
    case "wheel":
      return wheelSvg(menu.items, key, centre);
    case "grid":
      return gridHtml(menu.cols, menu.rows, menu.items, key, tag);
    default:
      return "";
  }
}

/** Mark the item under the thumb (`.sel` on `[data-i]`) inside `root`. */
export function markSelected(root: Element, selected: number | null): void {
  root.querySelectorAll<Element>("[data-i]").forEach((el) => el.classList.toggle("sel", Number(el.getAttribute("data-i")) === selected));
}

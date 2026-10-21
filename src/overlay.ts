// The Command Deck's on-screen menu (overlay.html, made by src-tauri/src/overlay.rs): shows the
// menu of the stick or trackpad being touched, with the item under the thumb lit.
import { listen } from "@tauri-apps/api/event";
import type { OverlayMenu } from "./controller";
import { markSelected, menuHtml } from "./deck-menus";

const el = document.querySelector<HTMLElement>("#menu")!;
let drawn = "";

function show(m: OverlayMenu | null): void {
  if (!m || m.menu.kind === "none") {
    el.hidden = true;
    drawn = "";
    return;
  }
  // Redraw only when the menu itself changed; moving between items only moves the highlight.
  const key = JSON.stringify(m.menu);
  if (key !== drawn) {
    const stick = m.part.endsWith("stick");
    const centre = m.menu.pick === "click" ? "Click" : stick || m.menu.kind === "wheel" ? "Let go" : "Lift";
    el.innerHTML = menuHtml(m.menu, m.part, "div", centre);
    el.className = m.menu.kind;
    drawn = key;
  }
  markSelected(el, m.selected);
  el.hidden = false;
}

listen<OverlayMenu | null>("deck-menu", (e) => show(e.payload));

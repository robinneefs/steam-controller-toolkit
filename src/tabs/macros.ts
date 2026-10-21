import type { Tab } from "./tab";
import { ICONS } from "./icons";

/** Recorded keyboard and controller input to play back, from the Command Deck among others. Not built yet. */
export class MacrosTab implements Tab {
  id = "macros";
  title = "Macros";
  group = "tools" as const;
  icon = ICONS.macros;

  mount(root: HTMLElement): void {
    root.innerHTML = `
      <section class="tile macros-empty">
        <div class="macros-icon">${ICONS.macros}</div>
        <h3>Macros are coming</h3>
        <p>Record what you do on the keyboard and the controller, then play it back with one press: from a button,
          a wheel or a grid on the <b>Command Deck</b>.</p>
      </section>`;
  }
}

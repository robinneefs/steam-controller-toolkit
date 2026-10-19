import { invoke } from "@tauri-apps/api/core";
import type { Snapshot } from "../controller";
import type { Tab } from "./tab";
import { ICONS } from "./icons";

/** Mirrors `BackgroundSettings` in src-tauri/src/tray.rs. */
interface BackgroundSettings {
  close_to_tray: boolean;
  autostart: boolean;
}

/** App settings: running in the background (tray, start with the system). */
export class SettingsTab implements Tab {
  id = "settings";
  title = "Settings";
  group = "app" as const;
  icon = ICONS.settings;
  private root!: HTMLElement;

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="settings">
        <section class="tile">
          <h3>Running in the background</h3>
          <label class="setting">
            <input type="checkbox" data-s="close_to_tray" />
            <span><b>Keep running in the tray when the window is closed</b>
              <span class="muted">MIDI and OSC keep sending with the window closed. Click the tray icon to open it
              again, or right-click it to quit.</span></span>
          </label>
          <label class="setting">
            <input type="checkbox" data-s="autostart" />
            <span><b>Start when you sign in</b>
              <span class="muted">Starts in the tray, without opening the window.</span></span>
          </label>
          <p class="setting-error warn-text" hidden></p>
        </section>
        <section class="tile">
          <h3>What runs in the background</h3>
          <ul class="bg-list" data-bg></ul>
          <p class="muted">Outputs that are off do no work unless their tab is open. A minimised window draws
          nothing. Closing it to the tray also frees the memory the window uses, most of the app's.</p>
        </section>
      </div>`;
    root.querySelectorAll<HTMLInputElement>("[data-s]").forEach((input) =>
      input.addEventListener("change", () => this.set(input)),
    );
    this.load();
  }

  private async load(): Promise<void> {
    try {
      const s = await invoke<BackgroundSettings>("background_settings");
      this.input("close_to_tray").checked = s.close_to_tray;
      this.input("autostart").checked = s.autostart;
    } catch (e) {
      this.error(String(e));
    }
  }

  private async set(input: HTMLInputElement): Promise<void> {
    const on = input.checked;
    this.error("");
    try {
      await invoke(input.dataset.s === "autostart" ? "set_autostart" : "set_close_to_tray", { on });
    } catch (e) {
      input.checked = !on;
      this.error(`Couldn't change that: ${e}`);
    }
  }

  update(snap: Snapshot): void {
    const midi = snap.midi.output.kind === "off" ? "off" : snap.midi.connected ? `sending to ${snap.midi.connected}` : "not connected";
    const osc = (snap.osc?.destinations ?? []).filter((d) => d.address).map((d) => d.address!);
    const lines = [
      `<b>MIDI</b> ${midi}`,
      `<b>OSC</b> ${osc.length ? `sending to ${osc.join(", ")}` : "off"}`,
      `<b>Recorder</b> ${snap.tape.recording ? "recording" : "idle"}`,
    ];
    const html = lines.map((l) => `<li>${l}</li>`).join("");
    const list = this.root.querySelector<HTMLElement>("[data-bg]")!;
    if (list.innerHTML !== html) list.innerHTML = html;
  }

  private input(name: string): HTMLInputElement {
    return this.root.querySelector<HTMLInputElement>(`[data-s="${name}"]`)!;
  }

  private error(text: string): void {
    const el = this.root.querySelector<HTMLElement>(".setting-error")!;
    el.textContent = text;
    el.hidden = !text;
  }
}

import { invoke } from "@tauri-apps/api/core";
import { hex, type Haptic, type Snapshot } from "../controller";
import type { Tab } from "./tab";

/** Values of the `side` field: a bitmask, 1 left, 2 right (0 also plays on the left). */
const SIDES: [string, number][] = [["Left", 1], ["Right", 2], ["Both", 3]];

/** Built-in effects of the haptic command report (`haptic_type_t` in SDL's controller_structs.h). */
const EFFECTS: [string, number][] = [["Tick", 1], ["Click", 2], ["Tone", 3], ["Rumble", 4], ["Noise", 5]];

/** Two octaves of keys for the tone piano, from C4. */
const NOTES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const PIANO = Array.from({ length: 24 }, (_, i) => ({
  name: `${NOTES[i % 12]}${4 + Math.floor(i / 12)}`,
  hz: Math.round(440 * 2 ** ((i - 9) / 12)),
  black: NOTES[i % 12].endsWith("#"),
}));

interface Slider {
  key: string;
  label: string;
  min: number;
  max: number;
  step?: number;
  value: number;
  unit: string;
}

const slider = (s: Slider) => `
  <label class="hslider">
    <span>${s.label}</span>
    <input type="range" data-s="${s.key}" min="${s.min}" max="${s.max}" step="${s.step ?? 1}" value="${s.value}" />
    <b class="num" data-v="${s.key}"></b><small>${s.unit}</small>
  </label>`;

/** Play every haptic output report the controller understands. */
export class HapticsTab implements Tab {
  id = "haptics";
  title = "Haptics";
  private root!: HTMLElement;
  private slot: string | null = null;
  private side = SIDES[2][1];
  private rumbling = false;
  /** A rumble update waiting for the previous one to finish, so dragging never queues up. */
  private rumblePending = false;
  private rumbleBusy = false;

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="haptics">
        <section class="tile wide haptics-bar">
          <span class="bar-group"><span class="muted">Actuator</span>
            <span class="segmented" data-sides>${SIDES.map(([l, v]) => `<button data-side="${v}">${l}</button>`).join("")}</span>
          </span>
          ${slider({ key: "gain", label: "Gain", min: -24, max: 6, value: 0, unit: "dB" })}
          <span class="last-sent"><span class="muted">Sent</span> <code data-sent data-copy>–</code></span>
        </section>

        <section class="tile">
          <h3>Rumble <button class="mini" data-stop>Stop</button></h3>
          <p class="hint">Plays while a motor is above 0. Leaving the tab stops it.</p>
          ${slider({ key: "rumbleL", label: "Left", min: 0, max: 100, value: 0, unit: "%" })}
          ${slider({ key: "rumbleR", label: "Right", min: 0, max: 100, value: 0, unit: "%" })}
        </section>

        <section class="tile">
          <h3>Effects</h3>
          <p class="hint">Built-in effects of the firmware, at the gain above.</p>
          <div class="effect-grid">
            ${EFFECTS.map(([l, v]) => `<button class="gbtn" data-effect="${v}">${l}</button>`).join("")}
          </div>
          <div class="script-row">
            ${slider({ key: "script", label: "Script", min: 0, max: 31, value: 1, unit: "" })}
            <button class="gbtn" data-play="script">Play</button>
          </div>
        </section>

        <section class="tile wide">
          <h3>Tone <button class="mini" data-play="tone">Play</button></h3>
          <div class="cols">
            ${slider({ key: "freq", label: "Frequency", min: 20, max: 1000, value: 200, unit: "Hz" })}
            ${slider({ key: "toneMs", label: "Duration", min: 20, max: 2000, step: 10, value: 300, unit: "ms" })}
            ${slider({ key: "lfo", label: "LFO", min: 0, max: 50, value: 0, unit: "Hz" })}
            ${slider({ key: "depth", label: "LFO depth", min: 0, max: 255, value: 0, unit: "" })}
          </div>
          <div class="piano" title="Each key plays a tone at the duration and LFO above">
            ${PIANO.map((k) => `<button class="${k.black ? "black" : "white"}" data-hz="${k.hz}" title="${k.name} · ${k.hz} Hz">${k.black ? "" : `<span>${k.name}</span>`}</button>`).join("")}
          </div>
        </section>

        <section class="tile">
          <h3>Pulse train <button class="mini" data-play="pulse">Play</button></h3>
          ${slider({ key: "onUs", label: "On", min: 100, max: 20000, step: 100, value: 2000, unit: "µs" })}
          ${slider({ key: "offUs", label: "Off", min: 0, max: 50000, step: 100, value: 8000, unit: "µs" })}
          ${slider({ key: "repeat", label: "Repeats", min: 0, max: 200, value: 30, unit: "" })}
        </section>

        <section class="tile">
          <h3>Sweep <button class="mini" data-play="sweep">Play</button></h3>
          ${slider({ key: "from", label: "From", min: 20, max: 1000, value: 40, unit: "Hz" })}
          ${slider({ key: "to", label: "To", min: 20, max: 1000, value: 400, unit: "Hz" })}
          ${slider({ key: "sweepMs", label: "Duration", min: 50, max: 3000, step: 10, value: 800, unit: "ms" })}
        </section>
      </div>`;

    root.querySelectorAll<HTMLInputElement>("input[data-s]").forEach((input) => {
      const show = () => (root.querySelector(`[data-v="${input.dataset.s}"]`)!.textContent = input.value);
      show();
      input.addEventListener("input", () => {
        show();
        if (input.dataset.s === "rumbleL" || input.dataset.s === "rumbleR") this.sendRumble();
      });
    });
    root.querySelector("[data-sides]")!.addEventListener("click", (e) => {
      const b = (e.target as Element).closest<HTMLElement>("[data-side]");
      if (b) this.setSide(Number(b.dataset.side));
    });
    this.setSide(this.side);
    root.querySelector("[data-stop]")!.addEventListener("click", () => this.stopRumble());
    root.querySelectorAll<HTMLElement>("[data-effect]").forEach((b) =>
      b.addEventListener("click", () =>
        this.play({ kind: "command", side: this.side, command: Number(b.dataset.effect), gain_db: this.num("gain") }),
      ),
    );
    root.querySelectorAll<HTMLElement>("[data-play]").forEach((b) =>
      b.addEventListener("click", () => this.play(this.effect(b.dataset.play!))),
    );
    root.querySelectorAll<HTMLElement>("[data-hz]").forEach((b) =>
      b.addEventListener("pointerdown", () => this.play(this.tone(Number(b.dataset.hz)))),
    );
  }

  update(snap: Snapshot): void {
    const connected = snap.state !== null;
    if (snap.slot !== this.slot || !connected) this.rumbling = false;
    this.slot = connected ? snap.slot : null;
    this.root.querySelector(".haptics")!.classList.toggle("disconnected", !connected);
  }

  hide(): void {
    this.stopRumble();
  }

  private num(key: string): number {
    return Number(this.root.querySelector<HTMLInputElement>(`[data-s="${key}"]`)!.value);
  }

  private setSide(side: number): void {
    this.side = side;
    this.root.querySelectorAll<HTMLElement>("[data-side]").forEach((b) => b.classList.toggle("on", Number(b.dataset.side) === side));
  }

  private tone(frequency: number): Haptic {
    return {
      kind: "tone", side: this.side, gain_db: this.num("gain"), frequency,
      duration_ms: this.num("toneMs"), lfo_freq: this.num("lfo"), lfo_depth: this.num("depth"),
    };
  }

  private effect(name: string): Haptic {
    const side = this.side;
    const gain_db = this.num("gain");
    switch (name) {
      case "tone":
        return this.tone(this.num("freq"));
      case "pulse":
        return { kind: "pulse", side, on_us: this.num("onUs"), off_us: this.num("offUs"), repeat: this.num("repeat") };
      case "sweep":
        return { kind: "sweep", side, gain_db, duration_ms: this.num("sweepMs"), start_hz: this.num("from"), end_hz: this.num("to") };
      default:
        return { kind: "script", side, script_id: this.num("script"), gain_db };
    }
  }

  private async play(h: Haptic): Promise<boolean> {
    const sent = this.root.querySelector<HTMLElement>("[data-sent]")!;
    if (!this.slot) {
      sent.textContent = "No controller connected";
      sent.dataset.copy = "";
      return false;
    }
    try {
      const bytes = await invoke<number[]>("play_haptic", { slot: this.slot, haptic: h });
      sent.textContent = sent.dataset.copy = hex(bytes);
      sent.classList.remove("error");
      return true;
    } catch (e) {
      sent.textContent = String(e);
      sent.dataset.copy = "";
      sent.classList.add("error");
      return false;
    }
  }

  private async sendRumble(): Promise<void> {
    if (this.rumbleBusy) {
      this.rumblePending = true;
      return;
    }
    this.rumbleBusy = true;
    const speed = (key: string) => Math.round((this.num(key) / 100) * 65535);
    const h: Haptic = { kind: "rumble", left: speed("rumbleL"), right: speed("rumbleR") };
    if (await this.play(h)) this.rumbling = h.left > 0 || h.right > 0;
    this.rumbleBusy = false;
    if (this.rumblePending) {
      this.rumblePending = false;
      this.sendRumble();
    }
  }

  private stopRumble(): void {
    for (const key of ["rumbleL", "rumbleR"]) {
      const input = this.root.querySelector<HTMLInputElement>(`[data-s="${key}"]`)!;
      input.value = "0";
      this.root.querySelector(`[data-v="${key}"]`)!.textContent = "0";
    }
    if (this.rumbling) this.sendRumble();
  }
}

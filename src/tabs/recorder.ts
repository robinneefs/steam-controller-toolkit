import { invoke } from "@tauri-apps/api/core";
import type { RecordingInfo, Snapshot, TapeStatus } from "../controller";
import { controlPlayback, formatTime, playRecording, startRecording, stopRecording } from "../tape";
import type { Tab } from "./tab";
import { ICONS as NAV_ICONS } from "./icons";

const ICONS = {
  play: `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5z"/></svg>`,
  pause: `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4.5" width="4" height="15" rx="1"/><rect x="14" y="4.5" width="4" height="15" rx="1"/></svg>`,
  step: `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M5 4.5v15l11-7.5z"/><rect x="16" y="4.5" width="3" height="15" rx="1"/></svg>`,
  stop: `<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="5" y="5" width="14" height="14" rx="2"/></svg>`,
  loop: `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 2l4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="M7 22l-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/></svg>`,
};

/** Seek bar resolution. */
const SEEK_STEPS = 1000;

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

const escape = (s: string) => s.replace(/[&<>"]/g, (c) => `&${{ "&": "amp", "<": "lt", ">": "gt", '"': "quot" }[c]};`);

/** Record live input to files and replay them in place of the controller, in every tab. */
export class RecorderTab implements Tab {
  id = "recorder";
  title = "Recorder";
  group = "tools" as const;
  icon = NAV_ICONS.recorder;
  private root!: HTMLElement;
  private visible = false;
  private tape: TapeStatus = { recording: null, playback: null, message: null };
  private connected = false;
  private recordings: RecordingInfo[] = [];
  private seeking = false;
  private lastTapeMessage: string | null = null;
  /** The recording whose Delete button was clicked once, waiting for the confirming click. */
  private armedDelete: string | null = null;
  private armTimer: number | undefined;

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="recorder">
        <section class="tile rec-panel">
          <h3>Record <button class="mini" data-folder title="Open the recordings folder in the file manager">Open folder</button></h3>
          <div class="rec-row">
            <button class="rec-big" data-record><i></i><span data-rec-label>Record</span></button>
            <div class="rec-meta"><b class="num" data-rec-time>0:00</b><small class="muted" data-rec-info></small></div>
          </div>
          <p class="hint">Saves every report the controller sends, up to 10 minutes. Play a recording back to
            see it in every tab without a controller; pause and step through it in Raw data.
            Drop <code>.screc</code> files on the window to add them.</p>
          <p class="rec-message" data-message hidden></p>
        </section>

        <section class="tile player">
          <h3>Playback <em data-play-name></em></h3>
          <div class="transport">
            <button class="tbtn main" data-act="toggle" title="Play / pause">${ICONS.play}</button>
            <button class="tbtn" data-act="step" title="Pause on the next state report">${ICONS.step}</button>
            <button class="tbtn" data-act="stop" title="Stop and return to live input">${ICONS.stop}</button>
            <input type="range" class="seek" data-seek min="0" max="${SEEK_STEPS}" value="0" />
            <span class="num play-time" data-play-time>0:00.0 / 0:00.0</span>
            <button class="tbtn" data-act="loop" title="Loop">${ICONS.loop}</button>
          </div>
          <p class="hint" data-play-hint>Pick a recording below to play it. While it plays, every tab shows it instead of the controller.</p>
        </section>

        <section class="tile rec-list">
          <h3>Recordings <em data-count></em></h3>
          <div class="rec-scroll"><table class="rec-table"><tbody data-rows></tbody></table></div>
        </section>
      </div>`;

    this.q("[data-record]").addEventListener("click", () => this.toggleRecording());
    this.q("[data-folder]").addEventListener("click", () => this.run(invoke("reveal_recording", { name: null })));
    root.querySelectorAll<HTMLElement>("[data-act]").forEach((b) =>
      b.addEventListener("click", () => this.transport(b.dataset.act!)),
    );
    const seek = this.q<HTMLInputElement>("[data-seek]");
    seek.addEventListener("pointerdown", () => (this.seeking = true));
    seek.addEventListener("pointerup", () => (this.seeking = false));
    seek.addEventListener("change", () => (this.seeking = false));
    seek.addEventListener("input", () => {
      const p = this.tape.playback;
      if (p) this.run(controlPlayback({ kind: "seek", seconds: (Number(seek.value) / SEEK_STEPS) * p.duration }));
    });
    this.q("[data-rows]").addEventListener("click", (e) => {
      const b = (e.target as Element).closest<HTMLElement>("[data-row-act]");
      const name = b?.closest<HTMLElement>("[data-name]")?.dataset.name;
      if (b && name !== undefined) this.rowAction(b.dataset.rowAct!, name);
    });
    this.renderList();
  }

  update(snap: Snapshot): void {
    if (!this.visible) {
      this.visible = true;
      this.refresh();
    }
    // A recording that just finished belongs in the list.
    if (this.tape.recording && !snap.tape.recording) this.refresh();
    const playingChanged = this.tape.playback?.name !== snap.tape.playback?.name;
    if (snap.tape.message !== this.lastTapeMessage) {
      this.lastTapeMessage = snap.tape.message;
      if (snap.tape.message) this.showMessage(snap.tape.message);
    }
    this.tape = snap.tape;
    this.connected = snap.state !== null && !snap.tape.playback;
    this.render();
    if (playingChanged) this.renderList();
  }

  hide(): void {
    this.visible = false;
  }

  /** Reload the list of recordings, e.g. after one was dropped on the window. */
  async refresh(): Promise<void> {
    try {
      this.recordings = await invoke<RecordingInfo[]>("list_recordings");
    } catch (e) {
      this.showMessage(String(e));
    }
    this.renderList();
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector<T>(sel)!;
  }

  private async run(p: Promise<unknown>): Promise<void> {
    try {
      await p;
    } catch (e) {
      this.showMessage(String(e));
    }
  }

  private showMessage(text: string | null): void {
    const el = this.q("[data-message]");
    el.hidden = !text;
    el.textContent = text ?? "";
  }

  private async toggleRecording(): Promise<void> {
    this.showMessage(null);
    await this.run(this.tape.recording ? stopRecording() : startRecording());
  }

  private transport(act: string): void {
    const p = this.tape.playback;
    if (!p) {
      // Play starts the newest recording when nothing is loaded.
      if (act === "toggle" && this.recordings[0]) this.run(playRecording(this.recordings[0].name));
      return;
    }
    if (act === "toggle") this.run(controlPlayback({ kind: p.paused ? "resume" : "pause" }));
    else if (act === "loop") this.run(controlPlayback({ kind: "loop", on: !p.looping }));
    else this.run(controlPlayback({ kind: act as "step" | "stop" }));
  }

  private rowAction(act: string, name: string): void {
    if (act === "play") this.run(playRecording(name));
    else if (act === "show") this.run(invoke("reveal_recording", { name }));
    else if (act === "delete") {
      clearTimeout(this.armTimer);
      if (this.armedDelete !== name) {
        this.armedDelete = name;
        this.armTimer = window.setTimeout(() => {
          this.armedDelete = null;
          this.renderList();
        }, 3000);
      } else {
        this.armedDelete = null;
        this.run(invoke("delete_recording", { name })).then(() => this.refresh());
      }
      this.renderList();
    }
  }

  private render(): void {
    const { recording: r, playback: p } = this.tape;
    const rec = this.q<HTMLButtonElement>("[data-record]");
    rec.classList.toggle("on", !!r);
    rec.disabled = !r && !this.connected;
    rec.title = r ? "Stop recording" : p ? "Stop the playback to record" : this.connected ? "Record live input" : "Connect a controller to record";
    this.q("[data-rec-label]").textContent = r ? "Stop" : "Record";
    this.q("[data-rec-time]").textContent = formatTime(r?.seconds ?? 0);
    this.q("[data-rec-info]").textContent = r ? `${r.packets.toLocaleString()} reports · ${r.name}` : "";

    this.root.querySelector(".player")!.classList.toggle("idle", !p);
    this.q("[data-play-name]").textContent = p?.name ?? "";
    const hint = p
      ? "Every tab shows this recording instead of the controller. Pause or step to inspect single reports in Raw data; stop to return to live input."
      : "Pick a recording below to play it. While it plays, every tab shows it instead of the controller.";
    const hintEl = this.q("[data-play-hint]");
    if (hintEl.textContent !== hint) hintEl.textContent = hint;
    const toggle = this.q("[data-act=toggle]");
    const icon = p && !p.paused ? ICONS.pause : ICONS.play;
    if (toggle.innerHTML !== icon) toggle.innerHTML = icon;
    this.q("[data-act=loop]").classList.toggle("on", !!p?.looping);
    this.q("[data-play-time]").textContent = `${formatTime(p?.position ?? 0, true)} / ${formatTime(p?.duration ?? 0, true)}`;
    const seek = this.q<HTMLInputElement>("[data-seek]");
    seek.disabled = !p;
    if (!this.seeking) seek.value = String(p && p.duration > 0 ? Math.round((p.position / p.duration) * SEEK_STEPS) : 0);
  }

  private renderList(): void {
    const list = this.recordings;
    this.q("[data-count]").textContent = list.length ? String(list.length) : "";
    const playing = this.tape.playback?.name;
    const date = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
    this.q("[data-rows]").innerHTML = list.length
      ? list
          .map((r) => {
            const n = escape(r.name);
            const armed = this.armedDelete === r.name;
            return `<tr data-name="${n}" class="${r.name === playing ? "playing" : ""}">
              <td class="rec-name" title="${n}">${r.name === playing ? `<span class="now">▶</span>` : ""}${n}</td>
              <td class="num">${formatTime(r.seconds, true)}</td>
              <td class="num muted">${size(r.bytes)}</td>
              <td class="muted">${date.format(r.modified)}</td>
              <td class="rec-actions">
                <button class="mini" data-row-act="play">Play</button>
                <button class="mini" data-row-act="show" title="Show the file in the file manager">Show</button>
                <button class="mini danger${armed ? " armed" : ""}" data-row-act="delete" title="${armed ? "Click again to delete" : "Delete the recording"}">${armed ? "Delete?" : "Delete"}</button>
              </td>
            </tr>`;
          })
          .join("")
      : `<tr><td class="empty muted">No recordings yet. Connect a controller and press Record.</td></tr>`;
  }
}

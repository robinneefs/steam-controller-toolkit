// Recording and playback commands, shared by the top bar (main.ts) and the Recorder tab.
import { invoke } from "@tauri-apps/api/core";
import type { PlaybackAction } from "./controller";

/** 75.3 -> "1:15", or "1:15.3" with `tenths`. */
export function formatTime(seconds: number, tenths = false): string {
  // Round down, like a clock, so both forms agree on the second.
  const s = Math.floor(Math.max(0, seconds) * 10) / 10;
  const m = Math.floor(s / 60);
  const rest = tenths ? (s % 60).toFixed(1).padStart(4, "0") : String(Math.floor(s % 60)).padStart(2, "0");
  return `${m}:${rest}`;
}

/** Start a recording named after the local date and time; returns the name used. */
export function startRecording(): Promise<string> {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const name = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`;
  return invoke<string>("start_recording", { name });
}

export function stopRecording(): Promise<void> {
  return invoke("stop_recording");
}

export function playRecording(name: string): Promise<void> {
  return invoke("play_recording", { name });
}

export function controlPlayback(action: PlaybackAction): Promise<void> {
  return invoke("control_playback", { action });
}

/** Copy recording files (from a drop on the window) into the recordings folder. */
export function importRecordings(paths: string[]): Promise<string[]> {
  return invoke<string[]>("import_recordings", { paths });
}

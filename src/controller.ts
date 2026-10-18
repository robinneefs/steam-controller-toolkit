// Mirrors the Rust types in crates/sc-core/src/protocol.rs and the Snapshot in src-tauri/src/lib.rs.
import { listen } from "@tauri-apps/api/event";

export interface Trackpad {
  touched: boolean;
  clicked: boolean;
  /** Raw signed position, about -32768..32767; +y is up. */
  x: number;
  y: number;
  pressure: number;
}

export interface Imu {
  /** Raw counts; ±2 g full scale (16384 = 1 g). */
  accel: [number, number, number];
  /** Raw counts; ±2000 °/s full scale. */
  gyro: [number, number, number];
  quat: [number, number, number, number] | null;
  timestamp: number;
}

export interface ControllerState {
  report_id: number;
  seq: number;
  buttons: number;
  /** 0..32767 */
  trigger_left: number;
  trigger_right: number;
  left_stick: [number, number];
  right_stick: [number, number];
  left_pad: Trackpad;
  right_pad: Trackpad;
  imu: Imu;
}

export interface Snapshot {
  slot: string | null;
  state: ControllerState | null;
  battery: { charge_state: number; level: number } | null;
  report_rate: number;
  /** Bytes of the latest state report, report id first. */
  raw: number[];
  /** Report ids the parser does not decode yet, with their latest bytes. */
  unknown_reports: { id: number; count: number; data: number[] }[];
  /** Steam is running; its Desktop Layout may turn controller input into mouse/keyboard input. */
  steam_running: boolean;
  /** Recorder and player state. While a recording plays, the rest of the snapshot is replayed. */
  tape: TapeStatus;
  /** MIDI output state; MIDI follows whatever this snapshot shows (live input or a replay). */
  midi: MidiStatus;
}

/** Mirrors `OutputChoice` in src-tauri/src/midi_out.rs. */
export type MidiOutputChoice = { kind: "off" } | { kind: "virtual" } | { kind: "port"; name: string };

/** Mirrors `MidiStatus` in src-tauri/src/midi_out.rs. */
export interface MidiStatus {
  output: MidiOutputChoice;
  connected: string | null;
  error: string | null;
  /** Messages sent over the last second. */
  rate: number;
  /** Output level 0..1 of each mapping, in preset order. */
  levels: (number | null)[];
  /** Cell under the finger on the left and right pad grids, and whether it plays a note. */
  cells: [[number, boolean] | null, [number, boolean] | null];
  /** Latest messages, oldest first, as [number, bytes]. */
  log: [number, number[]][];
  learning: boolean;
  learned: string | null;
}

/** The types below mirror src-tauri/src/midi.rs. */
export type MidiTarget =
  | { kind: "cc"; cc: number; hi_res: boolean }
  | { kind: "pitch_bend" }
  | { kind: "channel_pressure" }
  | { kind: "note"; note: number; velocity: number }
  | { kind: "program"; program: number };

export interface MidiMapping {
  enabled: boolean;
  /** An axis name (see `SOURCES` in tabs/midi.ts) or `button:<name>`. */
  source: string;
  target: MidiTarget;
  channel: number;
  range: [number, number];
  invert: boolean;
  deadzone: number;
  curve: number;
  smoothing: number;
  toggle: boolean;
  hold: string | null;
}

export type MidiScale =
  | "chromatic" | "major" | "minor" | "major_pentatonic" | "minor_pentatonic" | "blues" | "dorian" | "harmonic_minor";

export interface PadGrid {
  enabled: boolean;
  channel: number;
  cols: number;
  rows: number;
  root: number;
  scale: MidiScale;
  row_step: number;
  trigger: "touch" | "click";
  velocity: number;
  dynamic_velocity: boolean;
  slide: boolean;
  pressure: "off" | "poly" | "channel";
}

export interface MidiPreset {
  name: string;
  mappings: MidiMapping[];
  left_pad: PadGrid;
  right_pad: PadGrid;
}

/** Mirrors `TapeStatus` in src-tauri/src/tape.rs. */
export interface TapeStatus {
  recording: { name: string; seconds: number; packets: number } | null;
  playback: { name: string; position: number; duration: number; paused: boolean; looping: boolean } | null;
  /** Why the last recording stopped on its own, if it did. */
  message: string | null;
}

/** A file in the recordings folder (`list_recordings`). */
export interface RecordingInfo {
  name: string;
  bytes: number;
  seconds: number;
  /** Last modified, ms since the Unix epoch. */
  modified: number;
}

/** Player controls for `control_playback`; mirrors `PlaybackAction` in tape.rs. */
export type PlaybackAction =
  | { kind: "pause" | "resume" | "stop" | "step" }
  | { kind: "seek"; seconds: number }
  | { kind: "loop"; on: boolean };

/** EChargeState from SDL's controller_structs.h. */
export const CHARGE_STATES = ["reset", "on battery", "charging", "validating source", "fully charged"];

/** Connection types by USB product id (see protocol.rs). */
export const PRODUCTS: Record<number, { name: string; short: string; dongle: boolean }> = {
  0x1302: { name: "Steam Controller (USB)", short: "USB", dongle: false },
  0x1303: { name: "Steam Controller (Bluetooth)", short: "Bluetooth", dongle: false },
  0x1304: { name: "Steam Controller Puck (dongle)", short: "Puck", dongle: true },
  0x1305: { name: "Steam Controller dongle (newer)", short: "Dongle", dongle: true },
};

/** Readable form of a hub slot id such as "1304:2" (product id : HID interface). */
export function describeSlot(slot: string): { pid: number; iface: number; product: string; short: string; label: string } {
  const [p, i] = slot.split(":");
  const pid = parseInt(p, 16);
  const iface = Number(i);
  const known = PRODUCTS[pid];
  // Dongle interfaces 2..=5 are its four controller slots.
  const label = known
    ? known.dongle && iface >= 2 ? `${known.short} · slot ${iface - 1}` : known.short
    : slot;
  return { pid, iface, product: known?.name ?? `Unknown product 0x${p}`, short: known?.short ?? slot, label };
}

export const REPORT_NAMES: Record<number, string> = {
  0x42: "state + orientation",
  0x45: "state",
  0x47: "state + pad timestamp",
};

/** A haptic effect for the `play_haptic` command; mirrors `Haptic` in protocol.rs. */
export type Haptic =
  | { kind: "rumble"; left: number; right: number }
  | { kind: "pulse"; side: number; on_us: number; off_us: number; repeat: number }
  | { kind: "command"; side: number; command: number; gain_db: number }
  | { kind: "tone"; side: number; gain_db: number; frequency: number; duration_ms: number; lfo_freq: number; lfo_depth: number }
  | { kind: "sweep"; side: number; gain_db: number; duration_ms: number; start_hz: number; end_hz: number }
  | { kind: "script"; side: number; script_id: number; gain_db: number };

export function hex(bytes: number[]): string {
  return bytes.map((b) => b.toString(16).padStart(2, "0")).join(" ");
}

export const Buttons = {
  A: 0x00000001, B: 0x00000002, X: 0x00000004, Y: 0x00000008,
  QAM: 0x00000010, R3: 0x00000020, View: 0x00000040, R4: 0x00000080,
  R5: 0x00000100, R: 0x00000200, DpadDown: 0x00000400, DpadRight: 0x00000800,
  DpadLeft: 0x00001000, DpadUp: 0x00002000, Menu: 0x00004000, L3: 0x00008000,
  Steam: 0x00010000, L4: 0x00020000, L5: 0x00040000, L: 0x00080000,
  RightStickTouch: 0x00100000, RightPadTouch: 0x00200000, RightPadClick: 0x00400000,
  RightTriggerClick: 0x00800000, LeftStickTouch: 0x01000000, LeftPadTouch: 0x02000000,
  LeftPadClick: 0x04000000, LeftTriggerClick: 0x08000000,
  RightGripTouch: 0x10000000, LeftGripTouch: 0x20000000,
} as const;

export type ButtonName = keyof typeof Buttons;

export function pressed(state: ControllerState, name: ButtonName): boolean {
  return (state.buttons & Buttons[name]) !== 0;
}

/** Subscribe to controller snapshots (~60 per second). */
export function onSnapshot(cb: (snap: Snapshot) => void): Promise<() => void> {
  return listen<Snapshot>("controller", (e) => cb(e.payload));
}

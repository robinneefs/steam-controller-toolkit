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
}

/** EChargeState from SDL's controller_structs.h. */
export const CHARGE_STATES = ["reset", "on battery", "charging", "validating source", "fully charged"];

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

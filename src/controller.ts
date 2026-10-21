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
  /** OSC output state; like MIDI, it follows whatever this snapshot shows. */
  osc: OscStatus;
  /** Keyboard & mouse and Gamepad output state; they follow this snapshot too. */
  play: PlayStatus;
}

/** Mirrors `OutputChoice` in src-tauri/src/midi_out.rs. */
export type MidiOutputChoice = { kind: "off" } | { kind: "virtual" } | { kind: "port"; name: string };

/** Mirrors `MidiStatus` in src-tauri/src/midi_out.rs. */
export interface MidiStatus {
  output: MidiOutputChoice;
  connected: string | null;
  /** A port is being opened. */
  connecting: boolean;
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

/** One OSC argument; mirrors `Arg` in src-tauri/src/osc.rs. */
export type OscArg = { int: number } | { float: number } | { bool: boolean } | { string: string };

/** The types below mirror src-tauri/src/osc.rs (presets) and osc_out.rs (destinations, status). */
export interface OscGroups {
  triggers: boolean;
  sticks: boolean;
  pads: boolean;
  buttons: boolean;
  touch: boolean;
  tilt: boolean;
  gyro: boolean;
  accel: boolean;
  quat: boolean;
  status: boolean;
}

export interface OscStream {
  enabled: boolean;
  prefix: string;
  split: boolean;
  groups: OscGroups;
}

export interface OscMapping {
  enabled: boolean;
  /** An axis name (see `SOURCES` in tabs/sources.ts) or `button:<name>`. */
  source: string;
  range: [number, number];
  invert: boolean;
  deadzone: number;
  curve: number;
  smoothing: number;
  toggle: boolean;
  hold: string | null;
  address: string;
  /** Sent before the value. */
  args: OscArg[];
  value: "float" | "int" | "bool";
  out: [number, number];
  mode: "continuous" | "button" | "trigger";
}

export interface OscPreset {
  name: string;
  /** The app's default port; picking the preset fills it in. */
  port: number | null;
  stream: OscStream;
  mappings: OscMapping[];
  /** Most messages per second for axes; 265 sends every report. */
  rate: number;
  bundle: boolean;
}

export interface OscDestination {
  enabled: boolean;
  host: string;
  port: number;
  preset: OscPreset;
}

export interface OscDestinationStatus {
  /** Where packets go, once the host is known (and the destination is on). */
  address: string | null;
  resolving: boolean;
  error: string | null;
  /** Messages sent over the last second. */
  rate: number;
  levels: (number | null)[];
  /** Every address with what it last carried. */
  values: [string, OscArg[]][];
}

export interface OscStatus {
  /** The OSC tab's on/off switch; each destination has its own too. */
  enabled: boolean;
  destinations: OscDestinationStatus[];
  learning: boolean;
  learned: string | null;
}

/** The types below mirror src-tauri/src/layout.rs (layouts) and play_out.rs (config, status). */
export type MouseButton = "left" | "right" | "middle" | "back" | "forward";
export type Dir = "up" | "down" | "left" | "right";
export type PadInput =
  | "a" | "b" | "x" | "y" | "lb" | "rb" | "ls" | "rs" | "back" | "start" | "guide"
  | "up" | "down" | "left" | "right" | "lt" | "rt";

export type Action =
  | { kind: "none" }
  | { kind: "key"; keys: string[]; repeat: boolean }
  | { kind: "mouse"; button: MouseButton }
  | { kind: "scroll"; dir: Dir; repeat: boolean }
  | { kind: "pad"; input: PadInput }
  | { kind: "switch" }
  /** Open a program, file, folder or URL, once per press. */
  | { kind: "run"; target: string };

export interface Directions {
  up: Action;
  down: Action;
  left: Action;
  right: Action;
}

export type Side = "left" | "right";

export type StickMode =
  | { kind: "none" }
  | { kind: "stick"; target: Side; deadzone: number }
  | ({ kind: "directions"; threshold: number } & Directions)
  | { kind: "mouse"; speed: number; deadzone: number }
  | { kind: "scroll"; speed: number; deadzone: number };

export type PadMode =
  | { kind: "none" }
  | { kind: "mouse"; speed: number; trackball: boolean }
  | { kind: "scroll"; speed: number }
  | { kind: "stick"; target: Side }
  | ({ kind: "directions"; click: boolean } & Directions);

export type GyroOutput = "none" | "mouse" | "stick" | "steer";
export type Activation =
  | { kind: "always" }
  | { kind: "right_stick_touch" }
  | { kind: "right_pad_touch" }
  | { kind: "left_trigger" }
  | { kind: "right_trigger" }
  | { kind: "hold"; button: string }
  | { kind: "toggle"; button: string };

export interface Gyro {
  output: GyroOutput;
  activation: Activation;
  axis: "auto" | "yaw" | "roll";
  invert_y: boolean;
  /** Mouse: pixels per degree turned. */
  mouse_speed: number;
  /** Stick: turn rate (°/s) for a full stick. */
  stick_range: number;
  /** Steer: lean (°) for full lock. */
  steer_angle: number;
}

export interface Layout {
  name: string;
  /** Control name (`CONTROLS` in tabs/layout-editor.ts) to action; missing ones do nothing. */
  buttons: Record<string, Action>;
  left_stick: StickMode;
  right_stick: StickMode;
  left_pad: PadMode;
  right_pad: PadMode;
  gyro: Gyro;
}

export type PlayMode = "desktop" | "game";
/** One of the three layouts: Keyboard & mouse for Desktop and for Game, and the Gamepad's. */
export type PlaySlot = "desktop" | "game" | "pad";

export interface KeyMouseConfig {
  enabled: boolean;
  desktop: Layout;
  game: Layout;
}

// ---------- Command Deck (src-tauri/src/deck.rs, DeckConfig in play_out.rs) ----------

/** One entry of a wheel or grid. */
export interface DeckItem {
  /** Shown in the menu; empty: the action's own name. */
  label: string;
  action: Action;
}

/** Wheels: when the stick comes back to the centre; grids: when the thumb lifts. Click: on click, held. */
export type Pick = "release" | "click";

export type DeckMenu =
  | { kind: "none" }
  /** A slice per item, the first centred at the top, clockwise. */
  | { kind: "wheel"; items: DeckItem[]; pick: Pick }
  /** Trackpads: cols x rows cells, row by row from the top left. */
  | { kind: "grid"; cols: number; rows: number; items: DeckItem[]; pick: Pick };

export interface Deck {
  /** Control name (`CONTROLS` in tabs/layout-editor.ts) to action; missing ones are left to the layout. */
  buttons: Record<string, Action>;
  left_stick: DeckMenu;
  right_stick: DeckMenu;
  left_pad: DeckMenu;
  right_pad: DeckMenu;
}

export type DeckPart = "left_stick" | "right_stick" | "left_pad" | "right_pad";
export const DECK_PARTS: DeckPart[] = ["left_stick", "right_stick", "left_pad", "right_pad"];

/** A page that plays while one program is in front. */
export interface AppDeck {
  /** The program's file name (`Photoshop.exe`); any case, `.exe` optional. */
  app: string;
  deck: Deck;
}

export interface DeckConfig {
  enabled: boolean;
  /** Show a menu on screen while its stick or trackpad is touched. */
  overlay: boolean;
  /** The page for every program without one of its own. */
  deck: Deck;
  apps: AppDeck[];
}

/** What `deck_setup` returns: the config, and what a new wheel and grid hold. */
export interface DeckSetup {
  config: DeckConfig;
  wheel: DeckItem[];
  grid: DeckItem[];
}

export interface MenuView {
  open: boolean;
  /** The item under the thumb, or playing. */
  selected: number | null;
}

/** What the on-screen menu shows (`OverlayMenu` in play_out.rs). */
export interface OverlayMenu {
  part: DeckPart;
  menu: DeckMenu;
  selected: number | null;
}

export interface DeckStatus {
  enabled: boolean;
  overlay: boolean;
  supported: boolean;
  apps_supported: boolean;
  /** The page playing (null: the default page) and its program. */
  page: number | null;
  app: string | null;
  /** Programs seen in front lately, most recent first. */
  recent_apps: string[];
  run_error: string | null;
  /** The page the tab shows, live. */
  preview: { page: number | null; shown: boolean; lit: number; menus: [MenuView, MenuView, MenuView, MenuView] };
}

export interface GamepadConfig {
  enabled: boolean;
  layout: Layout;
}

/** What `play_setup` returns. */
export interface PlaySetup {
  keymouse: KeyMouseConfig;
  gamepad: GamepadConfig;
  auto_switch: boolean;
  mode: PlayMode;
}

export type PadSupport = { kind: "ready" } | { kind: "missing"; detail: string } | { kind: "unsupported" };

/** The virtual Xbox controller's state, XInput ranges. */
export interface PadState {
  buttons: number;
  lt: number;
  rt: number;
  lx: number;
  ly: number;
  rx: number;
  ry: number;
}

export interface PlayStatus {
  mode: PlayMode;
  auto_switch: boolean;
  /** The full-screen app auto switch sees, by file name. */
  fullscreen_app: string | null;
  /** The layout in play. */
  active: PlaySlot | null;
  keymouse: { enabled: boolean; supported: boolean; error: string | null };
  gamepad: {
    enabled: boolean;
    /** null while being checked. */
    support: PadSupport | null;
    connected: boolean;
    opening: boolean;
    player: number | null;
    error: string | null;
    installing: boolean;
    install_error: string | null;
    /** Large and small motor, 0..255. */
    rumble: [number, number];
  };
  /** The layout of the tab in view, live. */
  preview: {
    slot: PlaySlot | null;
    pad: PadState;
    keys: string[];
    mouse_buttons: number;
    /** px/s, x right, y down. */
    mouse: [number, number];
    /** Notches/s, x right, y up. */
    scroll: [number, number];
    gyro_active: boolean;
    /** Bit per control in use (`CONTROLS` index, then the sticks, pads and gyro). */
    lit: number;
  };
  deck: DeckStatus;
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
  QAM: 0x00000010, R3: 0x00000020, Menu: 0x00000040, R4: 0x00000080,
  R5: 0x00000100, R: 0x00000200, DpadDown: 0x00000400, DpadRight: 0x00000800,
  DpadLeft: 0x00001000, DpadUp: 0x00002000, View: 0x00004000, L3: 0x00008000,
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

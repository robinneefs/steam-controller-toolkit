// Key names as layouts store them: physical US key positions, mirrored from `KEYS` in
// src-tauri/src/layout.rs (modifiers first). Captured from `KeyboardEvent.code`, so a key binds the
// same position on any keyboard layout. Except in shortcuts (Ctrl, Alt or Win with a letter or
// digit, `byChar`): those letters are characters, so Ctrl+Z is undo on AZERTY too. Labels show
// what each position types on the user's layout.

export const KEYS = [
  "Ctrl", "Shift", "Alt", "Win", "RCtrl", "RShift", "RAlt",
  "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
  "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
  "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
  "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
  "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
  "Escape", "Tab", "CapsLock", "Space", "Enter", "Backspace", "Delete", "Insert",
  "Home", "End", "PageUp", "PageDown", "Up", "Down", "Left", "Right",
  "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon", "Quote",
  "Backquote", "Comma", "Period", "Slash",
  "Num0", "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9",
  "NumAdd", "NumSubtract", "NumMultiply", "NumDivide", "NumDecimal", "NumEnter", "NumLock",
  "PrintScreen", "ScrollLock", "Pause", "ContextMenu",
  "VolumeUp", "VolumeDown", "VolumeMute", "MediaPlayPause", "MediaNext", "MediaPrevious", "MediaStop",
  "BrowserBack", "BrowserForward",
] as const;

export const MODIFIERS = new Set(KEYS.slice(0, 7));

const LABELS: Record<string, string> = {
  Win: "Win", RCtrl: "Right Ctrl", RShift: "Right Shift", RAlt: "Right Alt",
  Escape: "Esc", CapsLock: "Caps Lock", Backspace: "Backspace", Delete: "Del", Insert: "Ins",
  PageUp: "Page Up", PageDown: "Page Down", Up: "↑", Down: "↓", Left: "←", Right: "→",
  Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\", Semicolon: ";",
  Quote: "'", Backquote: "`", Comma: ",", Period: ".", Slash: "/",
  NumAdd: "Num +", NumSubtract: "Num -", NumMultiply: "Num *", NumDivide: "Num /", NumDecimal: "Num .",
  NumEnter: "Num Enter", NumLock: "Num Lock", PrintScreen: "Print Screen", ScrollLock: "Scroll Lock",
  ContextMenu: "Menu key", VolumeUp: "Volume up", VolumeDown: "Volume down", VolumeMute: "Mute",
  MediaPlayPause: "Play/Pause", MediaNext: "Next track", MediaPrevious: "Previous track", MediaStop: "Stop",
  BrowserBack: "Browser back", BrowserForward: "Browser forward",
};

/** The key's label. `byChar`: a letter of a shortcut, which is that letter whatever the layout. */
export function keyLabel(name: string, byChar = false): string {
  if (!byChar || !isChar(name)) {
    const typed = layoutChar(name);
    if (typed) return typed;
  }
  if (LABELS[name]) return LABELS[name];
  const num = /^Num(\d)$/.exec(name);
  return num ? `Num ${num[1]}` : name;
}

/** Mirrors `by_char` in layout.rs: a combination with Ctrl, Alt or Win sends its letters and digits
 * by character. */
export function byChar(keys: string[]): boolean {
  return keys.some((k) => ["Ctrl", "Alt", "Win", "RCtrl", "RAlt"].includes(k));
}

const isChar = (name: string) => /^[A-Z0-9]$/.test(name);

/** A combination's label, its letters as `byChar` sends them. */
export function keysLabel(keys: string[]): string {
  const chars = byChar(keys);
  return keys.map((k) => keyLabel(k, chars)).join(" + ");
}

// ---------- The user's keyboard layout ----------

const PUNCTUATION = ["Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon", "Quote", "Backquote", "Comma", "Period", "Slash"];

/** The `KeyboardEvent.code` of a letter or punctuation position (digits keep their names). */
function codeOf(name: string): string | null {
  if (/^[A-Z]$/.test(name)) return `Key${name}`;
  return PUNCTUATION.includes(name) ? name : null;
}

let layoutMap: Map<string, string> | null = null;

/** What the key at this position types on the user's layout, upper case, if it differs. */
function layoutChar(name: string): string | null {
  const code = codeOf(name);
  const typed = code && layoutMap?.get(code);
  if (!typed || typed.trim() === "") return null;
  const label = typed.toUpperCase();
  return label === name || label === LABELS[name] ? null : label;
}

/** In a shortcut, the key the user pressed as the letter it types on their layout (AZERTY's Z
 * position is "W"; Ctrl+Z there should bind undo, which goes out as the letter Z). */
export function shortcutKey(name: string): string {
  const code = codeOf(name) ?? (/^\d$/.test(name) ? `Digit${name}` : null);
  const typed = code && layoutMap?.get(code)?.toUpperCase();
  return typed && isChar(typed) ? typed : name;
}

type KeyboardApi = { getLayoutMap?: () => Promise<Map<string, string>> };

/** Reads the keyboard layout (WebView2/Chromium); again on focus, as the user may switch it.
 * Fires `keylayout` on window when it changed. */
async function readLayout(): Promise<void> {
  try {
    const map = await (navigator as unknown as { keyboard?: KeyboardApi }).keyboard?.getLayoutMap?.();
    if (!map) return;
    const next = new Map(map);
    const same = layoutMap && next.size === layoutMap.size && [...next].every(([k, v]) => layoutMap!.get(k) === v);
    layoutMap = next;
    if (!same) window.dispatchEvent(new Event("keylayout"));
  } catch {
    // No layout map: labels stay US.
  }
}
void readLayout();
window.addEventListener("focus", () => void readLayout());

/** `KeyboardEvent.code` to a key name, or null for keys a layout can't send. */
const CODES: Record<string, string> = {
  ControlLeft: "Ctrl", ControlRight: "RCtrl", ShiftLeft: "Shift", ShiftRight: "RShift",
  AltLeft: "Alt", AltRight: "RAlt", MetaLeft: "Win", MetaRight: "Win", OSLeft: "Win", OSRight: "Win",
  ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
  NumpadAdd: "NumAdd", NumpadSubtract: "NumSubtract", NumpadMultiply: "NumMultiply", NumpadDivide: "NumDivide",
  NumpadDecimal: "NumDecimal", NumpadEnter: "NumEnter",
  AudioVolumeUp: "VolumeUp", AudioVolumeDown: "VolumeDown", AudioVolumeMute: "VolumeMute",
  VolumeUp: "VolumeUp", VolumeDown: "VolumeDown", VolumeMute: "VolumeMute",
  MediaTrackNext: "MediaNext", MediaTrackPrevious: "MediaPrevious",
};
const NAMES = new Set<string>(KEYS);

export function keyFromCode(code: string): string | null {
  if (CODES[code]) return CODES[code];
  const m = /^(?:Key([A-Z])|Digit(\d)|Numpad(\d))$/.exec(code);
  if (m) return m[1] ?? m[2] ?? `Num${m[3]}`;
  return NAMES.has(code) ? code : null;
}

/** Keys that are hard to press while capturing (the system takes them) or not on every keyboard. */
export const KEY_LISTS: [string, string[]][] = [
  ["System", ["Win", "PrintScreen", "ContextMenu", "Pause", "ScrollLock", "CapsLock", "NumLock", "Escape", "Tab", "F11", "F12"]],
  ["Media", ["VolumeUp", "VolumeDown", "VolumeMute", "MediaPlayPause", "MediaNext", "MediaPrevious", "MediaStop", "BrowserBack", "BrowserForward"]],
  ["F13 – F24", ["F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24"]],
];

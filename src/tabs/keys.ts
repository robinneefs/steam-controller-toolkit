// Key names as layouts store them: physical US key positions, mirrored from `KEYS` in
// src-tauri/src/layout.rs (modifiers first). Captured from `KeyboardEvent.code`, so a key binds the
// same position on any keyboard layout.

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

export function keyLabel(name: string): string {
  if (LABELS[name]) return LABELS[name];
  const num = /^Num(\d)$/.exec(name);
  return num ? `Num ${num[1]}` : name;
}

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

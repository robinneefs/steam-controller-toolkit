// Inputs that MIDI and OSC mappings can read, shared by their tabs.

/** Buttons by their names in protocol.rs `buttons::NAMES`, with friendlier labels. */
export const BUTTONS: [string, string][] = [
  ["A", "A"], ["B", "B"], ["X", "X"], ["Y", "Y"],
  ["Up", "D-pad up"], ["Down", "D-pad down"], ["Left", "D-pad left"], ["Right", "D-pad right"],
  ["L", "Left bumper"], ["R", "Right bumper"], ["LTrigClick", "Left trigger click"], ["RTrigClick", "Right trigger click"],
  ["L4", "L4"], ["L5", "L5"], ["R4", "R4"], ["R5", "R5"],
  ["L3", "L3 (stick click)"], ["R3", "R3 (stick click)"], ["LPadClick", "Left pad click"], ["RPadClick", "Right pad click"],
  ["LPadTouch", "Left pad touch"], ["RPadTouch", "Right pad touch"], ["LStickTouch", "Left stick touch"],
  ["RStickTouch", "Right stick touch"], ["LGrip", "Left grip touch"], ["RGrip", "Right grip touch"],
  ["View", "View"], ["Menu", "Menu"], ["QAM", "Quick access"], ["Steam", "Steam"],
];

/** Every input a mapping can read. Axis keys mirror `AXES` in src-tauri/src/shaping.rs. */
export const SOURCES: [string, [string, string][]][] = [
  ["Triggers", [["trigger_left", "Left trigger"], ["trigger_right", "Right trigger"]]],
  ["Sticks", [
    ["left_stick_x", "Left stick X"], ["left_stick_y", "Left stick Y"],
    ["right_stick_x", "Right stick X"], ["right_stick_y", "Right stick Y"],
  ]],
  ["Trackpads", [
    ["left_pad_x", "Left pad X"], ["left_pad_y", "Left pad Y"], ["left_pad_pressure", "Left pad pressure"],
    ["right_pad_x", "Right pad X"], ["right_pad_y", "Right pad Y"], ["right_pad_pressure", "Right pad pressure"],
  ]],
  ["Motion", [
    ["tilt_pitch", "Tilt forward / back"], ["tilt_roll", "Tilt left / right"],
    ["gyro_pitch", "Turn speed: pitch"], ["gyro_roll", "Turn speed: roll"], ["gyro_yaw", "Turn speed: yaw"],
  ]],
  ["Buttons", BUTTONS.map(([n, l]) => [`button:${n}`, l])],
];
export const SOURCE_LABELS = new Map(SOURCES.flatMap(([, list]) => list));

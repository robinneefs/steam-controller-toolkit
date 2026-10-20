// The on/off switch every output tab (MIDI, OSC, Keyboard & mouse, Gamepad) starts with, followed
// by a status dot and text: one look for turning an output on and off.

/** The switch with the output's name; its checkbox has `data-on`. */
export function powerHtml(name: string, title: string): string {
  return `<label class="out-switch" title="${title}"><input type="checkbox" role="switch" data-on /><span class="out-switch-track" aria-hidden="true"></span><b>${name}</b></label>`;
}

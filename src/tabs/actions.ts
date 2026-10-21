// What actions are called, for the layout editors, the Command Deck and its on-screen menu (which
// has none of the editors' code).
import type { Action, PadInput } from "../controller";
import { keysLabel } from "./keys";

export const PAD_INPUTS: [PadInput, string][] = [
  ["a", "A"], ["b", "B"], ["x", "X"], ["y", "Y"], ["lb", "LB"], ["rb", "RB"], ["lt", "LT"], ["rt", "RT"],
  ["ls", "LS click"], ["rs", "RS click"], ["back", "Back"], ["start", "Start"], ["guide", "Guide"],
  ["up", "D-pad ↑"], ["down", "D-pad ↓"], ["left", "D-pad ←"], ["right", "D-pad →"],
];
const PAD_LABELS = new Map(PAD_INPUTS);
export const MOUSE_LABELS: Record<string, string> = { left: "Left click", right: "Right click", middle: "Middle click", back: "Mouse back", forward: "Mouse forward" };
export const SCROLL_LABELS: Record<string, string> = { up: "Scroll up", down: "Scroll down", left: "Scroll left", right: "Scroll right" };

export const escape = (s: string) => s.replace(/[&<>"]/g, (c) => `&${{ "&": "amp", "<": "lt", ">": "gt", '"': "quot" }[c]};`);
export const NONE: Action = { kind: "none" };

export function actionLabel(a: Action | undefined): string {
  switch (a?.kind ?? "none") {
    case "key": {
      const k = a as Extract<Action, { kind: "key" }>;
      return keysLabel(k.keys) + (k.repeat ? " ⟳" : "");
    }
    case "mouse":
      return MOUSE_LABELS[(a as Extract<Action, { kind: "mouse" }>).button];
    case "scroll": {
      const s = a as Extract<Action, { kind: "scroll" }>;
      return SCROLL_LABELS[s.dir] + (s.repeat ? " ⟳" : "");
    }
    case "pad":
      return `Xbox ${PAD_LABELS.get((a as Extract<Action, { kind: "pad" }>).input)}`;
    case "switch":
      return "Desktop ⇄ Game";
    case "run": {
      // The file or page name is enough to tell them apart.
      const t = (a as Extract<Action, { kind: "run" }>).target.replace(/[\\/]+$/, "");
      return `Open ${/^[a-z][\w+.-]*:\/\//i.test(t) ? t.replace(/^[a-z][\w+.-]*:\/\//i, "") : t.split(/[\\/]/).pop()}`;
    }
    default:
      return "–";
  }
}


import type { Snapshot } from "../controller";

/** One feature of the app. Add new tools by creating a Tab and registering it in main.ts. */
export interface Tab {
  id: string;
  title: string;
  /** Build the tab's DOM inside `root`. Called once, the first time the tab is opened. */
  mount(root: HTMLElement): void;
  /** Called with every snapshot while the tab is visible. */
  update?(snap: Snapshot): void;
  /** Called when another tab is opened. */
  hide?(): void;
}

import type { Snapshot } from "../controller";

/** Sidebar section a tab is listed under; "app" tabs sit at the bottom (About). */
export type TabGroup = "input" | "output" | "tools" | "app";

/** One feature of the app. Add new tools by creating a Tab and registering it in main.ts. */
export interface Tab {
  id: string;
  title: string;
  /** A shorter name for the sidebar, if the title doesn't fit there. */
  navTitle?: string;
  group: TabGroup;
  /** Inline SVG for the sidebar (24 x 24 stroke icon, see `src/tabs/icons.ts`). */
  icon: string;
  /** Build the tab's DOM inside `root`. Called once, the first time the tab is opened. */
  mount(root: HTMLElement): void;
  /** Called with every snapshot while the tab is visible. */
  update?(snap: Snapshot): void;
  /** Called when another tab is opened. */
  hide?(): void;
}

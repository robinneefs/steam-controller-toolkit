import { getVersion } from "@tauri-apps/api/app";
import { versionLabel } from "../version";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Tab } from "./tab";
import { ICONS } from "./icons";

const REPO = "https://github.com/robinneefs/steam-controller-toolkit";
const MODEL = "https://sketchfab.com/3d-models/steam-machine-controller-c1fcf77d71984c11818a5e2412816705";

/**
 * About the app, including the 3D model credit. The model is CC BY 4.0, so its attribution
 * here (title, author, license, and that we changed it) is a license requirement: keep it.
 */
export class AboutTab implements Tab {
  id = "about";
  title = "About";
  group = "app" as const;
  icon = ICONS.about;

  mount(root: HTMLElement): void {
    root.innerHTML = `
      <div class="about">
        <section class="about-hero">
          <img src="/logo.png" alt="" />
          <div>
            <h1>Steam Controller Toolkit</h1>
            <p class="muted"><span data-version></span>A tinkering toolkit for the 2026 Steam Controller: live input, haptics, MIDI and OSC, keyboard, mouse and gamepad, raw reports and recordings.</p>
            <div class="about-links">
              <a class="gbtn" href="${REPO}">Source on GitHub</a>
              <a class="gbtn" href="${REPO}/blob/main/docs/PROTOCOL.md">Protocol notes</a>
              <a class="gbtn" href="${REPO}/issues">Report an issue</a>
            </div>
          </div>
        </section>
        <div class="about-grid">
          <section class="tile">
            <h3>3D model</h3>
            <p>
              The controller model is adapted from
              <a href="${MODEL}">“Steam Machine + Controller”</a> by
              <a href="https://sketchfab.com/dark_igorek">dark_igorek</a>, licensed under
              <a href="https://creativecommons.org/licenses/by/4.0/">CC BY 4.0</a>.
            </p>
            <p class="muted">
              Changes: only the controller was kept, its buttons, sticks, trackpads, triggers and
              D-pad arms were split into moving parts, and it was compressed. dark_igorek does not
              endorse this project.
            </p>
          </section>
          <section class="tile">
            <h3>Built on</h3>
            <ul class="credits">
              <li><a href="https://github.com/libsdl-org/SDL">SDL</a>: its Steam Controller driver documents the protocol</li>
              <li><a href="https://github.com/libusb/hidapi">hidapi</a> and <a href="https://github.com/ruabmbua/hidapi-rs">hidapi-rs</a>: USB/HID access</li>
              <li><a href="https://tauri.app">Tauri</a>: the app shell</li>
              <li><a href="https://threejs.org">three.js</a>: the 3D view</li>
              <li><a href="https://github.com/Boddlnagg/midir">midir</a>: MIDI ports</li>
              <li><a href="https://github.com/microsoft/MIDI">Windows MIDI Services</a>: the Windows MIDI port
                (Microsoft's preview API, used until Windows ships it)</li>
              <li><a href="https://github.com/nefarius/ViGEmBus">ViGEmBus</a> by Nefarius
                (<a href="https://github.com/nefarius/ViGEmBus/blob/master/LICENSE">BSD 3-Clause</a>, shipped unmodified),
                <a href="https://github.com/CasualX/vigem-client">vigem-client</a> and
                <a href="https://github.com/emberian/evdev">evdev</a>: the gamepad</li>
            </ul>
          </section>
          <section class="tile">
            <h3>License</h3>
            <p>The source code is released under the <a href="${REPO}/blob/main/LICENSE">MIT License</a>.
            The 3D model keeps its CC BY 4.0 license.</p>
            <p class="muted">Not affiliated with or endorsed by Valve. Steam is a trademark of Valve Corporation.</p>
          </section>
        </div>
      </div>`;

    // Links open in the system browser, not inside the app window.
    root.querySelectorAll<HTMLAnchorElement>("a[href]").forEach((a) =>
      a.addEventListener("click", (e) => {
        e.preventDefault();
        openUrl(a.href).catch(() => window.open(a.href, "_blank"));
      }),
    );
    getVersion()
      .then((v) => (root.querySelector("[data-version]")!.textContent = `Version ${versionLabel(v)}. `))
      .catch(() => {});
  }
}

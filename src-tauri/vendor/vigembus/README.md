# ViGEmBus 1.22.0

`ViGEmBus_1.22.0_x64_x86_arm64.exe` is the unmodified installer of the ViGEm Bus Driver 1.22.0 by
Nefarius Software Solutions e.U., from https://github.com/nefarius/ViGEmBus/releases/tag/v1.22.0
(BSD 3-Clause, `LICENSE.txt`). The project was archived in November 2023; this is its last release.

The Windows build embeds it (`src-tauri/src/vigem.rs`). The Gamepad tab's **Install driver**
button writes it to `<app data>/gamepad/` and runs it; Windows asks for administrator permission.
Users remove it in Settings → Apps → Installed apps → "ViGEm Bus Driver".

- SHA-256: `89220a7865076b342892f98865f3499fb7c4cfd673159e89d352c360fd014c6a`
- Authenticode: valid, signed by Nefarius Software Solutions e.U. (checked Oct 2026)

Why a driver: Windows has no way for an app to add a game controller without one. Its
`InputInjector` gamepad injection returns success for an unpackaged app, but XInput, GameInput,
Windows.Gaming.Input and Raw Input never see the pad (tested on Windows 11 26100, Oct 2026).

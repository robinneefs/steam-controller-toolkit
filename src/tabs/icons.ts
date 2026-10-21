// Sidebar icons: 24 x 24, drawn with currentColor strokes so they follow the item's colour.
const svg = (body: string, attrs = 'stroke-linecap="round" stroke-linejoin="round"') =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" ${attrs} aria-hidden="true">${body}</svg>`;

export const ICONS = {
  visualizer: svg('<path d="M7 5h10a5 5 0 0 1 4.9 6l-1 5a3 3 0 0 1-5.2 1.4L14 16h-4l-1.7 1.4A3 3 0 0 1 3.1 16l-1-5A5 5 0 0 1 7 5z"/><path d="M6 10.5h4M8 8.5v4M15.5 9.5h.01M17.5 11.5h.01"/>'),
  raw: svg('<path d="M8 4H7a2 2 0 0 0-2 2v4l-2 2 2 2v4a2 2 0 0 0 2 2h1M16 4h1a2 2 0 0 1 2 2v4l2 2-2 2v4a2 2 0 0 1-2 2h-1"/>'),
  midi: svg('<circle cx="12" cy="12" r="9"/><path d="M7.5 12h.01M16.5 12h.01M8.8 8.8h.01M15.2 8.8h.01M12 7.5h.01M10.5 17.5h3"/>'),
  osc: svg('<circle cx="12" cy="12" r="2"/><path d="M8.5 15.5a5 5 0 0 1 0-7M15.5 8.5a5 5 0 0 1 0 7M5.6 18.4a9 9 0 0 1 0-12.8M18.4 5.6a9 9 0 0 1 0 12.8"/>'),
  keymouse: svg('<rect x="2" y="6" width="14" height="11" rx="2"/><path d="M5 9.5h.01M8 9.5h.01M11 9.5h.01M5 13.5h6"/><rect x="17.5" y="9" width="5" height="9" rx="2.5"/><path d="M20 9v3"/>'),
  gamepad: svg('<path d="M6 7h12a4 4 0 0 1 3.9 4.8l-.9 4.6a2.6 2.6 0 0 1-4.5 1.2L15 16H9l-1.5 1.6a2.6 2.6 0 0 1-4.5-1.2l-.9-4.6A4 4 0 0 1 6 7z"/><circle cx="8" cy="11" r="1.7"/><circle cx="15.5" cy="13.6" r="1.7"/>'),
  deck: svg('<rect x="3" y="3" width="7.5" height="7.5" rx="2"/><rect x="13.5" y="3" width="7.5" height="7.5" rx="2"/><rect x="3" y="13.5" width="7.5" height="7.5" rx="2"/><circle cx="17.25" cy="17.25" r="3.75"/><path d="M17.25 13.5v1.6"/>'),
  macros: svg('<circle cx="6" cy="7" r="2.5"/><path d="M11.5 7H21M11.5 12H21M11.5 17H21M3.5 12h5M3.5 17h5"/>'),
  haptics: svg('<rect x="8" y="4" width="8" height="16" rx="2"/><path d="M4.5 9v6M19.5 9v6M1.8 10.5v3M22.2 10.5v3"/>'),
  recorder: svg('<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="3.5" fill="currentColor" stroke="none"/>', ""),
  settings: svg('<circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1"/><circle cx="12" cy="12" r="6.5"/>'),
  about: svg('<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>'),
  collapse: svg('<path d="M11 17l-5-5 5-5M18 17l-5-5 5-5"/>'),
};

// Sidebar icons: 24 x 24, drawn with currentColor strokes so they follow the item's colour.
const svg = (body: string, attrs = 'stroke-linecap="round" stroke-linejoin="round"') =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" ${attrs} aria-hidden="true">${body}</svg>`;

export const ICONS = {
  visualizer: svg('<path d="M7 5h10a5 5 0 0 1 4.9 6l-1 5a3 3 0 0 1-5.2 1.4L14 16h-4l-1.7 1.4A3 3 0 0 1 3.1 16l-1-5A5 5 0 0 1 7 5z"/><path d="M6 10.5h4M8 8.5v4M15.5 9.5h.01M17.5 11.5h.01"/>'),
  raw: svg('<path d="M8 4H7a2 2 0 0 0-2 2v4l-2 2 2 2v4a2 2 0 0 0 2 2h1M16 4h1a2 2 0 0 1 2 2v4l2 2-2 2v4a2 2 0 0 1-2 2h-1"/>'),
  midi: svg('<circle cx="12" cy="12" r="9"/><path d="M7.5 12h.01M16.5 12h.01M8.8 8.8h.01M15.2 8.8h.01M12 7.5h.01M10.5 17.5h3"/>'),
  haptics: svg('<rect x="8" y="4" width="8" height="16" rx="2"/><path d="M4.5 9v6M19.5 9v6M1.8 10.5v3M22.2 10.5v3"/>'),
  recorder: svg('<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="3.5" fill="currentColor" stroke="none"/>', ""),
  about: svg('<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>'),
  collapse: svg('<path d="M11 17l-5-5 5-5M18 17l-5-5 5-5"/>'),
};

// One icon set, drawn here: 24x24, 1.8 px strokes, round joins. No icon font, no library.
const s = (body: string, fill = false) =>
  `<svg viewBox="0 0 24 24" fill="${fill ? 'currentColor' : 'none'}" stroke="${fill ? 'none' : 'currentColor'}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`

export const icon = {
  // The mark: an anvil, side on, with a sound wave on its side (D-029). Drawn by app/icon.py,
  // which makes the app icon from the same shapes and prints this line.
  logo: `<svg viewBox="0.4 7.8 31.2 16.4"><path d="M1.14 10.1C2.36 9.69 5.77 9.62 10.27 9.62L10.27 9.15L12.18 9.15L12.18 8.6L31 8.6L31 9.83C26.91 9.96 22.95 11.46 21.86 14.6C21.18 17.05 22.55 20.05 26.5 21.15L26.5 23.33L20.91 23.33C20.64 20.6 14.5 20.6 14.23 23.33L8.5 23.33L8.5 21.15C12.18 20.19 13.95 18.28 13.95 16.1C9.86 15.69 4.14 13.92 1.14 10.1Z" fill="currentColor"/><g fill="none" stroke-linecap="round" stroke-linejoin="round" stroke-width="0.46" style="stroke:var(--surface)"><path d="M5.09 12.42C6.86 13.65 8.64 14.26 10.14 14.46"/><path d="M10.48 9.49L10.48 15.08"/><path d="M12.32 9.35L12.32 15.42"/><path d="M12.32 9.35L29.5 9.35"/><path d="M14.64 16.51C14.64 18.28 13.14 20.05 10.55 21.01"/><path d="M22.55 12.08C21.45 13.65 20.77 16.37 21.86 18.69C22.55 20.05 24.05 21.15 25.82 21.69"/><path d="M13.68 21.83C14.77 19.51 20.36 19.51 21.45 21.83"/><path d="M13.41 12.55C13.94 12.55 14.03 11.6 14.57 11.6C15.2 11.6 15.3 13.65 15.93 13.65C16.56 13.65 16.67 10.65 17.3 10.65C17.92 10.65 18.03 14.46 18.66 14.46C19.25 14.46 19.36 11.74 19.95 11.74C20.36 11.74 20.43 12.55 20.84 12.55" stroke-width="0.9"/></g></svg>`,
  start: s('<path d="M6 5v14"/><path d="M18 6 9 12l9 6z" fill="currentColor"/>'),
  play: s('<path d="M7 4.5v15a1 1 0 0 0 1.5.86l12.5-7.5a1 1 0 0 0 0-1.72L8.5 3.64A1 1 0 0 0 7 4.5z"/>', true),
  stop: s('<rect x="5.5" y="5.5" width="13" height="13" rx="2"/>', true),
  rec: s('<circle cx="12" cy="12" r="6.5"/>', true),
  loop: s('<path d="M17 2.5 20.5 6 17 9.5"/><path d="M3.5 11.5V10a4 4 0 0 1 4-4h13"/><path d="M7 21.5 3.5 18 7 14.5"/><path d="M20.5 12.5V14a4 4 0 0 1-4 4h-13"/>'),
  metro: s('<path d="M8.5 3h7l4 18h-15z"/><path d="m12 15 6-9"/>'),
  gear: s('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>'),
  chevron: s('<path d="m6 9 6 6 6-6"/>'),
  down: s('<path d="m6 15 6-6 6 6"/>'),
  plus: s('<path d="M12 5v14M5 12h14"/>'),
  wave: s('<path d="M3 12h2M7 8v8M11 5v14M15 9v6M19 7v10M21 12h0"/>'),
  keys: s('<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M8 5v9M12 5v9M16 5v9M8 14v5M12 14v5M16 14v5"/>'),
  mono: s('<circle cx="12" cy="12" r="7"/>'),
  ab: s('<path d="M4 18 8 6l4 12M5.3 14h5.4"/><path d="M14 6h3.5a3 3 0 0 1 0 6H14zM14 12h4a3 3 0 0 1 0 6h-4z"/>'),
  search: s('<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/>'),
  close: s('<path d="M6 6l12 12M18 6 6 18"/>'),
  undo: s('<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/>'),
  redo: s('<path d="m15 14 5-5-5-5"/><path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H13"/>'),
  folder: s('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>'),
  export: s('<path d="M12 3v12M7 8l5-5 5 5"/><path d="M5 15v4a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4"/>'),
  plug: s('<path d="M9 2v6M15 2v6"/><path d="M6 8h12v4a6 6 0 0 1-12 0z"/><path d="M12 18v4"/>'),
  pointer: s('<path d="m5 3 14 8-6 1.5L10 19z"/>'),
  pencil: s('<path d="M16 3.5a2.1 2.1 0 0 1 3 3L8 17.5 4 19l1.5-4z"/>'),
  magnet: s('<path d="M6 3v9a6 6 0 0 0 12 0V3"/><path d="M6 7h4M14 7h4"/>'),
  terminal: s('<path d="m5 8 4 4-4 4M12 17h7"/>'),
}

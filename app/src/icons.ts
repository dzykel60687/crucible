// One icon set, drawn here: 24x24, 1.8 px strokes, round joins. No icon font, no library.
const s = (body: string, fill = false) =>
  `<svg viewBox="0 0 24 24" fill="${fill ? 'currentColor' : 'none'}" stroke="${fill ? 'none' : 'currentColor'}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`

export const icon = {
  logo: `<svg viewBox="0 0 32 32"><path d="M6.5 9h19l-2.4 14.6A4 4 0 0 1 19.2 27h-6.4a4 4 0 0 1-3.9-3.4z" fill="currentColor"/><path d="M4 8h22.5l2.5-1.5" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/><ellipse cx="16" cy="11.6" rx="8" ry="1.5" fill="#ffd68c"/></svg>`,
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

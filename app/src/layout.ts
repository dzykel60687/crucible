// Layout themes (D-028). A layout is data: a name on <html> that style.css turns into different
// sizes and grid areas. The same views draw in every layout; nothing here forks behaviour.
// Saved per machine, since the right layout depends on the screen (laptop, touch, big monitor).
import { refreshPalette } from './timeline'
import { changed, ui } from './store'

export interface Layout { id: string; name: string; note: string; sketch: string }

// Sketches: top bar, track list, timeline, panel, as 60x38 boxes.
const box = (x: number, y: number, w: number, h: number, accent = false) => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="1.5" fill="${accent ? 'var(--accent)' : 'currentColor'}" opacity="${accent ? 0.75 : 0.28}"/>`
const svg = (body: string) => `<svg viewBox="0 0 60 38" width="60" height="38">${body}</svg>`

export const LAYOUTS: Layout[] = [
  { id: 'studio', name: 'Studio', note: 'Tracks left, editors below. The default.', sketch: svg(box(0, 0, 60, 5) + box(0, 7, 14, 18) + box(16, 7, 44, 18, true) + box(0, 27, 60, 11)) },
  { id: 'compact', name: 'Compact', note: 'Slim rows and bars for a laptop screen.', sketch: svg(box(0, 0, 60, 3) + box(0, 5, 11, 22) + box(13, 5, 47, 22, true) + box(0, 29, 60, 9)) },
  { id: 'touch', name: 'Touch', note: 'Bigger rows and buttons for the touchscreen.', sketch: svg(box(0, 0, 60, 7) + box(0, 9, 18, 16) + box(20, 9, 40, 16, true) + box(0, 27, 60, 11)) },
  { id: 'side', name: 'Side editor', note: 'Editors on the right; the timeline gets the full height.', sketch: svg(box(0, 0, 60, 5) + box(0, 7, 10, 31) + box(12, 7, 26, 31, true) + box(40, 7, 20, 31)) },
  { id: 'mixing', name: 'Mixing', note: 'The mixer takes most of the screen, the arrangement stays on top.', sketch: svg(box(0, 0, 60, 5) + box(0, 7, 14, 10) + box(16, 7, 44, 10, true) + box(0, 19, 60, 19)) },
]

const KEY = 'crucible.layout'

export function currentLayout(): string {
  return document.documentElement.dataset.layout ?? 'studio'
}

export function applyLayout(id: string) {
  if (!LAYOUTS.some((l) => l.id === id)) id = 'studio'
  document.documentElement.dataset.layout = id
  document.documentElement.style.removeProperty('--panel-h') // a hand-resized panel belongs to the old layout
  document.documentElement.style.removeProperty('--side-w')
  try { localStorage.setItem(KEY, id) } catch { /* private mode: not remembered, still applied */ }
  if (id === 'mixing') { ui.tab = 'mixer'; ui.panelOpen = true }
  refreshPalette()
  changed()
}

export function loadLayout() {
  let id = 'studio'
  try { id = localStorage.getItem(KEY) ?? 'studio' } catch { /* use the default */ }
  document.documentElement.dataset.layout = LAYOUTS.some((l) => l.id === id) ? id : 'studio'
}

export function cycleLayout() {
  const i = LAYOUTS.findIndex((l) => l.id === currentLayout())
  applyLayout(LAYOUTS[(i + 1) % LAYOUTS.length].id)
  return LAYOUTS[(i + 1) % LAYOUTS.length]
}

# 05. GUI design brief

The GUI is the reason this project exists. Sound quality is table stakes that every DAW
already meets. What Dzyan wants is an interface that looks modern and stays uncluttered.

## The diagnosis

Old DAWs do not look dated because of their toolkits. They look dated because every option
ever requested is still on screen: nested menus, rows of tiny icon buttons, floating palettes,
hundreds of preferences. The cure is discipline, not technology.

## Principles

1. **Refusing features is the product.** If a control is not needed weekly by the people this
   is built for, it is not on screen. If it is not needed monthly, it does not exist.
2. **One window.** The only floating windows are plugin editors, which the plugins draw themselves.
3. **The top bar holds what is always needed and nothing else.**
4. **No menu deeper than one level.** No sub-sub-menus.
5. **One settings page.** Not a tree of preference panes.
6. **Every control is big enough to touch.** One of the three machines is a touchscreen laptop.
7. **Keyboard first for the common actions.** Space plays and stops. One key or one chord arms, records, splits (Ctrl+E), undoes. Full list in Settings, Keyboard.
8. **Smooth or not at all.** Scrolling, zooming and meters hold sixty frames per second, or the
   design is simplified until they do.
9. **Safety is visible.** Clip light, headroom, overruns and "device is resampling" are always
   in view. Nothing bad happens silently.

## Layout, first proposal

To be judged by eye at M1. Nothing here is final.

```
+--------------------------------------------------------------------------------+
| TOP BAR                                                                        |
| [project name]   |<  >   tempo  bar.beat  time          | peak   [output device] | (gear) |
+----------------+---------------------------------------------------------------+
| TRACK HEADERS  | TIMELINE (canvas)                                             |
| name           | ruler                                                         |
| mute solo      | clips, waveforms, MIDI notes in miniature                     |
| volume  pan    |                                                               |
| ...            |                                                               |
| [+ add track]  |                                                               |
+----------------+---------------------------------------------------------------+
| BOTTOM PANEL, one of:  Mixer  |  Piano roll  |  Clip editor      (collapsible) |
+--------------------------------------------------------------------------------+
```

Top bar contents, left to right:

- Project name. Click for open, save, export. That is the whole "File menu".
- Transport: return to start, play or stop. Record and loop join it when the engine can do them
  (D-038: nothing is shown that does not work).
- Tempo and time signature.
- Position, in bars and in time.
- Peak (headroom) number and clip light. Loudness, mono and reference A/B come back when built.
- The audio output in use, its rate, and the dropout count. A gear for settings.

That is the entire top bar. Anything proposed for it has to displace something.

### Layout themes (D-028)

People prefer different arrangements, so the same screen comes in five layouts: Studio (above),
Compact, Touch, Side editor and Mixing. A layout only changes sizes and where the regions sit;
every layout runs the same views. New layouts are welcome if they fit that rule. Docking and
floating panels are not: they break principle 2 and multiply what has to be tested.

### Mouse (D-027)

Clicking, dragging, trimming, copying and pasting clips and notes follow the conventions most
DAWs share, so nothing has to be learned: click selects, Shift adds, Ctrl toggles, drag moves,
Ctrl+drag copies, edges trim, empty-space drag selects, Alt ignores the grid. Every drag previews
on screen and becomes one command, one undo step, when the button is released.

Added 2026-11-22 after testing:
- A drag that would land a clip on the wrong kind of track shows it while dragging (red outline,
  not-allowed cursor), instead of only refusing on release.
- The selected track after selecting clips is the topmost one, so copy and paste of a box
  selection lands on the same tracks again.
- Piano roll rows are never smaller than `--piano-row` (11 px, 18 px in Touch); the wheel
  scrolls pitch. Fitting every note made rows 4.7 px tall on a laptop panel, too small to hit.
- Effects (2026-11-23): mixer slots show each track's effects; click to edit, right-click to
  bypass, move or remove, "+ effect" to add. The editor is drawn from the engine's parameter table
  (`fx.kinds`): numbers drag up and down (Shift for fine, double-click for the default, click to
  type), options are segmented buttons. One command per change, on release, like every drag.
- Audio comes in by dropping files on the window (the track under the pointer, at the snapped
  beat), or Ctrl+I / the right-click menus with a path. A file never lands on top of audio
  already there; it gets its own new track.

## Visual direction

- Dark theme first. Light theme later through the same design tokens.
- Flat surfaces, generous spacing, one accent colour, a restrained palette for track colours.
- One typeface, bundled with the app so it is identical on every system.
- Icons from one consistent set. Labels where an icon would be ambiguous.
- Motion is short and functional. No decorative animation.
- Meters and waveforms are the only "busy" elements, and they are the content.

Dzyan chooses the look with his eyes. M1 delivers a clickable mockup of the main screen so he
can react to something real before any feature UI is built.

## Technical rules for the front end

- **Canvas for the heavy parts:** timeline, piano roll, meters, waveforms.
- **Page elements for the chrome:** top bar, track headers, dialogs, settings.
- **High-rate data bypasses the UI framework** and draws straight to the canvas.
- **The UI never computes audio.** It displays what the engine reports.
- **Design tokens** for colour, spacing, radius and type, defined once.
- **Budget:** a frame is about sixteen milliseconds. Drawing must fit with room to spare on the laptop.
- Proposed stack, decision D-015: TypeScript, React, Vite, Tailwind, Radix primitives, Canvas 2D.

## Motion and effects: the budget, and the pushback rule

Dzyan asked (2026-11-20) to be caught if a GUI wish would cost the engine or the frame rate.
The middle ground is fixed here so it does not have to be argued each time.

Allowed:
- Motion that shows a change of state: a panel opening, a value landing, a clip snapping.
  150 ms or less, CSS `transform` and `opacity` only, which the compositor handles without
  re-laying-out the page.
- Hover and press feedback through colour changes.
- The playhead and meters, which move because the audio moves. They follow the bridge rates in
  `02-architecture.md`.

Not allowed without a measured exception recorded in `08-decisions.md`:
- Anything that animates while the app is idle: looping, breathing or animated backgrounds.
- Blur, backdrop filters and large soft shadows over the timeline, piano roll or meters. They
  force repaints of large areas every frame.
- Animation and physics libraries. CSS transitions cover the need.
- Per-frame requests to the engine beyond the bridge table.
- Live previews that run audio or plugin code on the UI's behalf.

The pushback rule: when a request falls outside this, Claude says what it costs (frame time from
the browser profiler, CPU, engine work), offers the closest version that fits, and builds only
after Dzyan decides. Example: a spring-physics panel becomes a 120 ms ease-out; an animated
gradient becomes a static one.

## What must be true before any feature UI is built

The M1 stress prototype passes on the Lynx machine and the laptop:

- A timeline with five thousand clips scrolls and zooms smoothly.
- Thirty-two meters update sixty times a second from the Rust side.
- A plugin editor window floats above the main window, takes and returns keyboard focus
  correctly, and the space bar still controls the transport.

If it fails and cannot be fixed in a few days, the fallback is egui. See decision D-006.

## Platform notes

- The UI runs in a different browser engine on each system: Chromium's on Windows, WebKit on
  Mac and Linux. Use only features all three support. Test the look on each when the ports start.
- Bundle fonts. Do not rely on Segoe UI or any system font.
- Define shortcuts against a "primary modifier": Ctrl on Windows and Linux, Cmd on Mac.
- Window chrome, title bar buttons and menus differ per system. Keep the custom top bar inside
  the window content and let the OS draw its own title bar, at least until the ports are real.
- Touch input matters on Windows today because of the laptop. Keep it working, since it also
  helps any future tablet use.

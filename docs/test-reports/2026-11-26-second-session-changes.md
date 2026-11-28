# Test steps: what changed after the second session (0.0.4)

For D-037 part three, D-042 and D-043. Every step here answers something Dzyan found on 2026-11-26.

## Setup

| Item | Value |
|---|---|
| Build | 0.0.4, `Crucible_0.0.4_x64_en-US.msi` |
| Machine | Home desktop, Behringer UMC202HD |
| Before installing | Uninstall any older Crucible from Windows' Apps list |

## What a machine verified (dev laptop)

| # | Check | Result |
|---|---|---|
| 1 | The installer is for all users, in `Program Files\Crucible`, asks for admin, and makes a `logs` folder with write access for Users | Pass (MSI tables; the permission text applied to a test folder) |
| 2 | An unsaved project takes a file; saving takes the audio along; Save as makes a whole copy; discarding removes the temporary folder | Pass (unit tests) |
| 3 | "Add audio" on an empty audio track opens the file picker; the clip lands in a project never saved | Pass (GUI test) |
| 4 | Ctrl+S on an unsaved project asks for a name and folder, with Browse; the audio is in the saved folder | Pass (GUI test) |
| 5 | Browse shows Explorer's real folder picker | Pass (GUI test finds the dialog and closes it) |
| 6 | Closing with unsaved changes asks Save, Don't save or Cancel; Don't save closes | Pass (GUI test) |
| 7 | Playback stops at the end of the song; Play from there starts over; the end tab drags out | Pass (GUI test, a 2-second file) |
| 8 | Wheel through time, Ctrl+wheel out to the whole song, Ctrl+drag, the scroll bar | Pass (GUI test) |
| 9 | Position reads minutes:seconds.tenths | Pass (GUI test) |
| 10 | FX on a track opens the effect picker, then lists the effects | Pass (GUI test) |
| 11 | Effect editor: a slider, a typed value and the range per number; nothing runs off the side | Pass (GUI test, Echo) |
| 12 | Removing the last clip that uses a file lets its sound out of memory | Pass (unit test) |
| 13 | 81 unit tests, 69 GUI checks | Pass |

Not verified: installing 0.0.4; the `logs` folder's access rights on a real install; anything
audible; how the scrolling and the sliders feel.

## Needs Dzyan's hands, ears or hardware

| # | Step | Expected | Pass / Fail | What actually happened |
|---|---|---|---|---|
| 1 | Install 0.0.4, keeping the folder it offers | Windows asks for admin once; it installs to `C:\Program Files\Crucible` | | |
| 2 | Start Crucible, Settings, Diagnostics | The log file shown is `C:\Program Files\Crucible\logs\crucible-app.log`, and it grows | | |
| 3 | New project, drag a song in | It lands at once; nothing asks you to save | | |
| 4 | Add an audio track, press "Add audio…" on it | Explorer's file picker opens; the song you choose lands on that track | | |
| 5 | Roll the wheel over the timeline; drag the bar along the bottom; Ctrl+wheel all the way out | You reach the end of the longest song; fully out, the whole song is on screen | | |
| 6 | Ctrl+drag on empty space; middle-button drag anywhere | The view moves with the mouse | | |
| 7 | Play to the end of the song | Playback stops by itself at the end line; position reads like `8:32.6` | | |
| 8 | Drag the tab at the end line on the ruler to the right, play to the end again | Playback runs on to where you put it | | |
| 9 | FX on a track, add Echo | Sliders with a value box and a range each; Sync is a list; nothing to scroll sideways | | |
| 10 | Drag a slider; click a value box, type a number, Enter | Both change the sound when let go; no second popup | | |
| 11 | Ctrl+S | Name, "Save in", Browse; after saving, the folder holds `project.json` and `audio` with your songs | | |
| 12 | Task Manager: note Crucible's memory, remove a long clip, look again | Memory drops by about what the import message said it held | | |
| 13 | Change something, close the window with the X | It asks Save, Don't save or Cancel | | |

## When a step fails

Press F9 at the moment it goes wrong and type what you noticed. Then Settings, Diagnostics,
Create report file, and send the file it names.

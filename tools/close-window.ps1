# PLATFORM: Windows (user32 FindWindow and PostMessage).
#   macOS and Linux: the GUI test's check that the system's picker appears needs each system's own
#   way to find a window by its title and close it (AppleScript; xdotool or a portal call).
#
# Waits up to ten seconds for a top-level window with this exact title, asks it to close the way
# its own X does, and says "closed" or "not found". Used by tools\gui-test.mjs to prove that Browse
# shows the real folder picker, which the test cannot otherwise see or work.
param([Parameter(Mandatory)][string]$Title)
Add-Type -Namespace Crucible -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(IntPtr cls, string title);
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
[DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hwnd, uint what);
'@
for ($i = 0; $i -lt 50; $i++) {
  # Any window class. (A $null string would arrive as "", a class no window has.)
  $h = [Crucible.Win]::FindWindowW([IntPtr]::Zero, $Title)
  if ($h -ne [IntPtr]::Zero) {
    Start-Sleep -Milliseconds 400   # let it finish opening, so the close is not lost
    # A window that floats above another one has that one as its owner (GW_OWNER = 4).
    $owned = [Crucible.Win]::GetWindow($h, 4) -ne [IntPtr]::Zero
    [void][Crucible.Win]::PostMessageW($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)   # WM_CLOSE
    if ($owned) { 'closed (it had an owner)' } else { 'closed' }
    exit 0
  }
  Start-Sleep -Milliseconds 200
}
'not found'
exit 1

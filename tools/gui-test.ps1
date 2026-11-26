# PLATFORM: Windows (PowerShell 7, WebView2's debugging port, Win32 thread calls).
#   macOS and Linux: start the window with its web inspector's remote port instead; the test
#   itself (gui-test.mjs) is portable. The dropout part needs a way to pause one thread there.
#
# GUI smoke test for the real window program. Starts it, clicks through it the way a hand does
# (tools\gui-test.mjs), forces dropouts by freezing the mixing thread, closes it, and says what
# failed. Run it after any change to the window:
#
#   pwsh -File tools\gui-test.ps1                       # tests target\release\crucible-app.exe
#   pwsh -File tools\gui-test.ps1 -Exe dist\app\crucible-app.exe
#
# It plays the built-in test project, which is silent, through the default output for about
# fifteen seconds. Exit code 0 means every check passed.
param([string]$Exe = (Join-Path $PSScriptRoot '..\target\release\crucible-app.exe'), [int]$Port = 9333, [switch]$NoDropouts)
$ErrorActionPreference = 'Stop'
$Exe = (Resolve-Path $Exe).Path
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
$app = Start-Process -FilePath $Exe -PassThru
try {
  $more = [string[]]@("$Port")
  if (-not $NoDropouts) { $more += (Join-Path $PSScriptRoot 'freeze-thread.ps1') } else { $more += '' }
  # A 44.1 kHz file, for the convert question.
  $cd = Join-Path $PSScriptRoot '..\dist\test-audio\cd-44k1.wav'
  if (Test-Path $cd) { $more += (Resolve-Path $cd).Path }
  node (Join-Path $PSScriptRoot 'gui-test.mjs') @more
  $code = $LASTEXITCODE
} finally {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $null
  if (-not $app.HasExited) { [void]$app.CloseMainWindow(); if (-not $app.WaitForExit(8000)) { Stop-Process -Id $app.Id -Force } }
}
$log = Join-Path (Split-Path $Exe) 'crucible-app.log'
if (Test-Path $log) {
  $bad = Select-String -Path $log -CaseSensitive -Pattern ' ERROR E9003 | PANIC | CRASH ' | Select-Object -First 5
  if ($bad) { 'FAIL  the session log holds errors from the window itself:'; $bad | ForEach-Object { '      ' + $_.Line.Substring(0, [Math]::Min(180, $_.Line.Length)) }; $code = 1 }
  "log: $log"
}
exit $code

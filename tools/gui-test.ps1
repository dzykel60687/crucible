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
$log = Join-Path (Split-Path $Exe) 'crucible-app.log'
# A 44.1 kHz file, for the convert question.
$cd = Join-Path $PSScriptRoot '..\dist\test-audio\cd-44k1.wav'
$cd = if (Test-Path $cd) { (Resolve-Path $cd).Path } else { '' }
# The system's file and folder pickers cannot be worked by the test. The window answers them
# with these instead (see `pick` in app\src-tauri\src\main.rs): the file to import, and an empty
# folder to save the test's project in.
$saveRoot = Join-Path ([IO.Path]::GetTempPath()) "crucible-gui-test-$PID"
New-Item -ItemType Directory -Force $saveRoot | Out-Null
$env:CRUCIBLE_TEST_PICK_FILES = $cd
$env:CRUCIBLE_TEST_PICK_FOLDER = $saveRoot
# Plugins (D-044): the window looks for them in a folder of the test's own, holding the test
# instrument (crates\crucible-test-synth, built here if it is not), and keeps its plugin list in
# a file of the test's own. The machine's real plugins and its real list are left alone.
$plugins = "$saveRoot-plugins"
$synth = Join-Path $PSScriptRoot '..\target\release\crucible_test_synth.dll'
Push-Location (Join-Path $PSScriptRoot '..'); try { cargo build --release -p crucible-test-synth 2>&1 | Out-Null } finally { Pop-Location }
if (-not (Test-Path $synth)) { throw "the test instrument is not built: $synth" }
$inner = Join-Path $plugins 'vst3\Crucible Test Tone.vst3\Contents\x86_64-win'
New-Item -ItemType Directory -Force $inner | Out-Null
Copy-Item $synth (Join-Path $inner 'Crucible Test Tone.vst3')
$env:CRUCIBLE_VST3_PATH = Join-Path $plugins 'vst3'
$env:CRUCIBLE_PLUGIN_CATALOG = Join-Path $plugins 'plugins.json'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
$app = Start-Process -FilePath $Exe -PassThru
try {
  $freeze = if ($NoDropouts) { '' } else { Join-Path $PSScriptRoot 'freeze-thread.ps1' }
  # The window's process id: the last check asks it to close, as the title bar's X does.
  node (Join-Path $PSScriptRoot 'gui-test.mjs') "$Port" $freeze $cd "$($app.Id)" $saveRoot $log
  $code = $LASTEXITCODE
} finally {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $null
  $env:CRUCIBLE_TEST_PICK_FILES = $null
  $env:CRUCIBLE_TEST_PICK_FOLDER = $null
  $env:CRUCIBLE_VST3_PATH = $null
  $env:CRUCIBLE_PLUGIN_CATALOG = $null
  if (-not $app.HasExited) { [void]$app.CloseMainWindow(); if (-not $app.WaitForExit(4000)) { Stop-Process -Id $app.Id -Force; [void]$app.WaitForExit(4000) } }
  Remove-Item -Recurse -Force $saveRoot -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force $plugins -ErrorAction SilentlyContinue
}
if (Test-Path $log) {
  $bad = Select-String -Path $log -CaseSensitive -Pattern ' ERROR E9003 | PANIC | CRASH ' | Select-Object -First 5
  if ($bad) { 'FAIL  the session log holds errors from the window itself:'; $bad | ForEach-Object { '      ' + $_.Line.Substring(0, [Math]::Min(180, $_.Line.Length)) }; $code = 1 }
  "log: $log"
}
exit $code

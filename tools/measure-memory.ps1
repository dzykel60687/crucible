# PLATFORM: Windows (PowerShell 7, CIM). macOS: `footprint` or Activity Monitor per process tree;
# Linux: sum Pss from /proc/<pid>/smaps_rollup over the tree.
#
# Starts a program, waits, and reports memory for it and every process it started (WebView2's
# browser, GPU, renderer and utility processes for crucible-app). "TaskMgr MB" is the private
# working set, the number Task Manager shows and the one the budget in
# docs/01-vision-and-priorities.md is measured in. "Commit MB" is reserved memory, which the GPU
# process inflates; it is shown for reference only.
#
#   pwsh -File tools\measure-memory.ps1 -Exe dist\app\crucible-app.exe
#   pwsh -File tools\measure-memory.ps1 -Exe dist\app\crucible-app.exe -BrowserArgs "--disable-gpu"
param([Parameter(Mandatory)][string]$Exe, [int]$Wait = 10, [string]$BrowserArgs = '')
if ($BrowserArgs) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $BrowserArgs }
$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds $Wait
$all = Get-CimInstance Win32_Process
$tree = @($p.Id); $grew = $true
while ($grew) {
  $grew = $false
  foreach ($q in $all) { if ($tree -contains $q.ParentProcessId -and -not ($tree -contains $q.ProcessId)) { $tree += $q.ProcessId; $grew = $true } }
}
$perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process
$rows = foreach ($id in $tree) {
  $pr = Get-Process -Id $id -ErrorAction SilentlyContinue
  if (-not $pr) { continue }
  $cmd = ($all | Where-Object ProcessId -eq $id).CommandLine
  $type = if ($cmd -match '--type=([\w-]+)') { $matches[1] } else { 'main' }
  if ($cmd -match '--utility-sub-type=([\w.]+)') { $type += ':' + ($matches[1] -split '\.')[-1] }
  [pscustomobject]@{
    Name = $pr.ProcessName; Type = $type
    TaskMgrMB = [math]::Round((($perf | Where-Object IDProcess -eq $id).WorkingSetPrivate) / 1MB, 1)
    CommitMB = [math]::Round($pr.PrivateMemorySize64 / 1MB, 1)
  }
}
$rows | Sort-Object TaskMgrMB -Descending | Format-Table -AutoSize
"Total: {0:N0} MB as Task Manager counts it (private working set), {1:N0} MB committed, {2} processes" -f `
  ($rows | Measure-Object TaskMgrMB -Sum).Sum, ($rows | Measure-Object CommitMB -Sum).Sum, $rows.Count
Stop-Process -Id $p.Id

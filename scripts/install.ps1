# Installs svtoggle: copies the release build to %ProgramFiles%\svtoggle, registers a scheduled task
# `svtoggle` (Highest privileges, no trigger) and makes Desktop + Start Menu shortcuts that run that task,
# so launching never shows a UAC prompt. Asks for UAC once, here, when not already elevated.
# Re-run after every `cargo build --release`.   -DryRun: print the plan only.   -Uninstall: remove everything.
param([switch]$Uninstall, [switch]$DryRun, [switch]$PauseOnError)
$ErrorActionPreference = 'Stop'
# the elevated child runs in its own window: keep it open on failure so the error can be read
trap { "ERROR: $_"; if ($PauseOnError) { Read-Host 'Install failed - press Enter to close' }; exit 1 }

$src = Join-Path $PSScriptRoot '..\target\release'
# Program Files, not %LOCALAPPDATA%: the task runs this exe elevated with no prompt, so it must sit
# where only admins can write. A user-writable copy would be a free silent-admin hole.
$dst = Join-Path $env:ProgramFiles 'svtoggle'
$old = Join-Path $env:LOCALAPPDATA 'Programs\svtoggle' # pre-0.2.1 location
$exe = "$dst\svtoggle-gui.exe"
$task = 'svtoggle'
$lnks = foreach ($d in [Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs')) { Join-Path $d 'svtoggle.lnk' }
$self = $MyInvocation.MyCommand.Path

if (-not $Uninstall) { "Revert: powershell -File `"$self`" -Uninstall" }

$elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $elevated) {
    $a = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$self`"", '-PauseOnError')
    if ($Uninstall) { $a += '-Uninstall' }
    if ($DryRun) { $a += '-DryRun' }
    if ($DryRun) { "[dry-run] not elevated: would re-launch elevated: powershell $($a -join ' ')" }
    else { $p = Start-Process powershell -Verb RunAs -ArgumentList $a -Wait -PassThru; exit $p.ExitCode }
}

if ($Uninstall) {
    $plan = @("delete scheduled task '$task'") + ($lnks | ForEach-Object { "delete $_" }) + "delete folders $dst, $old"
    if ($DryRun) { $plan | ForEach-Object { "[dry-run] would $_" }; exit }
    Unregister-ScheduledTask -TaskName $task -Confirm:$false -ErrorAction SilentlyContinue
    Remove-Item $lnks -Force -ErrorAction SilentlyContinue
    Remove-Item $dst, $old -Recurse -Force -ErrorAction SilentlyContinue
    "uninstalled (firewall rules untouched; to drop them: Get-NetFirewallRule -Group svtoggle | Remove-NetFirewallRule)"
    exit
}

if ($DryRun) {
    "[dry-run] would copy $src\svtoggle-gui.exe, svtoggle.exe -> $dst"
    "[dry-run] would register task '$task': run $exe as $env:USERDOMAIN\$env:USERNAME, Highest, interactive, no trigger, on-demand, runs on battery"
    foreach ($l in $lnks) { "[dry-run] would create shortcut $l -> C:\Windows\System32\schtasks.exe /run /tn $task (icon $exe, minimized)" }
    exit
}

New-Item -ItemType Directory -Force $dst | Out-Null
Copy-Item "$src\svtoggle-gui.exe", "$src\svtoggle.exe" $dst -Force
Remove-Item $old -Recurse -Force -ErrorAction SilentlyContinue

$action = New-ScheduledTaskAction -Execute $exe -WorkingDirectory $dst
$user = "$env:USERDOMAIN\$env:USERNAME"
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
Register-ScheduledTask -TaskName $task -Action $action -Principal $principal -Settings $settings -Force | Out-Null
"task: $task"

$sh = New-Object -ComObject WScript.Shell
foreach ($lnk in $lnks) {
    $s = $sh.CreateShortcut($lnk)
    $s.TargetPath = 'C:\Windows\System32\schtasks.exe'
    $s.Arguments = "/run /tn $task"
    $s.WorkingDirectory = $dst
    $s.IconLocation = "$exe,0"
    $s.WindowStyle = 7 # minimized, so the schtasks console doesn't flash
    $s.Description = 'Block Overwatch / Deadlock server regions'
    $s.Save()
    "shortcut: $lnk"
}
"installed: $dst"

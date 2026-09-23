# Build (optional), then replace the per-user installation in place and restart the board.
param([switch]$SkipBuild, [switch]$StopMcp)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)

$version = (Get-Content -LiteralPath package.json -Raw | ConvertFrom-Json).version
if (-not $SkipBuild) { & "$PSScriptRoot/build.ps1" }
$installer = "release/AgentKanban_${version}_x64-setup.exe"
if (-not (Test-Path -LiteralPath $installer)) { throw "Installer not found: $installer. Run without -SkipBuild." }

$installDir = Join-Path $env:LOCALAPPDATA 'AgentKanban'
function Get-Installed([string]$name) {
  Get-Process $name -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($installDir, [StringComparison]::OrdinalIgnoreCase) }
}

# An Agent session holding the MCP locks its executable, so the installer could not replace it.
$mcp = @(Get-Installed 'agentkanban-mcp')
if ($mcp.Count -gt 0) {
  if (-not $StopMcp) {
    throw "AgentKanban MCP is running for an Agent session (PID $($mcp.Id -join ', ')). Close those sessions, or rerun with -StopMcp to end them (their board tools disconnect until the session restarts)."
  }
  $mcp | Stop-Process -Force
}

# The window only hides to the tray on close, so end the process. Settings are saved continuously.
$app = @(Get-Installed 'agentkanban')
$app | Stop-Process -Force
$app | ForEach-Object { $_.WaitForExit(5000) | Out-Null }

$setup = Start-Process -FilePath $installer -ArgumentList '/S' -Wait -PassThru
if ($setup.ExitCode -ne 0) { throw "Installer exited with $($setup.ExitCode)" }

# The installer keeps build timestamps (to about a second), so a match shows the installed MCP is
# this build. Tauri rewrites agentkanban.exe after bundling, so only the MCP is comparable.
$built = (Get-Item -LiteralPath 'target/release/agentkanban-mcp.exe').LastWriteTimeUtc
$installed = (Get-Item -LiteralPath (Join-Path $installDir 'agentkanban-mcp.exe')).LastWriteTimeUtc
if ([Math]::Abs(($built - $installed).TotalSeconds) -gt 2) {
  throw "Installed agentkanban-mcp.exe is not this build (installed $installed, built $built)"
}
$reported = & (Join-Path $installDir 'agentkanban-mcp.exe') --version
if ($reported -ne "agentkanban-mcp $version") { throw "Unexpected MCP version: $reported" }

if ($app.Count -gt 0) { Start-Process -FilePath (Join-Path $installDir 'agentkanban.exe') }
Write-Output "Updated AgentKanban $version in $installDir$(if ($app.Count -gt 0) { '; board restarted' })"

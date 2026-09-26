# Build (optional), then replace the per-user installation in place and restart the board.
param([switch]$SkipBuild, [switch]$StopMcp)

function Get-Installed([string]$name, [string]$directory) {
  if (-not (Test-Path -LiteralPath $directory -PathType Container)) { return }
  $resolvedDirectory = (Resolve-Path -LiteralPath $directory).ProviderPath.TrimEnd('\', '/')
  Get-Process $name -ErrorAction SilentlyContinue | Where-Object {
    if (-not $_.Path) { return $false }
    $executable = Resolve-Path -LiteralPath $_.Path -ErrorAction SilentlyContinue
    $executable -and [IO.Path]::GetDirectoryName($executable.ProviderPath).Equals(
      $resolvedDirectory, [StringComparison]::OrdinalIgnoreCase)
  }
}

function Get-McpVersion([string]$executable) {
  $reported = & $executable --version
  if ($LASTEXITCODE -ne 0) { throw "MCP version check failed: $executable" }
  return $reported
}

function Invoke-AgentKanbanUpdate([switch]$SkipBuild, [switch]$StopMcp) {
  $ErrorActionPreference = 'Stop'
  Push-Location (Split-Path $PSScriptRoot -Parent)
  try {
    $version = (Get-Content -LiteralPath package.json -Raw | ConvertFrom-Json).version
    if (-not $SkipBuild) { & "$PSScriptRoot/build.ps1" }
    $installer = "release/AgentKanban_${version}_x64-setup.exe"
    if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
      throw "Installer not found: $installer. Run without -SkipBuild."
    }
    $installer = (Resolve-Path -LiteralPath $installer).ProviderPath
    $expectedVersion = "agentkanban-mcp $version"
    $builtAt = $null
    if (-not $SkipBuild) {
      # Validate all required build inputs before ending any process. SkipBuild
      # installs the selected release package and never depends on target/.
      $builtFile = Get-Item -LiteralPath 'target/release/agentkanban-mcp.exe'
      if ((Get-McpVersion $builtFile.FullName) -ne $expectedVersion) {
        throw "Built MCP version does not match package.json ($version)"
      }
      $builtAt = $builtFile.LastWriteTimeUtc
    }

    $installDir = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'AgentKanban'))
    $mcp = @(Get-Installed 'agentkanban-mcp' $installDir)
    if ($mcp.Count -gt 0 -and -not $StopMcp) {
      throw "AgentKanban MCP is running for an Agent session (PID $($mcp.Id -join ', ')). Close those sessions, or rerun with -StopMcp to end them (their board tools disconnect until the session restarts)."
    }
    $app = @(Get-Installed 'agentkanban' $installDir)
    $restoreBoard = $false
    $failure = $null
    try {
      # Only executables directly in the resolved installation directory match.
      # Sibling portable/test installations must remain untouched.
      foreach ($process in $mcp) {
        $process | Stop-Process -Force
        if (-not $process.WaitForExit(5000)) { throw "MCP process $($process.Id) did not exit" }
      }
      $restoreBoard = $app.Count -gt 0
      foreach ($process in $app) {
        $process | Stop-Process -Force
        if (-not $process.WaitForExit(5000)) { throw "Board process $($process.Id) did not exit" }
      }

      $setup = Start-Process -FilePath $installer -ArgumentList '/S' -WindowStyle Hidden -Wait -PassThru
      if ($setup.ExitCode -ne 0) { throw "Installer exited with $($setup.ExitCode)" }
      $installedMcp = Join-Path $installDir 'agentkanban-mcp.exe'
      $installedFile = Get-Item -LiteralPath $installedMcp
      if ($null -ne $builtAt -and [Math]::Abs(($builtAt - $installedFile.LastWriteTimeUtc).TotalSeconds) -gt 2) {
        throw "Installed agentkanban-mcp.exe is not this build (installed $($installedFile.LastWriteTimeUtc), built $builtAt)"
      }
      $reported = Get-McpVersion $installedMcp
      if ($reported -ne $expectedVersion) { throw "Unexpected MCP version: $reported" }
    } catch {
      $failure = $_
    } finally {
      if ($restoreBoard) {
        try {
          if (@(Get-Installed 'agentkanban' $installDir).Count -eq 0) {
            $installedApp = Join-Path $installDir 'agentkanban.exe'
            if (-not (Test-Path -LiteralPath $installedApp -PathType Leaf)) {
              throw "Cannot restore the board: $installedApp is missing"
            }
            Start-Process -FilePath $installedApp -WindowStyle Hidden
          }
        } catch {
          if ($null -eq $failure) { $failure = $_ }
          else { Write-Warning "The update failed and the previous board could not be reopened: $_" }
        }
      }
    }
    if ($null -ne $failure) { throw $failure }
    Write-Output "Updated AgentKanban $version in $installDir$(if ($restoreBoard) { '; board restarted' })"
  } finally {
    Pop-Location
  }
}

# Dot-sourcing loads helpers for isolated script tests without installing anything.
if ($MyInvocation.InvocationName -ne '.') {
  Invoke-AgentKanbanUpdate -SkipBuild:$SkipBuild -StopMcp:$StopMcp
}

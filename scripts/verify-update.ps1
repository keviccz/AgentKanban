# Exercise the real update script with temporary files and in-process command
# doubles. No Windows process or installer is ever started or terminated.
param([switch]$KeepArtifacts)
$ErrorActionPreference = 'Stop'
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('agentkanban-update-test-' + [guid]::NewGuid().ToString('N'))
$project = Join-Path $testRoot 'project'
$localData = Join-Path $testRoot 'local-data'
$previousLocalData = $env:LOCALAPPDATA
$passed = $false
$script:events = [Collections.Generic.List[string]]::new()
$script:processes = @()
$script:installerExitCode = 0
$script:reportedVersion = 'agentkanban-mcp 99.1.0'
$script:versionFailure = $false

function Assert-Check([bool]$condition, [string]$message) {
  if (-not $condition) { throw $message }
}

function New-FakeProcess([int]$id, [string]$name, [string]$directory) {
  $path = Join-Path $directory "$name.exe"
  $null = New-Item -ItemType Directory -Path $directory -Force
  [IO.File]::WriteAllText($path, 'test fixture: never execute')
  $process = [pscustomobject]@{ Id = $id; Name = $name; Path = $path; Running = $true; AuditFake = $true }
  $process | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($timeout) return -not $this.Running }
  return $process
}

function Reset-Fixtures([switch]$WithMcp) {
  $script:events.Clear()
  $script:installerExitCode = 0
  $script:reportedVersion = 'agentkanban-mcp 99.1.0'
  $script:versionFailure = $false
  $script:processes = @(
    (New-FakeProcess 101 'agentkanban' (Join-Path $localData 'AgentKanban')),
    (New-FakeProcess 201 'agentkanban' (Join-Path $localData 'AgentKanban-test')),
    (New-FakeProcess 202 'agentkanban-mcp' (Join-Path $localData 'AgentKanban-test'))
  )
  if ($WithMcp) { $script:processes += New-FakeProcess 102 'agentkanban-mcp' (Join-Path $localData 'AgentKanban') }
  [IO.File]::WriteAllText((Join-Path $localData 'AgentKanban/agentkanban-mcp.exe'), 'test fixture: never execute')
}

function Expect-UpdateFailure([scriptblock]$action, [string]$message) {
  $failure = $null
  try { & $action | Out-Null } catch { $failure = $_ }
  Assert-Check ($null -ne $failure -and "$failure" -like "*$message*") "Expected update failure containing '$message'; got '$failure'"
}

try {
  $null = New-Item -ItemType Directory -Path (Join-Path $project 'scripts'), (Join-Path $project 'release') -Force
  Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'update.ps1') -Destination (Join-Path $project 'scripts/update.ps1')
  [IO.File]::WriteAllText((Join-Path $project 'package.json'), '{"version":"99.1.0"}')
  [IO.File]::WriteAllText((Join-Path $project 'release/AgentKanban_99.1.0_x64-setup.exe'), 'test fixture: never execute')
  [IO.File]::WriteAllText((Join-Path $project 'scripts/build.ps1'), '# Mock build intentionally leaves target absent.')
  $env:LOCALAPPDATA = $localData
  . (Join-Path $project 'scripts/update.ps1')

  # Every process operation is shadowed before invoking production helpers.
  function Get-Process {
    [CmdletBinding()] param([string]$Name)
    $script:processes | Where-Object { $_.Running -and $_.Name -eq $Name }
  }
  function Stop-Process {
    [CmdletBinding()] param([Parameter(ValueFromPipeline)]$InputObject, [switch]$Force)
    process {
      Assert-Check ($InputObject.AuditFake -eq $true) 'Refusing to stop a non-test process'
      $script:events.Add("stop:$($InputObject.Id)")
      $InputObject.Running = $false
    }
  }
  function Start-Process {
    [CmdletBinding()] param([string]$FilePath, [string]$ArgumentList, [string]$WindowStyle, [switch]$Wait, [switch]$PassThru)
    Assert-Check ($FilePath.StartsWith($testRoot + [IO.Path]::DirectorySeparatorChar)) 'Refusing to start a path outside test fixtures'
    if ($FilePath.EndsWith('-setup.exe')) {
      $script:events.Add('install')
      return [pscustomobject]@{ ExitCode = $script:installerExitCode }
    }
    $script:events.Add('restart')
    $script:processes | Where-Object { $_.Id -eq 101 } | ForEach-Object { $_.Running = $true }
  }
  function Get-McpVersion([string]$executable) {
    Assert-Check ($executable.StartsWith($testRoot + [IO.Path]::DirectorySeparatorChar)) 'Refusing to inspect a non-test executable'
    if ($script:versionFailure) { throw 'injected version check failure' }
    return $script:reportedVersion
  }

  Reset-Fixtures -WithMcp
  $installDir = Join-Path $localData 'AgentKanban'
  Assert-Check (@(Get-Installed 'agentkanban' $installDir).Id -eq 101) 'Sibling GUI must not match the installation'
  Assert-Check (@(Get-Installed 'agentkanban-mcp' $installDir).Id -eq 102) 'Sibling MCP must not match the installation'

  Reset-Fixtures
  Invoke-AgentKanbanUpdate -SkipBuild | Out-Null
  Assert-Check (-not (Test-Path -LiteralPath (Join-Path $project 'target'))) 'SkipBuild test must have no target directory'
  Assert-Check (($script:events -join ',') -eq 'stop:101,install,restart') 'SkipBuild must succeed without target and preserve sibling processes'

  Reset-Fixtures
  $script:installerExitCode = 9
  Expect-UpdateFailure { Invoke-AgentKanbanUpdate -SkipBuild } 'Installer exited with 9'
  Assert-Check (($script:events -join ',') -eq 'stop:101,install,restart') 'Installer failure must restore the board'

  Reset-Fixtures
  $script:reportedVersion = 'agentkanban-mcp old'
  Expect-UpdateFailure { Invoke-AgentKanbanUpdate -SkipBuild } 'Unexpected MCP version'
  Assert-Check (($script:events -join ',') -eq 'stop:101,install,restart') 'Version mismatch must restore the board'

  Reset-Fixtures
  $script:versionFailure = $true
  Expect-UpdateFailure { Invoke-AgentKanbanUpdate -SkipBuild } 'injected version check failure'
  Assert-Check (($script:events -join ',') -eq 'stop:101,install,restart') 'Version check exception must restore the board'

  Reset-Fixtures -WithMcp
  Expect-UpdateFailure { Invoke-AgentKanbanUpdate -SkipBuild } 'MCP is running'
  Assert-Check ($script:events.Count -eq 0) 'Active MCP must stop the update before any side effect'

  Reset-Fixtures -WithMcp
  Invoke-AgentKanbanUpdate -SkipBuild -StopMcp | Out-Null
  Assert-Check (($script:events -join ',') -eq 'stop:102,stop:101,install,restart') 'StopMcp must stop only the installed MCP and GUI'
  Assert-Check (($script:processes | Where-Object { $_.Id -eq 202 }).Running) 'Sibling MCP must remain running'

  Reset-Fixtures
  Expect-UpdateFailure { Invoke-AgentKanbanUpdate } 'target/release/agentkanban-mcp.exe'
  Assert-Check ($script:events.Count -eq 0) 'Missing required build input must fail before any process is stopped'

  $passed = $true
  [ordered]@{ ok = $true; scenarios = 8; process_actions_mocked = $true; installer_actions_mocked = $true } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $testRoot 'results.json') -Encoding utf8
  Write-Output 'UPDATE_TESTS_OK 8 scenarios; all process and installer actions mocked'
} finally {
  $env:LOCALAPPDATA = $previousLocalData
  if ($passed -and -not $KeepArtifacts) {
    $resolvedRoot = (Resolve-Path -LiteralPath $testRoot).ProviderPath
    $expectedRoot = [IO.Path]::GetFullPath($testRoot)
    if ($resolvedRoot -ne $expectedRoot -or -not ([IO.Path]::GetFileName($resolvedRoot).StartsWith('agentkanban-update-test-'))) {
      throw "Refusing cleanup outside the exact test directory: $resolvedRoot"
    }
    Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
  } else {
    Write-Output "UPDATE_TEST_ROOT=$testRoot"
  }
}

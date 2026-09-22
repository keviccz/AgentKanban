[CmdletBinding()]
param(
    [string]$Executable = (Join-Path $env:LOCALAPPDATA 'AgentKanban\agentkanban-mcp.exe'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\artifacts\client-config'),
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$binaryPath = [System.IO.Path]::GetFullPath($Executable)
if (-not (Test-Path -LiteralPath $binaryPath -PathType Leaf)) {
    throw "MCP executable not found: $binaryPath. Pass -Executable with the installed or built executable."
}
$outputPath = [System.IO.Path]::GetFullPath($OutputDirectory)
$null = New-Item -ItemType Directory -Path $outputPath -Force
$tomlPath = $binaryPath.Replace('\', '/')
$tomlCommand = $tomlPath | ConvertTo-Json -Compress
$server = [ordered]@{ type = 'stdio'; command = $binaryPath; args = @() }
$json = [ordered]@{ mcpServers = [ordered]@{ agentkanban = $server } } | ConvertTo-Json -Depth 5
$files = [ordered]@{
    'codex.toml' = "[mcp_servers.agentkanban]`ncommand = $tomlCommand`nargs = []`n"
    'claude-code.mcp.json' = "$json`n"
    'cursor.mcp.json' = "$json`n"
}
foreach ($name in $files.Keys) {
    $destination = Join-Path $outputPath $name
    if ((Test-Path -LiteralPath $destination) -and -not $Force) {
        throw "Output already exists: $destination. Choose another -OutputDirectory or use -Force."
    }
}
foreach ($name in $files.Keys) {
    $destination = Join-Path $outputPath $name
    [System.IO.File]::WriteAllText($destination, $files[$name], [System.Text.UTF8Encoding]::new($false))
    Write-Output $destination
}
Write-Output 'Examples only; no Codex, Claude Code, or Cursor configuration was changed.'

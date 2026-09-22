param([switch]$Release)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$cargoArgs = @('build', '-p', 'agentkanban-mcp')
if ($Release) { $cargoArgs += '--release' }
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { throw 'MCP build failed' }
$targetTriple = ((& rustc -vV) | Select-String '^host: ').ToString().Substring(6).Trim()
$profile = if ($Release) { 'release' } else { 'debug' }
New-Item -ItemType Directory -Path src-tauri/binaries -Force | Out-Null
Copy-Item -LiteralPath "target/$profile/agentkanban-mcp.exe" -Destination "src-tauri/binaries/agentkanban-mcp-$targetTriple.exe" -Force

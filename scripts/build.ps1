$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
& "$PSScriptRoot/prepare-mcp.ps1" -Release
& npm exec tauri build
if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed' }
New-Item -ItemType Directory -Path release/AgentKanban -Force | Out-Null
Copy-Item -LiteralPath target/release/agentkanban.exe,target/release/agentkanban-mcp.exe,README.md -Destination release/AgentKanban -Force
Copy-Item -LiteralPath examples,docs -Destination release/AgentKanban -Recurse -Force
New-Item -ItemType Directory -Path release/AgentKanban/scripts -Force | Out-Null
Copy-Item -LiteralPath scripts/write-client-examples.ps1 -Destination release/AgentKanban/scripts -Force
$installer = Get-ChildItem -LiteralPath target/release/bundle/nsis -Filter '*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $installer) { throw 'NSIS installer not found' }
Copy-Item -LiteralPath $installer.FullName -Destination release -Force
Compress-Archive -Path release/AgentKanban -DestinationPath release/AgentKanban-0.1.0-windows-x64.zip -Force
Write-Output "Installer: release/$($installer.Name)"
Write-Output 'Portable: release/AgentKanban-0.1.0-windows-x64.zip'

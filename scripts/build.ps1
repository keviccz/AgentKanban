param(
  [string]$Tag,
  [string]$Repository = 'keviccz/AgentKanban',
  [string]$NotesFile,
  [switch]$RequireSignature
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$releaseDir = Join-Path $projectRoot 'release'
$manifestPath = Join-Path $releaseDir 'latest.json'
$overridePath = Join-Path ([IO.Path]::GetTempPath()) ("agentkanban-build-" + [guid]::NewGuid().ToString('N') + '.json')
Push-Location $projectRoot
try {
  # A failed or unsigned build must never leave a previous update advertised.
  if (Test-Path -LiteralPath $manifestPath) {
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'release/latest.json must be a file' }
    Remove-Item -LiteralPath $manifestPath -Force
  }
  $version = (Get-Content -LiteralPath package.json -Raw -Encoding utf8 | ConvertFrom-Json).version
  try { $parsedVersion = [semver]$version } catch { throw "Invalid package version: $version" }
  if ($parsedVersion.ToString() -cne $version) { throw "Package version must be canonical SemVer: $version" }
  if (-not $Tag) { $Tag = "v$version" }
  if ($Tag -cne "v$version") { throw "Release tag '$Tag' must equal package version 'v$version'" }

  $config = Get-Content -LiteralPath src-tauri/tauri.conf.json -Raw -Encoding utf8 | ConvertFrom-Json
  if ($config.version -cne $version) { throw 'Tauri and package versions must match' }
  if ($config.productName -cne 'AgentKanban') { throw 'Expected the AgentKanban product name for Windows release assets' }
  foreach ($cargoPath in @('src-tauri/Cargo.toml', 'crates/kanban-core/Cargo.toml', 'crates/kanban-mcp/Cargo.toml')) {
    $cargoText = Get-Content -LiteralPath $cargoPath -Raw -Encoding utf8
    $packageSection = [regex]::Match($cargoText, '(?ms)^\[package\]\s*\r?\n(?<body>.*?)(?=^\[|\z)').Groups['body'].Value
    $cargoVersion = [regex]::Match($packageSection, '(?m)^version\s*=\s*"([^"]+)"\s*(?:#.*)?$').Groups[1].Value
    if ($cargoVersion -cne $version) { throw "Cargo and package versions must match: $cargoPath" }
  }
  $notes = "AgentKanban $version"
  if (-not $NotesFile) { $NotesFile = Join-Path $projectRoot 'docs/RELEASE_NOTES.md' }
  if (Test-Path -LiteralPath $NotesFile -PathType Leaf) {
    $notes = Get-Content -LiteralPath $NotesFile -Raw -Encoding utf8
  } elseif ($PSBoundParameters.ContainsKey('NotesFile')) {
    throw "Release notes not found: $NotesFile"
  }

  $signed = -not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)
  if ($RequireSignature -and -not $signed) {
    throw 'TAURI_SIGNING_PRIVATE_KEY is required for a signed release. Ordinary desktop:build can run without it.'
  }
  $installerName = "AgentKanban_${version}_x64-setup.exe"
  $installerPath = Join-Path $projectRoot "target/release/bundle/nsis/$installerName"
  $releasedInstaller = Join-Path $releaseDir $installerName
  $archive = Join-Path $releaseDir "AgentKanban-$version-windows-x64.zip"
  # Only exact, generated paths for this version are removed; other releases stay intact.
  foreach ($generatedFile in @($installerPath, "$installerPath.sig", $releasedInstaller, "$releasedInstaller.sig", $archive)) {
    if (Test-Path -LiteralPath $generatedFile) { Remove-Item -LiteralPath $generatedFile -Force }
  }

  & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'prepare-mcp.ps1') -Release
  if ($LASTEXITCODE -ne 0) { throw "MCP preparation failed with exit code $LASTEXITCODE" }

  # Keep unsigned local packaging usable even when the checked-in config enables updates.
  @{ bundle = @{ createUpdaterArtifacts = $signed } } | ConvertTo-Json -Depth 3 |
    Set-Content -LiteralPath $overridePath -Encoding utf8
  if (-not $signed) { Write-Output 'No signing key: building an ordinary installer and portable ZIP; no update manifest will be generated.' }
  & npm exec -- tauri build --config $overridePath
  if ($LASTEXITCODE -ne 0) { throw "Desktop build failed with exit code $LASTEXITCODE" }
  if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) { throw "NSIS installer not found: $installerName" }

  New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
  Copy-Item -LiteralPath $installerPath -Destination $releasedInstaller -Force
  if ($signed) {
    if (-not (Test-Path -LiteralPath "$installerPath.sig" -PathType Leaf)) { throw "Signed build did not produce $installerName.sig" }
    Copy-Item -LiteralPath "$installerPath.sig" -Destination "$releasedInstaller.sig" -Force
    & node (Join-Path $PSScriptRoot 'verify-update-signature.mjs') --installer $releasedInstaller --config (Join-Path $projectRoot 'src-tauri/tauri.conf.json') --version $version
    if ($LASTEXITCODE -ne 0) { throw 'Updater signature verification failed; refusing to generate latest.json' }
    & (Join-Path $PSScriptRoot 'create-update-manifest.ps1') -InstallerPath $releasedInstaller -Version $version -Tag $Tag -Repository $Repository -OutputPath $manifestPath -Notes $notes
  }

  $portableDir = [IO.Path]::GetFullPath((Join-Path $releaseDir 'AgentKanban'))
  $expectedPortable = [IO.Path]::GetFullPath((Join-Path $projectRoot 'release/AgentKanban'))
  if ($portableDir -ne $expectedPortable -or -not $portableDir.StartsWith([IO.Path]::GetFullPath($releaseDir) + [IO.Path]::DirectorySeparatorChar)) {
    throw 'Refusing to clean a portable directory outside the release directory'
  }
  if (Test-Path -LiteralPath $portableDir) { Remove-Item -LiteralPath $portableDir -Recurse -Force }
  New-Item -ItemType Directory -Path (Join-Path $portableDir 'scripts') -Force | Out-Null
  Copy-Item -LiteralPath target/release/agentkanban.exe,target/release/agentkanban-mcp.exe,README.md -Destination $portableDir -Force
  Copy-Item -LiteralPath examples,docs -Destination $portableDir -Recurse -Force
  Copy-Item -LiteralPath scripts/write-client-examples.ps1 -Destination (Join-Path $portableDir 'scripts') -Force
  Compress-Archive -LiteralPath $portableDir -DestinationPath $archive -Force
  Write-Output "Installer: $releasedInstaller"
  Write-Output "Portable: $archive"
} catch {
  if (Test-Path -LiteralPath $manifestPath -PathType Leaf) { Remove-Item -LiteralPath $manifestPath -Force }
  throw
} finally {
  if (Test-Path -LiteralPath $overridePath -PathType Leaf) { Remove-Item -LiteralPath $overridePath -Force }
  Pop-Location
}

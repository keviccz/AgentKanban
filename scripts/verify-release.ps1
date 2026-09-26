# Offline regression tests: npm/Tauri and MCP preparation are test doubles.
# Signature checks run for real using public official vectors. No production
# signing key, real build, installer execution, or GitHub request is used.
param([switch]$KeepArtifacts)
$ErrorActionPreference = 'Stop'
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('agentkanban-release-test-' + [guid]::NewGuid().ToString('N'))
$previousKey = $env:TAURI_SIGNING_PRIVATE_KEY
$previousPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# Test-only names cross the nested production-script scope used by the npm double.
$global:AgentKanbanReleaseTest_cases = [Collections.Generic.List[object]]::new()
$global:AgentKanbanReleaseTest_assertions = 0
$global:AgentKanbanReleaseTest_fixtureVersion = '99.2.3'
$global:AgentKanbanReleaseTest_fixturePublicKey = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("untrusted comment: minisign public key`nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3`n"))
# Official minisign-verify 0.2.5 vector authenticates the four bytes 'test'.
$global:AgentKanbanReleaseTest_fixtureSignature = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("untrusted comment: signature from minisign secret key`nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=`ntrusted comment: timestamp:1633700835`tfile:test`tprehashed`nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==`n"))
$passed = $false
New-Item -ItemType Directory -Path $testRoot -Force | Out-Null

function Assert-Check([bool]$Condition, [string]$Message) {
  if (-not $Condition) { throw $Message }
  $global:AgentKanbanReleaseTest_assertions++
}

function Write-Fixture([string]$RelativePath, [string]$Content) {
  $path = Join-Path $global:AgentKanbanReleaseTest_project $RelativePath
  New-Item -ItemType Directory -Path (Split-Path $path -Parent) -Force | Out-Null
  [IO.File]::WriteAllText($path, $Content)
}

function New-Fixture([string]$Name) {
  $global:AgentKanbanReleaseTest_project = Join-Path $testRoot $Name
  $global:AgentKanbanReleaseTest_bundleBehavior = 'normal'
  $global:AgentKanbanReleaseTest_observedSigning = $null
  $env:TAURI_SIGNING_PRIVATE_KEY = $null
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $null
  New-Item -ItemType Directory -Path (Join-Path $global:AgentKanbanReleaseTest_project 'scripts') -Force | Out-Null
  foreach ($file in @('build.ps1', 'create-update-manifest.ps1', 'verify-update-signature.mjs')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination (Join-Path $global:AgentKanbanReleaseTest_project "scripts/$file")
  }
  Write-Fixture 'scripts/prepare-mcp.ps1' @'
param([switch]$Release)
Add-Content -LiteralPath (Join-Path (Split-Path $PSScriptRoot -Parent) 'events.txt') -Value 'prepare'
exit 0
'@
  Write-Fixture 'package.json' (@{ version = $global:AgentKanbanReleaseTest_fixtureVersion } | ConvertTo-Json)
  Write-Fixture 'src-tauri/tauri.conf.json' (@{
    productName = 'AgentKanban'; version = $global:AgentKanbanReleaseTest_fixtureVersion; bundle = @{ createUpdaterArtifacts = $true }
    plugins = @{ updater = @{ pubkey = $global:AgentKanbanReleaseTest_fixturePublicKey } }
  } | ConvertTo-Json -Depth 4)
  foreach ($path in @('src-tauri/Cargo.toml', 'crates/kanban-core/Cargo.toml', 'crates/kanban-mcp/Cargo.toml')) {
    Write-Fixture $path ("[package]`nname = `"fixture`"`nversion = `"$global:AgentKanbanReleaseTest_fixtureVersion`"`n`n[dependencies]`n")
  }
  Write-Fixture 'target/release/agentkanban.exe' 'OFFLINE FIXTURE: never execute'
  Write-Fixture 'target/release/agentkanban-mcp.exe' 'OFFLINE FIXTURE: never execute'
  Write-Fixture 'README.md' 'portable documentation fixture'
  Write-Fixture 'docs/README.md' 'docs fixture'
  Write-Fixture 'examples/README.md' 'examples fixture'
  Write-Fixture 'scripts/write-client-examples.ps1' '# fixture'
  $global:AgentKanbanReleaseTest_installerRelative = "target/release/bundle/nsis/AgentKanban_$($global:AgentKanbanReleaseTest_fixtureVersion)_x64-setup.exe"
  $global:AgentKanbanReleaseTest_releasedRelative = "release/AgentKanban_$($global:AgentKanbanReleaseTest_fixtureVersion)_x64-setup.exe"
  $global:AgentKanbanReleaseTest_manifestPath = Join-Path $global:AgentKanbanReleaseTest_project 'release/latest.json'
}

function Seed-OldArtifacts {
  foreach ($path in @($global:AgentKanbanReleaseTest_installerRelative, "$global:AgentKanbanReleaseTest_installerRelative.sig", $global:AgentKanbanReleaseTest_releasedRelative, "$global:AgentKanbanReleaseTest_releasedRelative.sig", 'release/latest.json')) {
    Write-Fixture $path 'stale: must not be reused'
  }
  Write-Fixture 'release/AgentKanban/stale.txt' 'must not enter the new portable archive'
  Write-Fixture 'target/release/bundle/nsis/AgentKanban_1.0.0_x64-setup.exe' 'different version: preserve'
}

# This function shadows npm only inside this isolated test PowerShell process.
function npm {
  param([Parameter(ValueFromRemainingArguments)][string[]]$Arguments)
  $configIndex = [Array]::IndexOf($Arguments, '--config')
  Assert-Check ($configIndex -ge 0) 'Tauri must receive the explicit signing config override'
  $override = Get-Content -LiteralPath $Arguments[$configIndex + 1] -Raw | ConvertFrom-Json
  $global:AgentKanbanReleaseTest_observedSigning = $override.bundle.createUpdaterArtifacts
  Assert-Check ((Get-Content -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project 'events.txt') -Raw).Trim() -eq 'prepare') 'MCP must finish before Tauri starts'
  Assert-Check (-not (Test-Path -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project $global:AgentKanbanReleaseTest_installerRelative))) 'Old installer must be removed before Tauri'
  Assert-Check (-not (Test-Path -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project "$global:AgentKanbanReleaseTest_installerRelative.sig"))) 'Old signature must be removed before Tauri'
  Add-Content -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project 'events.txt') -Value 'tauri'
  $global:LASTEXITCODE = 0
  if ($global:AgentKanbanReleaseTest_bundleBehavior -eq 'failed') { $global:LASTEXITCODE = 11; return }
  if ($global:AgentKanbanReleaseTest_bundleBehavior -eq 'missing-installer') { return }
  Write-Fixture $global:AgentKanbanReleaseTest_installerRelative 'test'
  if ($global:AgentKanbanReleaseTest_observedSigning -and $global:AgentKanbanReleaseTest_bundleBehavior -ne 'missing-signature') {
    Write-Fixture "$global:AgentKanbanReleaseTest_installerRelative.sig" $global:AgentKanbanReleaseTest_fixtureSignature
  }
}

function Expect-Failure([scriptblock]$Action, [string]$Message) {
  $failure = $null
  try { & $Action | Out-Null } catch { $failure = $_ }
  Assert-Check ($null -ne $failure -and "$failure" -like "*$Message*") "Expected '$Message'; got '$failure'"
  Assert-Check (-not (Test-Path -LiteralPath $global:AgentKanbanReleaseTest_manifestPath)) 'A failed operation must not leave latest.json'
}

function Invoke-FixtureBuild([switch]$RequireSignature, [string]$Tag = "v$global:AgentKanbanReleaseTest_fixtureVersion") {
  & (Join-Path $global:AgentKanbanReleaseTest_project 'scripts/build.ps1') -Tag $Tag -RequireSignature:$RequireSignature | Out-Null
}

function Invoke-FixtureManifest([string]$Version = $global:AgentKanbanReleaseTest_fixtureVersion, [string]$Tag = "v$Version") {
  & (Join-Path $global:AgentKanbanReleaseTest_project 'scripts/create-update-manifest.ps1') -InstallerPath (Join-Path $global:AgentKanbanReleaseTest_project $global:AgentKanbanReleaseTest_installerRelative) -Version $Version -Tag $Tag -OutputPath $global:AgentKanbanReleaseTest_manifestPath | Out-Null
}

function Run-Case([string]$Name, [scriptblock]$Action) {
  New-Fixture $Name
  try {
    & $Action
    $global:AgentKanbanReleaseTest_cases.Add([ordered]@{ name = $Name; ok = $true })
  } catch {
    $global:AgentKanbanReleaseTest_cases.Add([ordered]@{ name = $Name; ok = $false; error = "$_" })
    throw
  }
}

try {
  & node --test (Join-Path $PSScriptRoot 'verify-update-signature.test.mjs') 2>&1 | Tee-Object -FilePath (Join-Path $testRoot 'signature-tests.log')
  if ($LASTEXITCODE -ne 0) { throw 'Signature verification tests failed' }
  Run-Case 'manifest-exact-asset' {
    Write-Fixture $global:AgentKanbanReleaseTest_installerRelative 'test'
    Write-Fixture "$global:AgentKanbanReleaseTest_installerRelative.sig" ($global:AgentKanbanReleaseTest_fixtureSignature + "`r`n")
    Invoke-FixtureManifest
    $manifest = Get-Content -LiteralPath $global:AgentKanbanReleaseTest_manifestPath -Raw | ConvertFrom-Json
    Assert-Check ($manifest.version -ceq $global:AgentKanbanReleaseTest_fixtureVersion) 'Manifest must preserve the exact version'
    Assert-Check ($manifest.platforms.'windows-x86_64'.signature -ceq $global:AgentKanbanReleaseTest_fixtureSignature) 'Manifest must use the .sig content'
    Assert-Check ($manifest.platforms.'windows-x86_64'.url -ceq "https://github.com/keviccz/AgentKanban/releases/download/v$global:AgentKanbanReleaseTest_fixtureVersion/AgentKanban_$($global:AgentKanbanReleaseTest_fixtureVersion)_x64-setup.exe") 'Manifest must target this exact tag and original filename'
    Assert-Check ((Get-Content -LiteralPath $global:AgentKanbanReleaseTest_manifestPath -Raw) -match '"pub_date":\s*"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ"') 'Publication date must be RFC3339 UTC'
  }
  Run-Case 'manifest-url-encoding' {
    $encodedVersion = '99.2.3-rc.1+build.7'
    $global:AgentKanbanReleaseTest_installerRelative = "target/release/bundle/nsis/AgentKanban_${encodedVersion}_x64-setup.exe"
    Write-Fixture $global:AgentKanbanReleaseTest_installerRelative 'test'
    Write-Fixture "$global:AgentKanbanReleaseTest_installerRelative.sig" $global:AgentKanbanReleaseTest_fixtureSignature
    Invoke-FixtureManifest -Version $encodedVersion
    $manifest = Get-Content -LiteralPath $global:AgentKanbanReleaseTest_manifestPath -Raw | ConvertFrom-Json
    Assert-Check ($manifest.platforms.'windows-x86_64'.url -ceq 'https://github.com/keviccz/AgentKanban/releases/download/v99.2.3-rc.1%2Bbuild.7/AgentKanban_99.2.3-rc.1%2Bbuild.7_x64-setup.exe') 'Both tag and filename must be URL encoded'
  }
  Run-Case 'missing-signature' {
    Write-Fixture $global:AgentKanbanReleaseTest_installerRelative 'installer fixture'
    Write-Fixture 'release/latest.json' 'stale'
    Expect-Failure { Invoke-FixtureManifest } 'signature is missing'
  }
  Run-Case 'empty-signature' {
    Write-Fixture $global:AgentKanbanReleaseTest_installerRelative 'installer fixture'
    Write-Fixture "$global:AgentKanbanReleaseTest_installerRelative.sig" ''
    Write-Fixture 'release/latest.json' 'stale'
    Expect-Failure { Invoke-FixtureManifest } 'signature is empty'
  }
  Run-Case 'malformed-signature' {
    Write-Fixture $global:AgentKanbanReleaseTest_installerRelative 'installer fixture'
    Write-Fixture "$global:AgentKanbanReleaseTest_installerRelative.sig" 'not a base64 signature!'
    Expect-Failure { Invoke-FixtureManifest } 'signature is not valid base64'
  }
  Run-Case 'manifest-tag-mismatch' {
    Write-Fixture 'release/latest.json' 'stale'
    Expect-Failure { Invoke-FixtureManifest -Tag 'v1.0.0' } 'must equal version'
  }
  Run-Case 'manifest-invalid-version' {
    Write-Fixture 'release/latest.json' 'stale'
    Expect-Failure { Invoke-FixtureManifest -Version 'not-semver' } 'Invalid update version'
  }
  Run-Case 'manifest-wrong-installer-version' {
    $global:AgentKanbanReleaseTest_installerRelative = 'target/release/bundle/nsis/AgentKanban_1.0.0_x64-setup.exe'
    Write-Fixture $global:AgentKanbanReleaseTest_installerRelative 'old installer'
    Write-Fixture "$global:AgentKanbanReleaseTest_installerRelative.sig" $global:AgentKanbanReleaseTest_fixtureSignature
    Expect-Failure { Invoke-FixtureManifest } 'exact version'
  }
  Run-Case 'direct-manifest-rejects-tampered-installer' {
    Write-Fixture $global:AgentKanbanReleaseTest_installerRelative 'tampered'
    Write-Fixture "$global:AgentKanbanReleaseTest_installerRelative.sig" $global:AgentKanbanReleaseTest_fixtureSignature
    Write-Fixture 'release/latest.json' 'stale'
    Expect-Failure { Invoke-FixtureManifest } 'Updater signature verification failed'
  }
  Run-Case 'signed-build-rejects-mismatched-public-key' {
    Seed-OldArtifacts
    $env:TAURI_SIGNING_PRIVATE_KEY = 'OFFLINE_TEST_ONLY_NOT_A_KEY'
    $config = Get-Content -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
    $publicText = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($global:AgentKanbanReleaseTest_fixturePublicKey))
    $publicLines = $publicText -split "`n"
    $keyPacket = [Convert]::FromBase64String($publicLines[1])
    $keyPacket[2] = $keyPacket[2] -bxor 1
    $publicLines[1] = [Convert]::ToBase64String($keyPacket)
    $config.plugins.updater.pubkey = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($publicLines -join "`n")))
    Write-Fixture 'src-tauri/tauri.conf.json' ($config | ConvertTo-Json -Depth 4)
    Expect-Failure { Invoke-FixtureBuild -RequireSignature } 'Updater signature verification failed'
  }
  Run-Case 'safe-manifest-output' {
    $keepPath = Join-Path $global:AgentKanbanReleaseTest_project 'keep.json'
    Write-Fixture 'keep.json' 'keep me'
    Expect-Failure {
      & (Join-Path $global:AgentKanbanReleaseTest_project 'scripts/create-update-manifest.ps1') -InstallerPath 'absent.exe' -Version $global:AgentKanbanReleaseTest_fixtureVersion -Tag "v$global:AgentKanbanReleaseTest_fixtureVersion" -OutputPath $keepPath
    } 'OutputPath must name latest.json'
    Assert-Check ((Get-Content -LiteralPath $keepPath -Raw) -ceq 'keep me') 'An unrelated output filename must remain untouched'
  }
  Run-Case 'unsigned-build-clears-stale-artifacts' {
    Seed-OldArtifacts
    Invoke-FixtureBuild
    Assert-Check ($global:AgentKanbanReleaseTest_observedSigning -eq $false) 'Unsigned local build must disable updater artifacts'
    Assert-Check (-not (Test-Path -LiteralPath $global:AgentKanbanReleaseTest_manifestPath)) 'Unsigned build must not produce a manifest'
    Assert-Check (-not (Test-Path -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project "$global:AgentKanbanReleaseTest_releasedRelative.sig"))) 'Unsigned build must not keep an old released signature'
    Assert-Check ((Get-Content -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project $global:AgentKanbanReleaseTest_releasedRelative) -Raw) -ceq 'test') 'Only the newly built exact installer may be packaged'
    Assert-Check (Test-Path -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project 'target/release/bundle/nsis/AgentKanban_1.0.0_x64-setup.exe')) 'Other versions must be preserved'
    $archive = [IO.Compression.ZipFile]::OpenRead((Join-Path $global:AgentKanbanReleaseTest_project "release/AgentKanban-$global:AgentKanbanReleaseTest_fixtureVersion-windows-x64.zip"))
    try {
      Assert-Check (@($archive.Entries | Where-Object FullName -like '*stale.txt').Count -eq 0) 'Portable ZIP must not include old staging files'
      Assert-Check (@($archive.Entries | Where-Object FullName -like '*agentkanban-mcp.exe').Count -eq 1) 'Portable ZIP must include MCP'
    } finally { $archive.Dispose() }
  }
  Run-Case 'signed-build-manifest' {
    Seed-OldArtifacts
    $env:TAURI_SIGNING_PRIVATE_KEY = 'OFFLINE_TEST_ONLY_NOT_A_KEY'
    Invoke-FixtureBuild -RequireSignature
    Assert-Check ($global:AgentKanbanReleaseTest_observedSigning -eq $true) 'Signed build must enable updater artifacts'
    $manifest = Get-Content -LiteralPath $global:AgentKanbanReleaseTest_manifestPath -Raw | ConvertFrom-Json
    Assert-Check ($manifest.platforms.'windows-x86_64'.signature -ceq $global:AgentKanbanReleaseTest_fixtureSignature) 'Signed build must use its fresh signature'
  }
  Run-Case 'signed-build-multiline-notes' {
    $env:TAURI_SIGNING_PRIVATE_KEY = 'OFFLINE_TEST_ONLY_NOT_A_KEY'
    $notes = "# AgentKanban v99.2.3`n`n- 新增签名更新。`n- 保留任务与用户备注。`n"
    Write-Fixture 'docs/RELEASE_NOTES.md' $notes
    Invoke-FixtureBuild -RequireSignature
    $manifest = Get-Content -LiteralPath $global:AgentKanbanReleaseTest_manifestPath -Raw | ConvertFrom-Json
    Assert-Check ($manifest.notes -ceq $notes) 'Manifest must preserve multiline Unicode release notes exactly'
  }
  Run-Case 'ci-missing-signing-key' {
    Write-Fixture 'release/latest.json' 'stale'
    Expect-Failure { Invoke-FixtureBuild -RequireSignature } 'TAURI_SIGNING_PRIVATE_KEY is required'
    Assert-Check (-not (Test-Path -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project 'events.txt'))) 'Missing release secret must fail before MCP or Tauri'
  }
  Run-Case 'prepare-exit-fails-build' {
    Seed-OldArtifacts
    Write-Fixture 'scripts/prepare-mcp.ps1' 'exit 7'
    Expect-Failure { Invoke-FixtureBuild } 'MCP preparation failed with exit code 7'
    Assert-Check ($null -eq $global:AgentKanbanReleaseTest_observedSigning) 'Tauri must not run after MCP preparation failure'
  }
  Run-Case 'tauri-exit-fails-build' {
    Seed-OldArtifacts
    $global:AgentKanbanReleaseTest_bundleBehavior = 'failed'
    Expect-Failure { Invoke-FixtureBuild } 'Desktop build failed with exit code 11'
  }
  Run-Case 'old-other-version-never-selected' {
    Seed-OldArtifacts
    $global:AgentKanbanReleaseTest_bundleBehavior = 'missing-installer'
    Expect-Failure { Invoke-FixtureBuild } "NSIS installer not found: AgentKanban_$($global:AgentKanbanReleaseTest_fixtureVersion)_x64-setup.exe"
  }
  Run-Case 'old-signature-never-selected' {
    Seed-OldArtifacts
    $env:TAURI_SIGNING_PRIVATE_KEY = 'OFFLINE_TEST_ONLY_NOT_A_KEY'
    $global:AgentKanbanReleaseTest_bundleBehavior = 'missing-signature'
    Expect-Failure { Invoke-FixtureBuild -RequireSignature } 'Signed build did not produce'
  }
  Run-Case 'portable-failure-clears-new-manifest' {
    $env:TAURI_SIGNING_PRIVATE_KEY = 'OFFLINE_TEST_ONLY_NOT_A_KEY'
    Remove-Item -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project 'target/release/agentkanban.exe')
    Expect-Failure { Invoke-FixtureBuild -RequireSignature } 'agentkanban.exe'
  }
  Run-Case 'build-tag-mismatch' {
    Write-Fixture 'release/latest.json' 'stale'
    Expect-Failure { Invoke-FixtureBuild -Tag 'v1.0.0' } 'must equal package version'
    Assert-Check (-not (Test-Path -LiteralPath (Join-Path $global:AgentKanbanReleaseTest_project 'events.txt'))) 'Wrong tag must fail before build'
  }
  Run-Case 'build-tauri-version-mismatch' {
    Write-Fixture 'src-tauri/tauri.conf.json' '{"version":"1.0.0","productName":"AgentKanban"}'
    Expect-Failure { Invoke-FixtureBuild } 'Tauri and package versions must match'
  }
  foreach ($cargoPath in @('src-tauri/Cargo.toml', 'crates/kanban-core/Cargo.toml', 'crates/kanban-mcp/Cargo.toml')) {
    Run-Case ('build-version-' + $cargoPath.Replace('/', '-')) {
      Write-Fixture $cargoPath "[package]`nversion = `"1.0.0`"`n"
      Expect-Failure { Invoke-FixtureBuild } "Cargo and package versions must match: $cargoPath"
    }
  }
  $passed = $true
  Write-Output "RELEASE_TESTS_OK $($global:AgentKanbanReleaseTest_cases.Count) scenarios, $global:AgentKanbanReleaseTest_assertions assertions; builds mocked, signature verification real"
} finally {
  $env:TAURI_SIGNING_PRIVATE_KEY = $previousKey
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $previousPassword
  [ordered]@{ ok = $passed; assertions = $global:AgentKanbanReleaseTest_assertions; cases = @($global:AgentKanbanReleaseTest_cases); builds_mocked = $true; release_signing_mocked = $true; signature_verification_real = $true; github_requests = 0 } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $testRoot 'results.json') -Encoding utf8
  if ($KeepArtifacts -or -not $passed) {
    Write-Output "Release test artifacts: $testRoot"
  } else {
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ((Split-Path $resolvedTestRoot -Parent) -ne $resolvedTemp -or (Split-Path $resolvedTestRoot -Leaf) -notlike 'agentkanban-release-test-*') { throw 'Refusing to clean outside the test directory' }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
  }
  Remove-Variable -Scope Global -Name 'AgentKanbanReleaseTest_*' -ErrorAction SilentlyContinue
}

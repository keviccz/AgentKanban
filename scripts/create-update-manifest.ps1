# Tauri v2 updates use the original NSIS installer and the content of its .sig.
# Verify the exact signed bytes against the public key that ships with this app.
param(
  [Parameter(Mandatory)][string]$InstallerPath,
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$Tag,
  [string]$Repository = 'keviccz/AgentKanban',
  [string]$OutputPath = (Join-Path (Split-Path $PSScriptRoot -Parent) 'release/latest.json'),
  [string]$Notes = "AgentKanban $Version"
)
$ErrorActionPreference = 'Stop'
$manifestPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputPath)
if ([IO.Path]::GetFileName($manifestPath) -cne 'latest.json') { throw 'OutputPath must name latest.json' }
if (Test-Path -LiteralPath $manifestPath) {
  if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'latest.json must be a file' }
  Remove-Item -LiteralPath $manifestPath -Force
}

try { $parsedVersion = [semver]$Version } catch { throw "Invalid update version: $Version" }
if ($parsedVersion.ToString() -cne $Version) { throw "Update version must be canonical SemVer: $Version" }
if ($Tag -cne "v$Version") { throw "Release tag '$Tag' must equal version 'v$Version'" }
if ($Repository -notmatch '^[A-Za-z0-9-]+/[A-Za-z0-9_.-]+$' -or ($Repository.Split('/')[1] -in @('.', '..'))) {
  throw 'Repository must be a GitHub owner/repository pair'
}
$installer = Get-Item -LiteralPath $InstallerPath
if ($installer.PSIsContainer -or $installer.Name -cne "AgentKanban_${Version}_x64-setup.exe" -or $installer.Length -eq 0) {
  throw 'Installer must be the non-empty AgentKanban NSIS installer for this exact version'
}
$signaturePath = "$($installer.FullName).sig"
if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) { throw 'Updater signature is missing; refusing to generate latest.json' }
$signature = Get-Content -LiteralPath $signaturePath -Raw -Encoding utf8
if ([string]::IsNullOrWhiteSpace($signature)) { throw 'Updater signature is empty; refusing to generate latest.json' }
$signature = $signature.Trim()
try { $null = [Convert]::FromBase64String($signature) } catch { throw 'Updater signature is not valid base64; refusing to generate latest.json' }
& node (Join-Path $PSScriptRoot 'verify-update-signature.mjs') --installer $installer.FullName --config (Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri/tauri.conf.json') --version $Version
if ($LASTEXITCODE -ne 0) { throw 'Updater signature verification failed; refusing to generate latest.json' }

$encodedTag = [Uri]::EscapeDataString($Tag)
$encodedName = [Uri]::EscapeDataString($installer.Name)
$manifest = [ordered]@{
  version = $Version
  notes = $Notes
  pub_date = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
  platforms = [ordered]@{
    'windows-x86_64' = [ordered]@{
      signature = $signature
      url = "https://github.com/$Repository/releases/download/$encodedTag/$encodedName"
    }
  }
}
New-Item -ItemType Directory -Path (Split-Path $manifestPath -Parent) -Force | Out-Null
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath -Encoding utf8
Write-Output "Update manifest: $manifestPath"

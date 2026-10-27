$ErrorActionPreference = 'Stop'
$cliVersion = '2.11.1'
$expectedHash = 'C019079A54F8D158435EC0A2B3DEA4EC2F256F073C3D2C09391396B4F8DA8DC1'
$archive = Join-Path $env:RUNNER_TEMP 'controller-tauri-cli.zip'
$unpack = Join-Path $env:RUNNER_TEMP 'controller-tauri-cli'
$binFolder = Join-Path $env:USERPROFILE '.cargo\bin'
Invoke-WebRequest -Uri "https://github.com/tauri-apps/tauri/releases/download/tauri-cli-v$cliVersion/cargo-tauri-x86_64-pc-windows-msvc.zip" -OutFile $archive
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Tauri CLI archive checksum differs from the official release digest' }
Expand-Archive -LiteralPath $archive -DestinationPath $unpack -Force
$executables = @(Get-ChildItem -LiteralPath $unpack -Filter cargo-tauri.exe -Recurse -File)
if ($executables.Count -ne 1) { throw 'Expected one Tauri CLI executable' }
New-Item -ItemType Directory -Path $binFolder -Force | Out-Null
$destination = Join-Path $binFolder 'cargo-tauri.exe'
Copy-Item -LiteralPath $executables[0].FullName -Destination $destination -Force
$actual = & $destination --version
if ($LASTEXITCODE -ne 0 -or $actual -notmatch '2\.11\.1$') { throw "Unexpected Tauri CLI version: $actual" }
Write-Output $actual

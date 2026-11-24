# Download the same pinned distribution in either native CI dispatch mode.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $IsWindows) {
    throw 'This smoke runner is restricted to hosted Windows Actions.'
}
$archive = Join-Path $env:RUNNER_TEMP 'blender-5.2.1-windows-x64.zip'
Invoke-WebRequest -Uri 'https://download.blender.org/release/Blender5.2/blender-5.2.1-windows-x64.zip' -OutFile $archive
$expected = '0e631dad7d0cad6d5d18abdd2e2550f6c0213215334eda00ddbd3d22b96ecb2c'
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw 'Blender archive digest mismatch' }
$destination = Join-Path $env:RUNNER_TEMP 'semwright-native-artifacts-blender'
if (Test-Path -LiteralPath $destination) { throw 'Native runtime destination already exists' }
Expand-Archive -LiteralPath $archive -DestinationPath $destination
$runtime = Join-Path $destination 'blender-5.2.1-windows-x64'
python -B scripts/windows/blender-artifacts-smoke.py --bin-dir target/debug --blender-root $runtime --output verification/windows-native-artifacts
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

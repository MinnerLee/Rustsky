$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$settingsPath = Join-Path $projectRoot '.preview/mysql/local-settings.json'
$settings = Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json

$previousDatabaseUrl = $env:DATABASE_URL
$previousJwtSecret = $env:RUSTSKY_JWT_SECRET
Push-Location $projectRoot
try {
    $env:DATABASE_URL = $settings.databaseUrl
    $env:RUSTSKY_JWT_SECRET = $settings.jwtSecret
    cargo run --locked
    if ($LASTEXITCODE -ne 0) { throw "Rustsky exited with code $LASTEXITCODE" }
} finally {
    $env:DATABASE_URL = $previousDatabaseUrl
    $env:RUSTSKY_JWT_SECRET = $previousJwtSecret
    Pop-Location
}

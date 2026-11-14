$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$assets = Join-Path $root '.preview/course/Web/nginx-1.20.2/html/sky'
$null = New-Item -ItemType Directory -Path $assets -Force
Push-Location (Join-Path $root 'frontend')
try {
    if (-not (Test-Path 'node_modules/@vue/cli-service/bin/vue-cli-service.js')) {
        npm.cmd ci --ignore-scripts --legacy-peer-deps
    }
    node --openssl-legacy-provider node_modules/@vue/cli-service/bin/vue-cli-service.js build --dest $assets
} finally {
    Pop-Location
}

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$prefix = $root.Replace('\', '/') + '/'
$mysqlRoot = Join-Path $root '.preview/mysql'
$mysqlBin = Join-Path $mysqlRoot 'mysql-8.4.13-winx64/bin'
$nginx = Join-Path $root '.preview/nginx/nginx-1.31.8/nginx.exe'
$nginxConfig = @('-p', $prefix, '-c', 'tools/nginx.conf', '-e', 'stderr')
$nginxRoot = Join-Path $root '.preview/nginx'
foreach ($directory in 'client_body_temp', 'proxy_temp', 'fastcgi_temp', 'uwsgi_temp', 'scgi_temp') {
    $null = New-Item -ItemType Directory -Force (Join-Path $nginxRoot $directory)
}
if (-not (Test-Path (Join-Path $root '.preview/course/Web/nginx-1.20.2/html/sky/index.html'))) {
    throw 'Build the admin frontend with tools/build-admin.ps1 before starting the stack.'
}
$mysqlConfig = '--defaults-file=' + (Join-Path $mysqlRoot 'my.ini').Replace('\', '/')
$mysqlClient = '--defaults-extra-file=' + (Join-Path $mysqlRoot 'root-client.ini').Replace('\', '/')
$settingsPath = Join-Path $mysqlRoot 'local-settings.json'
$settings = Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json -AsHashtable
$settingsChanged = $false
foreach ($key in 'customerJwtSecret', 'customerDevCode') {
    if (-not $settings[$key]) {
        $settings[$key] = [Convert]::ToHexString([System.Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
        $settingsChanged = $true
    }
}
if ($settingsChanged) {
    $settings | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $settingsPath -Encoding utf8
}
$apiProcesses = @()
$database = $null
$proxy = $null
$redis = $null
$redisRoot = Join-Path $root '.preview/redis'
$redisUrl = $env:RUSTSKY_REDIS_URL
$redisServer = $null
if (-not $redisUrl -and (Test-Path (Join-Path $redisRoot 'bin'))) {
    $redisServer = Get-ChildItem -LiteralPath (Join-Path $redisRoot 'bin') -Recurse -File -Filter redis-server.exe | Select-Object -First 1
}

function Wait-ForReady($process, [scriptblock]$check, [string]$label) {
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        if ($process.HasExited) { throw "$label exited with code $($process.ExitCode)." }
        try { if (& $check) { return } } catch { }
        Start-Sleep -Milliseconds 500
    }
    throw "$label did not become ready. Check the logs under .preview."
}

$ports = @(3306,8080,8081,8888)
if ($redisServer) { $ports += 6379 }
$occupied = Get-NetTCPConnection -State Listen -LocalPort $ports -ErrorAction SilentlyContinue
if ($occupied) { throw "Already listening on port(s) $($occupied.LocalPort -join ', '). Stop the existing local services before starting the stack." }

Push-Location $root
try {
    & $nginx @nginxConfig -t
    cargo build --locked

    $database = Start-Process (Join-Path $mysqlBin 'mysqld.exe') `
        -ArgumentList ('"' + $mysqlConfig + '"') -WindowStyle Hidden -PassThru
    Wait-ForReady $database {
        & (Join-Path $mysqlBin 'mysqladmin.exe') $mysqlClient --connect-timeout=1 --silent ping 2>$null | Out-Null
        $LASTEXITCODE -eq 0
    } 'MySQL'

    if ($redisServer) {
        $redisCli = Join-Path $redisServer.DirectoryName 'redis-cli.exe'
        $null = New-Item -ItemType Directory -Force (Join-Path $redisRoot 'data')
        $redis = Start-Process $redisServer.FullName -ArgumentList 'tools/redis-local.conf' -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput (Join-Path $redisRoot 'redis.log') -RedirectStandardError (Join-Path $redisRoot 'redis-error.log')
        Wait-ForReady $redis { (& $redisCli -h 127.0.0.1 -p 6379 PING 2>$null) -eq 'PONG' } 'Redis'
        $redisUrl = 'redis://127.0.0.1:6379/'
    }

    foreach ($apiPort in 8080,8081) {
        $api = Start-Process (Join-Path $root 'target/debug/Rustsky.exe') -WindowStyle Hidden -PassThru `
            -Environment @{
                DATABASE_URL = $settings.databaseUrl
                RUSTSKY_JWT_SECRET = $settings.jwtSecret
                RUSTSKY_USER_JWT_SECRET = $settings.customerJwtSecret
                RUSTSKY_DEV_USER_CODE = $settings.customerDevCode
                RUSTSKY_PORT = "$apiPort"
                RUSTSKY_REDIS_URL = $redisUrl
            } `
            -RedirectStandardError (Join-Path $mysqlRoot "api-$apiPort.err.log")
        $apiProcesses += $api
        Wait-ForReady $api { (Invoke-RestMethod "http://127.0.0.1:$apiPort/health" -TimeoutSec 1).code -eq 1 } "Rust API $apiPort"
    }

    $proxy = Start-Process $nginx -ArgumentList ('-p "' + $prefix + '" -c tools/nginx.conf -e stderr') -WindowStyle Hidden -PassThru
    Wait-ForReady $proxy { (Invoke-WebRequest 'http://127.0.0.1:8888/sky/' -TimeoutSec 1).StatusCode -eq 200 } 'Nginx'
    Write-Host 'Rustsky is ready at http://127.0.0.1:8888/sky/. Press Ctrl+C to stop the stack.'
    Wait-Process -Id $proxy.Id
} finally {
    if ($proxy -and -not $proxy.HasExited) {
        try { & $nginx @nginxConfig -s quit } catch { Write-Warning 'Could not stop Nginx gracefully.' }
    }
    foreach ($api in $apiProcesses) {
        if (-not $api.HasExited) { Stop-Process -InputObject $api -ErrorAction SilentlyContinue }
    }
    if ($redis -and -not $redis.HasExited) {
        try { & $redisCli -h 127.0.0.1 -p 6379 SHUTDOWN } catch { Write-Warning 'Could not stop Redis gracefully.' }
    }
    if ($database -and -not $database.HasExited) {
        try { & (Join-Path $mysqlBin 'mysqladmin.exe') $mysqlClient --connect-timeout=5 shutdown } catch { Write-Warning 'Could not stop MySQL gracefully.' }
    }
    Pop-Location
}

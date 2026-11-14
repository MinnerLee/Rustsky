param([ValidateRange(1024, 65535)][int]$Port = 3306)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$runtime = Join-Path $root '.preview/mysql'
$mysqlFolder = Join-Path $runtime 'mysql-8.4.13-winx64'
$bin = Join-Path $mysqlFolder 'bin'
$data = Join-Path $runtime 'data'
$ini = Join-Path $runtime 'my.ini'
$rootClient = Join-Path $runtime 'root-client.ini'
$settings = Join-Path $runtime 'local-settings.json'
if (Test-Path $settings) { throw 'Local settings already exist. Start the existing stack instead.' }
if (Test-Path $data) { throw 'A MySQL data directory already exists. Use a new checkout for initialization.' }
if (-not (Test-Path (Join-Path $bin 'mysqld.exe'))) { throw 'Extract the MySQL ZIP under .preview/mysql first.' }
if (Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue) { throw "Port $Port is in use." }

function New-Secret {
    [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(24))
}

$rootPassword = New-Secret
$appPassword = New-Secret
$employeePassword = New-Secret
$server = $null
$defaults = '--defaults-file=' + $ini
$rootOption = '--defaults-extra-file=' + $rootClient

Push-Location $root
try {
    cargo build --locked
    $env:RUSTSKY_DEV_PASSWORD = $employeePassword
    try { $passwordHash = & ./target/debug/Rustsky.exe --hash-dev-password }
    finally { Remove-Item Env:RUSTSKY_DEV_PASSWORD -ErrorAction SilentlyContinue }
    @"
[mysqld]
basedir=$($mysqlFolder.Replace('\','/'))
datadir=$($data.Replace('\','/'))
bind-address=127.0.0.1
port=$Port
mysqlx=OFF
log-error=$((Join-Path $runtime 'mysql-error.log').Replace('\','/'))
"@ | Set-Content -LiteralPath $ini -Encoding utf8NoBOM
    & (Join-Path $bin 'mysqld.exe') $defaults --initialize-insecure
    $server = Start-Process (Join-Path $bin 'mysqld.exe') -ArgumentList ('"' + $defaults + '"') -WindowStyle Hidden -PassThru
    $ready = $false
    for ($attempt = 0; $attempt -lt 30 -and -not $server.HasExited; $attempt++) {
        try {
            & (Join-Path $bin 'mysqladmin.exe') --host=127.0.0.1 "--port=$Port" --user=root --silent ping 2>$null | Out-Null
            $ready = $LASTEXITCODE -eq 0
        } catch {}
        if ($ready) { break }
        Start-Sleep -Milliseconds 500
    }
    if (-not $ready) { throw 'MySQL did not start. Read .preview/mysql/mysql-error.log.' }
    "ALTER USER 'root'@'localhost' IDENTIFIED BY '$rootPassword';" |
        & (Join-Path $bin 'mysql.exe') --host=127.0.0.1 "--port=$Port" --user=root
    @"
[client]
host=127.0.0.1
port=$Port
user=root
password=$rootPassword
"@ | Set-Content -LiteralPath $rootClient -Encoding utf8NoBOM
    @"
CREATE DATABASE rustsky_dev CHARACTER SET utf8mb4;
CREATE USER 'rustsky_app'@'127.0.0.1' IDENTIFIED BY '$appPassword';
GRANT SELECT ON rustsky_dev.* TO 'rustsky_app'@'127.0.0.1';
"@ | & (Join-Path $bin 'mysql.exe') $rootOption
    foreach ($migration in Get-ChildItem -LiteralPath (Join-Path $root 'db') -Filter '*.sql' -File | Sort-Object Name) {
        & (Join-Path $bin 'mysql.exe') $rootOption --database=rustsky_dev "--execute=SOURCE db/$($migration.Name)"
    }
    "INSERT INTO employee (name,username,password,phone,sex,id_number,create_time,update_time)
      VALUES ('Administrator','admin','$passwordHash','13900000000','1','000000000000000000',UTC_TIMESTAMP(),UTC_TIMESTAMP());" |
        & (Join-Path $bin 'mysql.exe') $rootOption --database=rustsky_dev
    @{
        databaseUrl = "mysql://rustsky_app:$appPassword@127.0.0.1:$Port/rustsky_dev"
        jwtSecret = New-Secret
        employeeUsername = 'admin'
        employeePassword = $employeePassword
    } | ConvertTo-Json | Set-Content -LiteralPath $settings -Encoding utf8NoBOM
    Write-Host 'MySQL initialized. Local admin credentials are in .preview/mysql/local-settings.json.'
} finally {
    if ($server -and -not $server.HasExited) {
        $admin = Join-Path $bin 'mysqladmin.exe'
        if (Test-Path $rootClient) { & $admin $rootOption shutdown }
        else { & $admin --host=127.0.0.1 "--port=$Port" --user=root shutdown }
    }
    Pop-Location
}

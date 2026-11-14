# Run against the local Rustsky stack with the employee-number migration applied.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$mysqlRoot = Join-Path $root '.preview/mysql'
$settings = Get-Content -LiteralPath (Join-Path $mysqlRoot 'local-settings.json') -Raw | ConvertFrom-Json
$mysql = Join-Path $mysqlRoot 'mysql-8.4.13-winx64/bin/mysql.exe'
$mysqlConfig = '--defaults-extra-file=' + (Join-Path $mysqlRoot 'root-client.ini')
$employeeEndpoint = 'http://127.0.0.1:8888/api/employee'

function Assert-Result([bool]$passed, [string]$message) {
    if (-not $passed) { throw $message }
}

function Invoke-EmployeeLogin([string]$username, [string]$password) {
    $body = @{ username = $username; password = $password } | ConvertTo-Json
    Invoke-RestMethod "$employeeEndpoint/login" -Method Post -ContentType application/json -Body $body -TimeoutSec 8
}

function Submit-Employee([hashtable]$payload, [hashtable]$headers = @{}) {
    Invoke-WebRequest $employeeEndpoint -Method Post -Headers $headers -ContentType application/json `
        -Body ($payload | ConvertTo-Json) -SkipHttpErrorCheck -TimeoutSec 8
}

$admin = Invoke-EmployeeLogin $settings.employeeUsername $settings.employeePassword
Assert-Result ($admin.code -eq 1 -and $admin.data.token -and $admin.data.id -gt 0) 'Admin login failed.'
$actor = [long]$admin.data.id
$token = $admin.data.token
$auth = @{ token = $token }
$nonce = [guid]::NewGuid().ToString('N').Substring(0, 10)
$firstUsername = "tst$nonce"
$secondUsername = "dupe$nonce"
$raceUsernames = @("racea$nonce", "raceb$nonce")
$firstNumber = "RS-000$($nonce.ToUpperInvariant())"
$raceNumber = "RS-RACE$($nonce.ToUpperInvariant())"
$payload = @{
    username = $firstUsername
    name = '测试员工'
    employeeNo = $firstNumber.ToLowerInvariant()
    phone = '13900000000'
    sex = '1'
    idNumber = '000000000000000000'
    initialPassword = [guid]::NewGuid().ToString('N')
}

try {
    $response = Submit-Employee $payload
    Assert-Result ($response.StatusCode -eq 401) 'A request without a token did not return 401.'

    $invalid = $payload.Clone()
    $invalid.Remove('employeeNo')
    $response = Submit-Employee $invalid $auth
    Assert-Result ($response.StatusCode -eq 400) 'A request without an employee number did not return 400.'

    # A client-supplied actor must never override the identity from the JWT.
    $payload.createUser = $actor + 1000
    $payload.updateUser = $actor + 2000
    $response = Submit-Employee $payload $auth
    $created = $response.Content | ConvertFrom-Json
    Assert-Result ($response.StatusCode -eq 200 -and $created.code -eq 1 -and $created.data.employeeNo -ceq $firstNumber -and $created.data.id -gt 0) 'The employee or normalized employee number was not returned.'

    $employeeId = [long]$created.data.id
    $saved = & $mysql $mysqlConfig --batch --skip-column-names --database=rustsky_dev --execute="SELECT COUNT(*) FROM employee WHERE id=$employeeId AND username='$firstUsername' AND employee_no='$firstNumber' AND status=1 AND create_time IS NOT NULL AND update_time IS NOT NULL AND create_user=$actor AND update_user=$actor AND LEFT(password,9)=CONCAT(CHAR(36),'argon2id');"
    Assert-Result ($saved -eq '1') 'MySQL data did not match the employee number, audit fields, enabled status, or password hash.'

    $duplicate = $payload.Clone()
    $duplicate.username = $secondUsername
    $duplicate.employeeNo = $firstNumber
    $response = Submit-Employee $duplicate $auth
    $conflict = $response.Content | ConvertFrom-Json
    Assert-Result ($response.StatusCode -eq 200 -and $conflict.code -eq 0 -and $conflict.msg -eq '账号或工号已存在') 'A duplicate employee number did not return a readable conflict response.'

    $duplicate.username = $firstUsername
    $duplicate.employeeNo = $raceNumber
    $response = Submit-Employee $duplicate $auth
    $conflict = $response.Content | ConvertFrom-Json
    Assert-Result ($response.StatusCode -eq 200 -and $conflict.code -eq 0 -and $conflict.msg -eq '账号或工号已存在') 'A duplicate username did not return a readable conflict response.'

    $employee = Invoke-EmployeeLogin $firstUsername $payload.initialPassword
    Assert-Result ($employee.code -eq 1 -and $employee.data.id -eq $employeeId) 'The new employee could not log in.'
    $duplicate.username = $secondUsername
    $response = Submit-Employee $duplicate @{ token = $employee.data.token }
    Assert-Result ($response.StatusCode -eq 403) 'An ordinary employee was allowed to create employees.'

    $requests = foreach ($username in $raceUsernames) {
        $candidate = $payload.Clone()
        $candidate.username = $username
        $candidate.employeeNo = $raceNumber
        $candidate | ConvertTo-Json -Compress
    }
    $results = $requests | ForEach-Object -Parallel {
        $response = Invoke-RestMethod $using:employeeEndpoint -Method Post -Headers @{ token = $using:token } `
            -ContentType application/json -Body $_ -TimeoutSec 8
        $response.code
    } -ThrottleLimit 2
    Assert-Result ((@($results | Sort-Object) -join ',') -eq '0,1') 'Concurrent requests did not produce exactly one success.'
    $raceRows = & $mysql $mysqlConfig --batch --skip-column-names --database=rustsky_dev --execute="SELECT COUNT(*) FROM employee WHERE employee_no='$raceNumber' AND create_user=$actor;"
    Assert-Result ($raceRows -eq '1') 'The concurrent test did not store exactly one row.'
} finally {
    $testUsernames = (@($firstUsername, $secondUsername) + $raceUsernames) -join "','"
    & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE FROM employee WHERE username IN ('$testUsernames') AND employee_no IN ('$firstNumber','$raceNumber') AND create_user=$actor;"
}

Write-Host 'Employee creation, login, JWT audit identity, duplicates, concurrency, and 400/401/403 checks passed.'

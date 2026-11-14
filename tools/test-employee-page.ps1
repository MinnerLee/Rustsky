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

$admin = Invoke-EmployeeLogin $settings.employeeUsername $settings.employeePassword
Assert-Result ($admin.code -eq 1 -and $admin.data.token -and $admin.data.id -gt 0) 'Admin login failed.'
$actor = [long]$admin.data.id
$auth = @{ token = $admin.data.token }
$nonce = [guid]::NewGuid().ToString('N').Substring(0, 8)
$name = "分页$nonce"
$filter = [Uri]::EscapeDataString($name)
$password = [guid]::NewGuid().ToString('N')
$testUsernames = @()
$testNumbers = @()
$ids = @()

try {
    foreach ($sequence in 1..3) {
        $username = "pg${nonce}$sequence"
        $number = "PG-00$($nonce.ToUpperInvariant())$sequence"
        $testUsernames += $username
        $testNumbers += $number
        $payload = @{
            username = $username; name = "$name$sequence"; employeeNo = $number
            phone = '13900000000'; sex = '1'; idNumber = '000000000000000000'; initialPassword = $password
        }
        $created = Invoke-RestMethod $employeeEndpoint -Method Post -Headers $auth -ContentType application/json `
            -Body ($payload | ConvertTo-Json) -TimeoutSec 8
        Assert-Result ($created.code -eq 1 -and $created.data.id -gt 0) 'Fixture creation failed.'
        $ids += [long]$created.data.id
    }

    $first = Invoke-RestMethod "$employeeEndpoint/page?page=1&pageSize=2&name=$filter" -Headers $auth -TimeoutSec 8
    $second = Invoke-RestMethod "$employeeEndpoint/page?page=2&pageSize=2&name=$filter" -Headers $auth -TimeoutSec 8
    $beyond = Invoke-RestMethod "$employeeEndpoint/page?page=3&pageSize=2&name=$filter" -Headers $auth -TimeoutSec 8
    Assert-Result ($first.code -eq 1 -and $first.data.total -eq 3 -and $first.data.records.Count -eq 2) 'Page 1 size or total mismatch.'
    Assert-Result ($second.code -eq 1 -and $second.data.total -eq 3 -and $second.data.records.Count -eq 1) 'Page 2 size or total mismatch.'
    Assert-Result ($beyond.code -eq 1 -and $beyond.data.total -eq 3 -and $beyond.data.records.Count -eq 0) 'Out-of-range page should retain the total.'
    $records = @($first.data.records) + @($second.data.records)
    Assert-Result (($records.id -join ',') -eq ($ids[2..0] -join ',')) 'Records are out of order or duplicated.'
    Assert-Result (($records.employeeNo -join ',') -ceq ($testNumbers[2..0] -join ',')) 'Employee numbers are missing or incorrect.'
    foreach ($record in $records) {
        $fields = @($record.PSObject.Properties.Name | Sort-Object)
        Assert-Result (($fields -join ',') -ceq 'employeeNo,id,name,phone,status,updateTime,username') 'Unexpected fields in employee list response.'
        Assert-Result ($record.updateTime -match '^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$') 'Last operation time format mismatch.'
    }

    $empty = Invoke-RestMethod "$employeeEndpoint/page?name=$filter%25" -Headers $auth -TimeoutSec 8
    Assert-Result ($empty.code -eq 1 -and $empty.data.total -eq 0 -and $empty.data.records.Count -eq 0) 'A literal percent sign unexpectedly matched rows.'
    $default = Invoke-RestMethod "$employeeEndpoint/page?name=$filter" -Headers $auth -TimeoutSec 8
    Assert-Result ($default.code -eq 1 -and $default.data.total -eq 3 -and $default.data.records.Count -eq 3) 'Default page or page size mismatch.'
    $invalid = Invoke-WebRequest "$employeeEndpoint/page?page=0&pageSize=101" -Headers $auth -SkipHttpErrorCheck -TimeoutSec 8
    Assert-Result ($invalid.StatusCode -eq 400) 'Invalid pagination was accepted.'
    $unauthorized = Invoke-WebRequest "$employeeEndpoint/page" -SkipHttpErrorCheck -TimeoutSec 8
    Assert-Result ($unauthorized.StatusCode -eq 401) 'Missing token did not return 401.'
    $employee = Invoke-EmployeeLogin $testUsernames[0] $password
    Assert-Result ($employee.code -eq 1 -and $employee.data.token) 'Fixture employee login failed.'
    $forbidden = Invoke-WebRequest "$employeeEndpoint/page" -Headers @{ token = $employee.data.token } -SkipHttpErrorCheck -TimeoutSec 8
    Assert-Result ($forbidden.StatusCode -eq 403) 'A regular employee was allowed to view employee records.'
} finally {
    if ($testUsernames.Count -gt 0) {
        $names = $testUsernames -join "','"
        $numbers = $testNumbers -join "','"
        & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE FROM employee WHERE username IN ('$names') AND employee_no IN ('$numbers') AND create_user=$actor;"
    }
}

Write-Host 'Employee search, paging, ordering, totals, employee numbers, date format, empty results, and access checks passed.'

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$mysqlRoot = Join-Path $root '.preview/mysql'
$settings = Get-Content -LiteralPath (Join-Path $mysqlRoot 'local-settings.json') -Raw | ConvertFrom-Json
$mysql = Join-Path $mysqlRoot 'mysql-8.4.13-winx64/bin/mysql.exe'
$mysqlConfig = '--defaults-extra-file=' + (Join-Path $mysqlRoot 'root-client.ini')
$base = 'http://127.0.0.1:8888/api'

function Assert-Result([bool]$passed, [string]$message) {
    if (-not $passed) { throw $message }
}

function Send-Json([string]$path, [string]$method, [hashtable]$body, [hashtable]$headers = @{}) {
    Invoke-RestMethod "$base$path" -Method $method -Headers $headers -ContentType application/json `
        -Body ($body | ConvertTo-Json) -TimeoutSec 8
}

$admin = Send-Json '/employee/login' Post @{ username = $settings.employeeUsername; password = $settings.employeePassword }
Assert-Result ($admin.code -eq 1) 'Admin login failed.'
$auth = @{ token = $admin.data.token }
$actor = [long]$admin.data.id
$nonce = [guid]::NewGuid().ToString('N').Substring(0, 10)
$employeeName = "mgr$nonce"
$initialNumber = "MGR-$nonce".ToUpperInvariant()
$editedNumber = "MGR-00$nonce".ToUpperInvariant()
$categoryName = "cat$nonce"
$editedCategory = "category$nonce"
$password = [guid]::NewGuid().ToString('N')

try {
    $profile = @{ username=$employeeName; name='测试员工'; employeeNo=$initialNumber; phone='13900000000'; sex='1'; idNumber='000000000000000000' }
    $create = $profile.Clone()
    $create.initialPassword = $password
    $new = Send-Json '/employee' Post $create $auth
    Assert-Result ($new.code -eq 1) 'Fixture employee creation failed.'
    $id = [long]$new.data.id
    $detail = Invoke-RestMethod "$base/employee/$id" -Headers $auth -TimeoutSec 8
    Assert-Result ($detail.code -eq 1 -and $detail.data.employeeNo -ceq $initialNumber -and $detail.data.idNumber -eq $profile.idNumber -and -not $detail.data.PSObject.Properties['password']) 'Employee detail mismatch.'

    $profile.id = $id
    $profile.name = '测试编辑'
    $profile.employeeNo = $editedNumber.ToLowerInvariant()
    $edit = Send-Json '/employee' Put $profile $auth
    Assert-Result ($edit.code -eq 1) 'Employee edit failed.'
    $detail = Invoke-RestMethod "$base/employee/$id" -Headers $auth -TimeoutSec 8
    Assert-Result ($detail.data.name -eq $profile.name -and $detail.data.employeeNo -ceq $editedNumber) 'Employee edit was not persisted.'
    $login = Send-Json '/employee/login' Post @{username=$employeeName;password=$password}
    Assert-Result ($login.code -eq 1) 'Original password failed after edit.'

    $disable = Invoke-RestMethod "$base/employee/status/0?id=$id" -Method Post -Headers $auth -TimeoutSec 8
    Assert-Result ($disable.code -eq 1) 'Employee disable failed.'
    $disabled = Send-Json '/employee/login' Post @{username=$employeeName;password=$password}
    $oldToken = Invoke-WebRequest "$base/employee/logout" -Method Post -Headers @{ token = $login.data.token } -SkipHttpErrorCheck -TimeoutSec 8
    Assert-Result ($disabled.code -eq 0 -and $oldToken.StatusCode -eq 401) 'Disabled employee retained access.'
    $enable = Invoke-RestMethod "$base/employee/status/1?id=$id" -Method Post -Headers $auth -TimeoutSec 8
    $enabled = Send-Json '/employee/login' Post @{username=$employeeName;password=$password}
    Assert-Result ($enable.code -eq 1 -and $enabled.code -eq 1) 'Employee enable failed.'
    $protected = Invoke-RestMethod "$base/employee/status/0?id=$actor" -Method Post -Headers $auth -TimeoutSec 8
    Assert-Result ($protected.code -eq 0) 'Administrator account was not protected.'

    $category = Send-Json '/category' Post @{name=$categoryName;type=1;sort=2} $auth
    Assert-Result ($category.code -eq 1) 'Category create failed.'
    $page = Invoke-RestMethod "$base/category/page?name=$categoryName&type=1&page=1&pageSize=10" -Headers $auth -TimeoutSec 8
    Assert-Result ($page.data.total -eq 1 -and $page.data.records[0].status -eq 0) 'Category page or initial status mismatch.'
    $categoryId = [long]$page.data.records[0].id
    $change = Send-Json '/category' Put @{id=$categoryId;name=$editedCategory;sort=4} $auth
    $on = Invoke-RestMethod "$base/category/status/1?id=$categoryId" -Method Post -Headers $auth -TimeoutSec 8
    $available = Invoke-RestMethod "$base/category/list?type=1" -Headers $auth -TimeoutSec 8
    Assert-Result ($change.code -eq 1 -and $on.code -eq 1 -and ($available.data | Where-Object { $_.id -eq $categoryId -and $_.name -eq $editedCategory -and $_.sort -eq 4 })) 'Updated category did not appear in enabled list.'
    $off = Invoke-RestMethod "$base/category/status/0?id=$categoryId" -Method Post -Headers $auth -TimeoutSec 8
    $removed = Invoke-RestMethod "$base/category?id=$categoryId" -Method Delete -Headers $auth -TimeoutSec 8
    $empty = Invoke-RestMethod "$base/category/page?name=$editedCategory" -Headers $auth -TimeoutSec 8
    Assert-Result ($off.code -eq 1 -and $removed.code -eq 1 -and $empty.data.total -eq 0) 'Category disable or delete failed.'
} finally {
    & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE FROM employee WHERE username='$employeeName' AND employee_no IN ('$initialNumber','$editedNumber') AND create_user=$actor; DELETE FROM category WHERE name IN ('$categoryName','$editedCategory') AND create_user=$actor;"
}

Write-Host 'Employee details, edit, status, session checks, and category lifecycle passed.'

param([string]$BaseUrl = 'http://127.0.0.1:8888')

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$settings = Get-Content -LiteralPath (Join-Path $root '.preview/mysql/local-settings.json') -Raw | ConvertFrom-Json
$addressRoute = "$BaseUrl/user/addressBook"

function Invoke-Json([string]$Method, [string]$Url, [hashtable]$Headers = @{}, $Data = $null) {
    $request = @{ Uri = $Url; Method = $Method; Headers = $Headers; TimeoutSec = 5; SkipHttpErrorCheck = $true }
    if ($null -ne $Data) {
        $request.ContentType = 'application/json; charset=utf-8'
        $request.Body = ConvertTo-Json -InputObject $Data -Compress
    }
    Invoke-RestMethod @request
}

function Assert-Success($result, [string]$message) {
    if ($result.code -ne 1) { throw "$message ($($result.msg))" }
}

$bad = Invoke-Json POST "$BaseUrl/user/user/login" @{} @{ code = 'invalid-code' }
if ($bad.code -ne 0) { throw 'Invalid login was accepted.' }
$anonymous = Invoke-WebRequest "$addressRoute/list" -SkipHttpErrorCheck -TimeoutSec 5
if ($anonymous.StatusCode -ne 401) { throw 'Anonymous address access was not denied.' }
$login = Invoke-Json POST "$BaseUrl/user/user/login" @{} @{ code = $settings.customerDevCode }
Assert-Success $login 'Could not sign in as development customer'
$again = Invoke-Json POST "$BaseUrl/user/user/login" @{} @{ code = $settings.customerDevCode }
if ($again.code -ne 1 -or $again.data.id -ne $login.data.id) { throw 'Repeat login did not find the same customer.' }
$headers = @{ authentication = $login.data.token }
$previous = Invoke-Json GET "$addressRoute/default" $headers
$input = @{
    consignee = '测试收货人'
    phone = '13800000000'
    sex = '1'
    cityName = '北京市'
    detail = 'Rustsky 联调 ' + [Guid]::NewGuid().ToString('N')
    label = '测试'
}
$id = $null
try {
    Assert-Success (Invoke-Json POST $addressRoute $headers $input) 'Could not create address'
    $listing = Invoke-Json GET "$addressRoute/list" $headers
    Assert-Success $listing 'Could not list addresses'
    $address = $listing.data | Where-Object detail -eq $input.detail | Select-Object -First 1
    if (-not $address -or $address.consignee -ne $input.consignee) { throw 'Created address is absent from the list.' }
    $id = $address.id
    Assert-Success (Invoke-Json PUT "$addressRoute/default" $headers @{ id = $id }) 'Could not set default address'
    $default = Invoke-Json GET "$addressRoute/default" $headers
    if ($default.code -ne 1 -or $default.data.id -ne $id) { throw 'Default address mismatch.' }
    $missing = Invoke-Json PUT "$addressRoute/default" $headers @{ id = -1 }
    $default = Invoke-Json GET "$addressRoute/default" $headers
    if ($missing.code -ne 0 -or $default.data.id -ne $id) { throw 'Invalid default change did not roll back.' }
    $input.id = $id
    $input.label = '已修改'
    Assert-Success (Invoke-Json PUT $addressRoute $headers $input) 'Could not edit address'
    $details = Invoke-Json GET "$addressRoute/$id" $headers
    if ($details.code -ne 1 -or $details.data.label -ne $input.label) { throw 'Edited address was not persisted.' }
} finally {
    if ($id) {
        if ($previous.code -eq 1) {
            Assert-Success (Invoke-Json PUT "$addressRoute/default" $headers @{ id = $previous.data.id }) 'Could not restore old default'
        }
        Assert-Success (Invoke-Json DELETE "$($addressRoute)?id=$id" $headers) 'Could not clean up test address'
    }
}
$deleted = Invoke-Json GET "$addressRoute/$id" $headers
if ($deleted.code -ne 0) { throw 'Deleted address is still visible.' }
Write-Host 'PASS customer login, repeat login, protected addresses, address CRUD and default rollback.'

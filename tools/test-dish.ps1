param(
    [switch]$IncludeSetmeal, [switch]$IncludeMenu, [switch]$IncludeCart, [switch]$IncludeOrder,
    [switch]$IncludeCache, [switch]$IncludeOrderManagement, [switch]$IncludeAutomation, [switch]$IncludeReports
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$runtime = Join-Path $root '.preview/mysql'
$mysql = Join-Path $runtime 'mysql-8.4.13-winx64/bin/mysql.exe'
$mysqlConfig = '--defaults-extra-file=' + (Join-Path $runtime 'root-client.ini')
$settings = Get-Content -LiteralPath (Join-Path $runtime 'local-settings.json') -Raw | ConvertFrom-Json
$base = 'http://127.0.0.1:8888'

function Assert-Result([bool]$passed, [string]$message) {
    if (-not $passed) { throw $message }
}

function Send-Json([string]$path, [string]$method, [hashtable]$body, [hashtable]$headers = @{}) {
    Invoke-RestMethod "$base/api$path" -Method $method -Headers $headers -ContentType application/json `
        -Body ($body | ConvertTo-Json -Depth 4) -TimeoutSec 8
}

function Send-Cart([string]$action, [hashtable]$body) {
    Invoke-RestMethod "$base/user/shoppingCart/$action" -Method Post -Headers $customerAuth -ContentType application/json `
        -Body ($body | ConvertTo-Json -Compress) -TimeoutSec 8
}

function Send-Customer([string]$path, [string]$method, [hashtable]$body) {
    Invoke-RestMethod "$base/user/$path" -Method $method -Headers $customerAuth -ContentType application/json `
        -Body ($body | ConvertTo-Json -Compress) -TimeoutSec 8
}

function Receive-OrderEvent($socket) {
    $cancel = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(8))
    try {
        $buffer = [byte[]]::new(4096)
        $message = $socket.ReceiveAsync([ArraySegment[byte]]::new($buffer), $cancel.Token).GetAwaiter().GetResult()
        if ($message.MessageType -ne [Net.WebSockets.WebSocketMessageType]::Text -or -not $message.EndOfMessage) {
            throw 'Expected a complete text WebSocket event.'
        }
        [Text.Encoding]::UTF8.GetString($buffer, 0, $message.Count) | ConvertFrom-Json
    } finally {
        $cancel.Dispose()
    }
}

$login = Send-Json '/employee/login' Post @{username=$settings.employeeUsername;password=$settings.employeePassword}
Assert-Result ($login.code -eq 1) 'Admin login failed.'
$auth = @{token=$login.data.token}
$actor = [long]$login.data.id
$nonce = [guid]::NewGuid().ToString('N').Substring(0,10)
$name = "dish$nonce"
$category = "dishcat$nonce"
$updatedName = "dishnew$nonce"
$mealCategory = "mealcat$nonce"
$mealName = "meal$nonce"
$editedMealName = "mealnew$nonce"
$imagePath = $null
if ($IncludeAutomation -or $IncludeReports) { $IncludeOrderManagement = $true }
if ($IncludeOrderManagement) { $IncludeOrder = $true }
if ($IncludeOrder) { $IncludeCart = $true }
if ($IncludeCache) {
    $IncludeMenu = $true
    $redisCli = Get-ChildItem (Join-Path $root '.preview/redis/bin') -Recurse -File -Filter redis-cli.exe | Select-Object -First 1
    Assert-Result ($null -ne $redisCli) 'Local Redis CLI was not found.'
}
if ($IncludeCart) {
    $customer = Invoke-RestMethod "$base/user/user/login" -Method Post -ContentType application/json `
        -Body (@{code=$settings.customerDevCode} | ConvertTo-Json -Compress) -TimeoutSec 8
    Assert-Result ($customer.code -eq 1) 'Customer login failed.'
    $customerAuth = @{authentication=$customer.data.token}
    $cartBefore = Invoke-RestMethod "$base/user/shoppingCart/list" -Headers $customerAuth -TimeoutSec 8
    Assert-Result ($cartBefore.code -eq 1) 'Customer cart could not be loaded.'
    if ($IncludeOrder -and $cartBefore.data.Count -ne 0) { throw 'Checkout test requires an empty development customer cart.' }
}
try {
    $photo = Get-Item -LiteralPath (Join-Path $root 'frontend/src/assets/login/hot-pot.jpg')
    $upload = Invoke-RestMethod "$base/api/common/upload" -Method Post -Form @{file=$photo} -Headers $auth -TimeoutSec 8
    Assert-Result ($upload.code -eq 1 -and $upload.data -match '^/uploads/[a-f0-9]{32}[.]jpg$') 'Image upload failed.'
    $imagePath = $upload.data
    $image = Invoke-WebRequest "$base$imagePath" -Method Head -TimeoutSec 8
    Assert-Result ('image/jpeg' -in $image.Headers['Content-Type']) 'Uploaded image is not served.'

    $createdCategory = Send-Json '/category' Post @{name=$category;type=1;sort=1} $auth
    Assert-Result ($createdCategory.code -eq 1) 'Fixture category failed.'
    $categoryPage = Invoke-RestMethod "$base/api/category/page?name=$category" -Headers $auth -TimeoutSec 8
    $categoryId = [long]$categoryPage.data.records[0].id
    $on = Invoke-RestMethod "$base/api/category/status/1?id=$categoryId" -Method Post -Headers $auth -TimeoutSec 8
    Assert-Result ($on.code -eq 1) 'Fixture category could not be enabled.'

    $payload = @{name=$name;categoryId=$categoryId;price='28.50';image=$imagePath;description='测试菜品';flavors=@(@{name='辣度';value='["微辣","中辣"]'})}
    $create = Send-Json '/dish' Post $payload $auth
    Assert-Result ($create.code -eq 1) "Dish creation failed: $($create.msg)"
    $page = Invoke-RestMethod "$base/api/dish/page?categoryId=$categoryId&name=$name&status=0" -Headers $auth -TimeoutSec 8
    Assert-Result ($page.data.total -eq 1 -and $page.data.records[0].price -eq 28.50 -and $page.data.records[0].categoryName -eq $category) 'Dish page mismatch.'
    $id = [long]$page.data.records[0].id
    $details = Invoke-RestMethod "$base/api/dish/$id" -Headers $auth -TimeoutSec 8
    Assert-Result ($details.data.flavors[0].name -eq '辣度') 'Original flavor missing.'
    $payload.id = $id
    $payload.name = $updatedName
    $payload.price = 29.75
    $payload.flavors = @(@{name='温度';value='["热"]'})
    $edit = Send-Json '/dish' Put $payload $auth
    $details = Invoke-RestMethod "$base/api/dish/$id" -Headers $auth -TimeoutSec 8
    Assert-Result ($edit.code -eq 1 -and $details.data.name -eq $updatedName -and $details.data.price -eq 29.75 -and $details.data.flavors.Count -eq 1 -and $details.data.flavors[0].name -eq '温度') 'Dish or flavor edit failed.'
    $protectedCategory = Invoke-RestMethod "$base/api/category?id=$categoryId" -Method Delete -Headers $auth -TimeoutSec 8
    Assert-Result ($protectedCategory.code -eq 0) 'Category in use was removed.'
    $sell = Invoke-RestMethod "$base/api/dish/status/1?id=$id" -Method Post -Headers $auth -TimeoutSec 8
    $prevented = Invoke-RestMethod "$base/api/dish?ids=$id" -Method Delete -Headers $auth -TimeoutSec 8
    Assert-Result ($sell.code -eq 1 -and $prevented.code -eq 0) 'A dish on sale was deleted.'
    if ($IncludeMenu) {
        $categories = Invoke-RestMethod "$base/user/category/list?type=1" -TimeoutSec 8
        $public = Invoke-RestMethod "$base/user/dish/list?categoryId=$categoryId" -TimeoutSec 8
        Assert-Result (($categories.data.id -contains $categoryId) -and $public.data.Count -eq 1 -and $public.data[0].id -eq $id -and $public.data[0].flavors[0].name -eq '温度') 'Public dish or flavor list mismatch.'
    }
    if ($IncludeCache) {
        $cacheVersion = & $redisCli.FullName --raw GET rustsky:menu:version
        $ttl = & $redisCli.FullName --raw TTL "rustsky:menu:${cacheVersion}:dish:$categoryId"
        Assert-Result ([int]$ttl -gt 0 -and [int]$ttl -le 60) 'Dish menu response was not cached with the expected TTL.'
    }
    if ($IncludeSetmeal) {
        $created = Send-Json '/category' Post @{name=$mealCategory;type=2;sort=1} $auth
        Assert-Result ($created.code -eq 1) 'Setmeal category creation failed.'
        $page = Invoke-RestMethod "$base/api/category/page?name=$mealCategory" -Headers $auth -TimeoutSec 8
        $mealCategoryId = [long]$page.data.records[0].id
        $on = Invoke-RestMethod "$base/api/category/status/1?id=$mealCategoryId" -Method Post -Headers $auth -TimeoutSec 8
        Assert-Result ($on.code -eq 1) 'Setmeal category enable failed.'
        $meal = @{name=$mealName;categoryId=$mealCategoryId;price='52.00';image=$imagePath;setmealDishes=@(@{dishId=$id;copies=2})}
        $created = Send-Json '/setmeal' Post $meal $auth
        Assert-Result ($created.code -eq 1) "Setmeal creation failed: $($created.msg)"
        $page = Invoke-RestMethod "$base/api/setmeal/page?name=$mealName&categoryId=$mealCategoryId&status=0" -Headers $auth -TimeoutSec 8
        Assert-Result ($page.data.total -eq 1 -and $page.data.records[0].price -eq 52) 'Setmeal page mismatch.'
        $mealId = [long]$page.data.records[0].id
        $details = Invoke-RestMethod "$base/api/setmeal/$mealId" -Headers $auth -TimeoutSec 8
        Assert-Result ($details.data.setmealDishes[0].dishId -eq $id -and $details.data.setmealDishes[0].copies -eq 2 -and $details.data.setmealDishes[0].price -eq 29.75) 'Setmeal detail mismatch.'
        $meal.id = $mealId
        $meal.name = $editedMealName
        $meal.setmealDishes[0].copies = 3
        $changed = Send-Json '/setmeal' Put $meal $auth
        $details = Invoke-RestMethod "$base/api/setmeal/$mealId" -Headers $auth -TimeoutSec 8
        $sellMeal = Invoke-RestMethod "$base/api/setmeal/status/1?id=$mealId" -Method Post -Headers $auth -TimeoutSec 8
        $blocked = Invoke-RestMethod "$base/api/setmeal?ids=$mealId" -Method Delete -Headers $auth -TimeoutSec 8
        Assert-Result ($changed.code -eq 1 -and $details.data.name -eq $editedMealName -and $details.data.setmealDishes[0].copies -eq 3 -and $sellMeal.code -eq 1 -and $blocked.code -eq 0) 'Setmeal edit, sale, or deletion guard failed.'
        if ($IncludeMenu) {
            $public = Invoke-RestMethod "$base/user/setmeal/list?categoryId=$mealCategoryId" -TimeoutSec 8
            $contents = Invoke-RestMethod "$base/user/setmeal/dish/$mealId" -TimeoutSec 8
            Assert-Result ($public.data.Count -eq 1 -and $public.data[0].id -eq $mealId -and $contents.data[0].copies -eq 3) 'Public setmeal list or contents mismatch.'
        }
        if ($IncludeCache) {
            $cacheVersion = & $redisCli.FullName --raw GET rustsky:menu:version
            $ttl = & $redisCli.FullName --raw TTL "rustsky:menu:${cacheVersion}:setmeal:$mealCategoryId"
            Assert-Result ([int]$ttl -gt 0 -and [int]$ttl -le 60) 'Setmeal menu response was not cached with the expected TTL.'
        }
    }
    if ($IncludeCart) {
        $hot = @{dishId=$id;dishFlavor='热';amount=0.01}
        $other = @{dishId=$id;dishFlavor='常温'}
        $add = Send-Cart add $hot
        Assert-Result ($add.code -eq 1) "Could not add the first dish ($($add.msg))."
        $add = Send-Cart add $hot
        Assert-Result ($add.code -eq 1) "Could not add another portion ($($add.msg))."
        Assert-Result ((Send-Cart add $other).code -eq 1) 'Could not add a separate flavor.'
        if ($IncludeSetmeal) {
            Assert-Result ((Send-Cart add @{setmealId=$mealId}).code -eq 1) 'Could not add a setmeal.'
        }
        $cart = Invoke-RestMethod "$base/user/shoppingCart/list" -Headers $customerAuth -TimeoutSec 8
        $line = $cart.data | Where-Object { $_.dishId -eq $id -and $_.dishFlavor -eq '热' }
        $otherLine = $cart.data | Where-Object { $_.dishId -eq $id -and $_.dishFlavor -eq '常温' }
        Assert-Result ($line.number -eq 2 -and $line.amount -eq 29.75 -and $otherLine.number -eq 1) 'Cart quantities or trusted price mismatch.'
        Assert-Result ((Send-Cart sub $hot).code -eq 1) 'Could not subtract a portion.'
        $cart = Invoke-RestMethod "$base/user/shoppingCart/list" -Headers $customerAuth -TimeoutSec 8
        $line = $cart.data | Where-Object { $_.dishId -eq $id -and $_.dishFlavor -eq '热' }
        Assert-Result ($line.number -eq 1) 'Subtracting two portions did not leave one.'
        Assert-Result ((Send-Cart sub $hot).code -eq 1) 'Could not remove the last portion.'
        Assert-Result ((Send-Cart sub $other).code -eq 1) 'Could not remove a different flavor.'
        if ($IncludeSetmeal) {
            Assert-Result ((Send-Cart sub @{setmealId=$mealId}).code -eq 1) 'Could not remove the setmeal.'
        }
        if ($cartBefore.data.Count -eq 0) {
            Assert-Result ((Send-Cart add $hot).code -eq 1) 'Could not add an item before clearing.'
            $clear = Invoke-RestMethod "$base/user/shoppingCart/clean" -Method Delete -Headers $customerAuth -TimeoutSec 8
            Assert-Result ($clear.code -eq 1) 'Could not clear the cart.'
        }
        $cart = Invoke-RestMethod "$base/user/shoppingCart/list" -Headers $customerAuth -TimeoutSec 8
        Assert-Result ($cart.data.Count -eq $cartBefore.data.Count) 'Test items are still in the cart.'
    }
    if ($IncludeOrder) {
        $customerId = [long]$customer.data.id
        $fixtureAddress = "orderfixture$nonce"
        $previousShop = (Invoke-RestMethod "$base/api/shop/status" -Headers $auth -TimeoutSec 8).data
        if ($IncludeReports) {
            $reportDay = [DateTime]::UtcNow.AddHours(8).ToString('yyyy-MM-dd')
            $reportRange = "?begin=$reportDay&end=$reportDay"
            $reportBefore = Invoke-RestMethod "$base/api/report/dataOverView$reportRange" -Headers $auth -TimeoutSec 8
            Assert-Result ($reportBefore.code -eq 1) 'Report baseline could not be read.'
        }
        $newAddress = Send-Customer 'addressBook' Post @{consignee='测试';phone='13800000000';cityName='北京市';detail=$fixtureAddress}
        Assert-Result ($newAddress.code -eq 1) 'Checkout address fixture could not be added.'
        $addresses = Invoke-RestMethod "$base/user/addressBook/list" -Headers $customerAuth -TimeoutSec 8
        $addressId = [long]($addresses.data | Where-Object detail -eq $fixtureAddress | Select-Object -ExpandProperty id)
        Assert-Result ($addressId -gt 0) 'Checkout address fixture was not found.'
        Assert-Result ((Send-Cart add @{dishId=$id}).code -eq 1 -and (Send-Cart add @{dishId=$id}).code -eq 1) 'Checkout cart fixture could not be added.'
        $off = Invoke-RestMethod "$base/api/shop/0" -Method Put -Headers $auth -TimeoutSec 8
        $orderInput = @{addressBookId=$addressId;payMethod=1;deliveryStatus=1;amount=0.01;packAmount=500;remark=$fixtureAddress}
        $closed = Send-Customer 'order/submit' Post $orderInput
        Assert-Result ($off.code -eq 1 -and $closed.code -eq 0) 'A closed shop accepted an order.'
        $on = Invoke-RestMethod "$base/api/shop/1" -Method Put -Headers $auth -TimeoutSec 8
        Assert-Result ($on.code -eq 1) 'The shop did not open.'
        $orderInput.addressBookId = 9223372036854775807L
        $invalid = Send-Customer 'order/submit' Post $orderInput
        $cart = Invoke-RestMethod "$base/user/shoppingCart/list" -Headers $customerAuth -TimeoutSec 8
        Assert-Result ($invalid.code -eq 0 -and $cart.data[0].number -eq 2) 'A failed checkout changed the cart.'
        $orderInput.addressBookId = $addressId
        $order = Send-Customer 'order/submit' Post $orderInput
        Assert-Result ($order.code -eq 1 -and $order.data.orderAmount -eq 59.50) "Order checkout or trusted total failed ($($order.msg))."
        $orderId = [long]$order.data.id
        $cart = Invoke-RestMethod "$base/user/shoppingCart/list" -Headers $customerAuth -TimeoutSec 8
        $saved = & $mysql $mysqlConfig --batch --skip-column-names --database=rustsky_dev --execute="SELECT CONCAT(o.status,':',o.pay_status,':',o.amount,':',d.number,':',d.amount) FROM orders o JOIN order_detail d ON d.order_id=o.id WHERE o.id=$orderId AND o.user_id=$customerId AND d.dish_id=$id;"
        Assert-Result ($cart.data.Count -eq 0 -and $saved -eq '1:0:59.50:2:29.75') 'Checkout did not atomically save pending order, details, and clear cart.'
        $retry = Send-Customer 'order/submit' Post $orderInput
        Assert-Result ($retry.code -eq 0) 'A repeated submit with an empty cart created a second order.'
        if ($IncludeOrderManagement) {
            $history = Invoke-RestMethod "$base/user/order/historyOrders?status=1" -Headers $customerAuth -TimeoutSec 8
            $details = Invoke-RestMethod "$base/user/order/orderDetail/$orderId" -Headers $customerAuth -TimeoutSec 8
            Assert-Result (($history.data.records.id -contains $orderId) -and $details.data.orderDetailList[0].number -eq 2) 'Customer order history or detail mismatch.'
            $adminPage = Invoke-RestMethod "$base/api/order/conditionSearch?number=$($order.data.orderNumber)&page=1&pageSize=10&status=1" -Headers $auth -TimeoutSec 8
            Assert-Result ($adminPage.data.total -eq 1 -and $adminPage.data.records[0].id -eq $orderId) 'Order page filters did not find the order.'
            $early = Send-Json '/order/confirm' Put @{id=$orderId} $auth
            Assert-Result ($early.code -eq 0) 'Admin accepted an unpaid order.'
            if ($IncludeAutomation) {
                $socket = [Net.WebSockets.ClientWebSocket]::new()
                $cancel = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(8))
                try {
                    $socket.ConnectAsync('ws://127.0.0.1:8888/api/order/events', $cancel.Token).GetAwaiter().GetResult()
                    $bytes = [Text.Encoding]::UTF8.GetBytes($auth.token)
                    $socket.SendAsync([ArraySegment[byte]]::new($bytes), [Net.WebSockets.WebSocketMessageType]::Text, $true, $cancel.Token).GetAwaiter().GetResult()
                } finally {
                    $cancel.Dispose()
                }
                Assert-Result ((Receive-OrderEvent $socket).type -eq 0) 'WebSocket authentication did not complete.'
            }
            $countsBefore = (Invoke-RestMethod "$base/api/order/statistics" -Headers $auth -TimeoutSec 8).data
            $paid = Send-Customer 'order/dev/payment' Put @{id=$orderId}
            $paidAgain = Send-Customer 'order/dev/payment' Put @{id=$orderId}
            $counts = (Invoke-RestMethod "$base/api/order/statistics" -Headers $auth -TimeoutSec 8).data
            Assert-Result ($paid.code -eq 1 -and $paidAgain.code -eq 0 -and $counts.toBeConfirmed -eq ($countsBefore.toBeConfirmed + 1)) 'Simulated payment or status count failed.'
            if ($IncludeAutomation) {
                $event = Receive-OrderEvent $socket
                Assert-Result ($event.type -eq 1 -and $event.orderId -eq $orderId) 'Paid order did not trigger the new order notification.'
                $remind = Invoke-RestMethod "$base/user/order/reminder/$orderId" -Method Post -Headers $customerAuth -TimeoutSec 8
                $repeat = Invoke-RestMethod "$base/user/order/reminder/$orderId" -Method Post -Headers $customerAuth -TimeoutSec 8
                $event = Receive-OrderEvent $socket
                Assert-Result ($remind.code -eq 1 -and $repeat.code -eq 0 -and $event.type -eq 2 -and $event.orderId -eq $orderId) 'Reminder notification or frequency limit failed.'
                $socket.Dispose()
                $socket = $null
            }
            $accepted = Send-Json '/order/confirm' Put @{id=$orderId} $auth
            $started = Invoke-RestMethod "$base/api/order/delivery/$orderId" -Method Put -Headers $auth -TimeoutSec 8
            $completed = Invoke-RestMethod "$base/api/order/complete/$orderId" -Method Put -Headers $auth -TimeoutSec 8
            $details = Invoke-RestMethod "$base/api/order/details/$orderId" -Headers $auth -TimeoutSec 8
            Assert-Result ($accepted.code -eq 1 -and $started.code -eq 1 -and $completed.code -eq 1 -and $details.data.status -eq 5 -and $details.data.deliveryTime) 'Order did not advance through fulfillment.'
            if ($IncludeReports) {
                $report = Invoke-RestMethod "$base/api/report/dataOverView$reportRange" -Headers $auth -TimeoutSec 8
                $daily = Invoke-RestMethod "$base/api/report/turnoverStatistics$reportRange" -Headers $auth -TimeoutSec 8
                $dailyUsers = Invoke-RestMethod "$base/api/report/userStatistics$reportRange" -Headers $auth -TimeoutSec 8
                $dailyOrders = Invoke-RestMethod "$base/api/report/ordersStatistics$reportRange" -Headers $auth -TimeoutSec 8
                $ranking = Invoke-RestMethod "$base/api/report/top10$reportRange" -Headers $auth -TimeoutSec 8
                $workspace = Invoke-RestMethod "$base/api/workspace/businessData" -Headers $auth -TimeoutSec 8
                Assert-Result ($report.data.turnover -eq ($reportBefore.data.turnover + 59.50) -and $report.data.validOrderCount -eq ($reportBefore.data.validOrderCount + 1)) 'Completed order did not change the reporting totals.'
                Assert-Result ($daily.data.dateList -eq $reportDay -and [decimal]$daily.data.turnoverList -eq [decimal]$report.data.turnover) 'Daily turnover series mismatch.'
                Assert-Result ($dailyUsers.data.dateList -eq $reportDay -and [long]$dailyUsers.data.totalUserList -ge [long]$dailyUsers.data.newUserList) 'User series mismatch.'
                Assert-Result ($dailyOrders.data.validOrderCount -eq $report.data.validOrderCount -and $dailyOrders.data.totalOrderCount -ge $dailyOrders.data.validOrderCount) 'Order series mismatch.'
                Assert-Result ($ranking.code -eq 1 -and $ranking.data.names.Count -eq $ranking.data.quantities.Count -and $ranking.data.names.Count -gt 0) 'Sales rankings mismatch.'
                Assert-Result ($workspace.data.turnover -eq $report.data.turnover) 'Workspace and reporting totals differ.'
                $workbook = Invoke-WebRequest "$base/api/report/export$reportRange" -Headers $auth -TimeoutSec 8
                $stream = [IO.MemoryStream]::new([byte[]]$workbook.Content)
                $archive = [IO.Compression.ZipArchive]::new($stream)
                try {
                    Assert-Result ($null -ne $archive.GetEntry('xl/workbook.xml')) 'Export did not contain an Excel workbook.'
                    $reader = [IO.StreamReader]::new($archive.GetEntry('xl/worksheets/sheet1.xml').Open())
                    try { [xml]$sheet = $reader.ReadToEnd() } finally { $reader.Dispose() }
                    $cell = $sheet.SelectSingleNode("//*[local-name()='c' and @r='B2']/*[local-name()='v']")
                    Assert-Result ($cell -and [decimal]$cell.InnerText -eq [decimal]$report.data.turnover) 'Excel amount differs from the report API.'
                } finally {
                    $archive.Dispose()
                    $stream.Dispose()
                }
            }
            $repeatOrder = Invoke-RestMethod "$base/user/order/repetition/$orderId" -Method Post -Headers $customerAuth -TimeoutSec 8
            $cart = Invoke-RestMethod "$base/user/shoppingCart/list" -Headers $customerAuth -TimeoutSec 8
            Assert-Result ($repeatOrder.code -eq 1 -and $cart.data.Count -eq 1 -and $cart.data[0].number -eq 2) 'Order repeat did not restore the correct cart quantities.'
            $rejectedOrder = Send-Customer 'order/submit' Post $orderInput
            Assert-Result ($rejectedOrder.code -eq 1) 'Second order could not be placed.'
            $rejectedId = [long]$rejectedOrder.data.id
            $miniPayment = Send-Customer 'order/payment' Put @{orderNumber=$rejectedOrder.data.orderNumber;payMethod=1}
            Assert-Result ($miniPayment.code -eq 1 -and $miniPayment.data.simulated) 'Mini-program order number payment simulation failed.'
            $verified = Invoke-RestMethod "$base/user/order/payment/$($rejectedOrder.data.orderNumber)" -Headers $customerAuth -TimeoutSec 8
            Assert-Result ($verified.code -eq 1 -and $verified.data.payStatus -eq 1) 'Payment status endpoint did not confirm the simulated payment.'
            $rejected = Send-Json '/order/rejection' Put @{id=$rejectedId;rejectionReason='店内测试'} $auth
            $details = Invoke-RestMethod "$base/user/order/orderDetail/$rejectedId" -Headers $customerAuth -TimeoutSec 8
            Assert-Result ($rejected.code -eq 1 -and $details.data.status -eq 6 -and $details.data.payStatus -eq 2 -and $details.data.rejectionReason -eq '店内测试') 'Rejection did not cancel and mark simulated payment refunded.'
            if ($IncludeAutomation) {
                Assert-Result ((Send-Cart add @{dishId=$id}).code -eq 1) 'Unpaid fixture could not be added.'
                $expired = Send-Customer 'order/submit' Post $orderInput
                Assert-Result ($expired.code -eq 1) 'Unpaid fixture could not be ordered.'
                $expiredId = [long]$expired.data.id
                & $mysql $mysqlConfig --database=rustsky_dev --execute="UPDATE orders SET order_time=DATE_SUB(UTC_TIMESTAMP(), INTERVAL 20 MINUTE) WHERE id=$expiredId AND user_id=$customerId;"
                $deadline = [DateTime]::UtcNow.AddSeconds(75)
                do {
                    $expiredDetails = Invoke-RestMethod "$base/user/order/orderDetail/$expiredId" -Headers $customerAuth -TimeoutSec 8
                    if ($expiredDetails.data.status -eq 6) { break }
                    Start-Sleep -Seconds 2
                } while ([DateTime]::UtcNow -lt $deadline)
                Assert-Result ($expiredDetails.data.status -eq 6 -and $expiredDetails.data.cancelReason -eq '支付超时') 'Order maintenance did not cancel expired unpaid order.'
            }
        }
    }
    $stop = Invoke-RestMethod "$base/api/dish/status/0?id=$id" -Method Post -Headers $auth -TimeoutSec 8
    if ($IncludeCache) {
        $afterVersion = & $redisCli.FullName --raw GET rustsky:menu:version
        Assert-Result ([long]$afterVersion -gt [long]$cacheVersion) 'Menu cache version did not change after stopping a dish.'
    }
    if ($IncludeCart) {
        Assert-Result ((Send-Cart add @{dishId=$id}).code -eq 0) 'A stopped dish was added to the cart.'
    }
    if ($IncludeMenu) {
        $public = Invoke-RestMethod "$base/user/dish/list?categoryId=$categoryId" -TimeoutSec 8
        Assert-Result ($public.code -eq 1 -and $public.data.Count -eq 0) 'A stopped dish appeared on the public menu.'
    }
    if ($IncludeSetmeal) {
        $details = Invoke-RestMethod "$base/api/setmeal/$mealId" -Headers $auth -TimeoutSec 8
        $blockedDish = Invoke-RestMethod "$base/api/dish?ids=$id" -Method Delete -Headers $auth -TimeoutSec 8
        $blockedSale = Invoke-RestMethod "$base/api/setmeal/status/1?id=$mealId" -Method Post -Headers $auth -TimeoutSec 8
        $removeMeal = Invoke-RestMethod "$base/api/setmeal?ids=$mealId" -Method Delete -Headers $auth -TimeoutSec 8
        Assert-Result ($details.data.status -eq 0 -and $blockedDish.code -eq 0 -and $blockedSale.code -eq 0 -and $removeMeal.code -eq 1) 'Dish-to-setmeal status propagation or reference guard failed.'
        $items = & $mysql $mysqlConfig --batch --skip-column-names --database=rustsky_dev --execute="SELECT COUNT(*) FROM setmeal_dish WHERE setmeal_id=$mealId;"
        Assert-Result ($items -eq '0') 'Setmeal contents survived deletion.'
    }
    $remove = Invoke-RestMethod "$base/api/dish?ids=$id" -Method Delete -Headers $auth -TimeoutSec 8
    $flavors = & $mysql $mysqlConfig --batch --skip-column-names --database=rustsky_dev --execute="SELECT COUNT(*) FROM dish_flavor WHERE dish_id=$id;"
    Assert-Result ($stop.code -eq 1 -and $remove.code -eq 1 -and $flavors -eq '0') 'Dish and flavor deletion failed.'
} finally {
    if ($socket) { $socket.Dispose() }
    if ($IncludeOrder -and $fixtureAddress) {
        try {
            if ($IncludeOrderManagement) {
                & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE e FROM order_event e JOIN orders o ON o.id=e.order_id WHERE o.remark='$fixtureAddress' AND o.user_id=$customerId;"
            }
            & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE od FROM order_detail od JOIN orders o ON o.id=od.order_id WHERE o.remark='$fixtureAddress' AND o.user_id=$customerId; DELETE FROM orders WHERE remark='$fixtureAddress' AND user_id=$customerId; DELETE FROM address_book WHERE detail='$fixtureAddress' AND user_id=$customerId;"
        } finally {
            if ($null -ne $previousShop) {
                $null = Invoke-RestMethod "$base/api/shop/$previousShop" -Method Put -Headers $auth -TimeoutSec 8
            }
        }
    }
    if ($IncludeCart) {
        $customerId = [long]$customer.data.id
        if ($id) { & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE FROM shopping_cart WHERE user_id=$customerId AND dish_id=$id;" }
        if ($mealId) { & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE FROM shopping_cart WHERE user_id=$customerId AND setmeal_id=$mealId;" }
    }
    if ($IncludeSetmeal) {
        & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE sd FROM setmeal_dish sd JOIN setmeal s ON s.id=sd.setmeal_id WHERE s.name IN ('$mealName','$editedMealName') AND s.create_user=$actor; DELETE FROM setmeal WHERE name IN ('$mealName','$editedMealName') AND create_user=$actor; DELETE FROM category WHERE name='$mealCategory' AND create_user=$actor;"
    }
    & $mysql $mysqlConfig --database=rustsky_dev --execute="DELETE f FROM dish_flavor f JOIN dish d ON d.id=f.dish_id WHERE d.name IN ('$name','$updatedName') AND d.create_user=$actor; DELETE FROM dish WHERE name IN ('$name','$updatedName') AND create_user=$actor; DELETE FROM category WHERE name='$category' AND create_user=$actor;"
    if ($imagePath -match '^/uploads/[a-f0-9]{32}[.]jpg$') {
        Remove-Item -LiteralPath (Join-Path $root ('.preview' + $imagePath))
    }
}

Write-Host 'Dish image, create, filters, detail, flavor replacement, status, and guarded deletion passed.'
if ($IncludeSetmeal) { Write-Host 'Setmeal lifecycle, dish-to-setmeal status propagation, and reference guards passed.' }
if ($IncludeMenu) { Write-Host 'Public menu categories, dishes, flavors, setmeals, and sale visibility passed.' }
if ($IncludeCart) { Write-Host 'Cart insert, quantity merge, flavors, trusted price, subtraction, and sale guard passed.' }
if ($IncludeOrder) { Write-Host 'Checkout shop/address guards, transaction, trusted total, and repeat submission passed.' }
if ($IncludeOrderManagement) { Write-Host 'Order filters, history, simulated payment, fulfillment, and rejection passed.' }
if ($IncludeCache) { Write-Host 'Redis menu cache keys, TTL, and invalidation passed.' }
if ($IncludeAutomation) { Write-Host 'WebSocket new orders, reminders, frequency guard, and overdue-order cancellation passed.' }
if ($IncludeReports) { Write-Host 'Daily turnover, users, orders, ranking, workspace, and Excel contents passed.' }

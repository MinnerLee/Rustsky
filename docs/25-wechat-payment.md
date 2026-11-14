# Rustsky 学习笔记 25 · 微信支付预下单、验签和退款

## 从顾客点下付款开始

1. 小程序把订单号提交给 `PUT /user/order/payment`。Rust 从 JWT 取得用户 ID，再从 MySQL 取得这个用户的订单金额和 OpenID。例如 59.50 元在微信支付接口中传作整数 `5950` 分。
2. Rust 用商户私钥给 HTTP 请求签名，向微信申请预支付 ID，并用它生成 `wx.requestPayment` 要的参数。短时间内并发请求同一订单时，只有一个请求会对外预下单。已取得预支付 ID 时，在该订单的付款期限内复用它。
3. 呼起微信收银台后，小程序请求 `GET /user/order/payment/{number}` 让后端确认结果。付款也会触发微信主动发送 `POST /notify/wechat/payment`，两条路径使用同一个入库函数。[小程序支付开发指引](https://pay.wechatpay.cn/doc/v3/merchant/4012791911)

## 回调为什么要先验签再解密

把回调想成带封条的信。**验签**检查发送方与内容是否可信，用的是微信支付公钥。**解密**读出回调里的订单信息，用的是 APIv3 密钥。Rust 直接对 HTTP 原始正文验签，JSON 重新排版会改变字节，即使字段的值一样也验不过。[微信支付签名规则](https://pay.wechatpay.cn/doc/v3/merchant/4013053420) · [回调解密规则](https://pay.wechatpay.cn/doc/v3/merchant/4012071382)

验签通过后，Rust 继续核对商户号、App ID、订单号、付款人和金额。MySQL 在同一个事务里写入微信交易号、更新订单状态和创建来单事件。微信重复送达同一笔付款时，服务端返回成功，订单与事件只写一次，这就是这里的**幂等**。

## 超时和退款

超过 15 分钟的订单，若已发起微信支付，后台先查微信订单。已支付时补记付款，仍未支付时关单后再取消本地订单。顾客或管理员手动取消已发起支付的未付款订单，需等这一轮查单完成。已支付的待接订单可以进入全额退款。退款申请和查询都复用同一商户退款单号，微信确认退款成功后才把 `payStatus` 改为 `2`。管理端订单详情显示等待提交、处理中、成功、关闭或异常。关闭和异常需到商户平台核对处理。[微信退款开发指引](https://pay.wechatpay.cn/doc/v3/merchant/4013071031)

## 本地验证

```powershell
Set-Location D:\rust\Rustsky
cargo test --locked --offline order::wechat
& .\tools\test-dish.ps1 -IncludeSetmeal -IncludeCache -IncludeOrderManagement -IncludeReports
```

Rust 测试使用临时 RSA 密钥，验证预下单签名、收银台参数、回调解密、正文篡改和过期时间戳。数据库回归还会验证并发预下单只向本地模拟服务请求一次、错误金额被拒、同一付款通知只生成一条来单事件，以及签名关单和退款状态更新。它需要已迁移的本地测试库，以及只在当前 PowerShell 进程设置的 `RUSTSKY_TEST_DATABASE_URL`。运行命令为 `cargo test --locked --offline signed_notifications_update_mysql_once_and_refund_only_on_success -- --ignored`。

## 配置真实联调

已有库按顺序执行 `db/011-wechat-payment.sql` 和 `db/012-wechat-prepay.sql`。真实登录需要 `RUSTSKY_WX_APP_ID`、`RUSTSKY_WX_APP_SECRET` 和独立的用户 JWT 密钥。额外的支付配置由启动 Rust 的进程提供：

| 环境变量 | 来源 |
| --- | --- |
| `RUSTSKY_WXPAY_MCH_ID` | 已与小程序绑定的商户号 |
| `RUSTSKY_WXPAY_MERCHANT_SERIAL` | 商户 API 证书序列号 |
| `RUSTSKY_WXPAY_PRIVATE_KEY` | 商户 API 私钥 PEM 文件路径 |
| `RUSTSKY_WXPAY_PUBLIC_KEY_ID` | 微信支付公钥 ID |
| `RUSTSKY_WXPAY_PUBLIC_KEY` | 微信支付公钥 PEM 文件路径 |
| `RUSTSKY_WXPAY_API_V3_KEY` | 商户 APIv3 密钥 |
| `RUSTSKY_WXPAY_NOTIFY_URL` | 公网 HTTPS 回调，例如 `https://你的域名/notify/wechat/payment` |

当前验签配置使用微信支付公钥模式。生产代理需要将上述回调路径转给 Rust，开发机的回环地址只供本机使用。给微信开发者工具生成正式联调副本时，运行 `node tools/configure-miniprogram.cjs <小程序AppID> <HTTPS源地址>`。文件输出到 `.preview/miniprogram-wechat`，会使用真正的微信登录和 HTTPS 请求。真实登录、收款和退款仍需商户环境点验。[微信支付接入准备](https://pay.wechatpay.cn/doc/v3/merchant/4015459512)

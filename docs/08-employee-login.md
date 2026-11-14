# Rustsky 学习笔记 08 · 完善员工登录

第 12 集给员工表加上密码哈希校验。Java 课程用 MD5 演示，Rustsky 已经用 Argon2id 保存并验证带盐的密码哈希。本节在这个基础上加了登录参数校验、JWT 验证和管理端已有的退出接口。[课程第 12 集](https://www.bilibili.com/video/BV1TP411v7v6?p=12) · [课程讲义镜像](https://github.com/shuhongfan/sky-take-out)

## 密码和令牌各有什么用

**密码用于登录。** Rust 从 MySQL 的 `employee` 表查到启用的员工，再用 Argon2 验证输入的密码能否生成相同的哈希。哈希是一种单向计算。数据库保存的是包含盐和参数的哈希字符串，程序从它校验密码。盐让相同的密码也可以得到不同的存储结果，Argon2 的计算成本让大量离线猜测更慢。

**令牌用于登录之后。** 密码正确时，Rust 签发一个有效期一小时的 JWT，里面有员工 ID 和到期时间。前端保存 JWT，后续请求放到 `token` 请求头。后端用签名检查令牌是否被改动，并检查是否过期。两个 Rust 实例共用同一个签名密钥，所以 Nginx 可以把登录交给 `8080`，再把退出交给 `8081`。

## 一次登录和退出怎样走完

| 请求 | Rustsky 的处理 |
| --- | --- |
| `POST /api/employee/login` | 校验输入，查启用员工并核对密码。成功返回 `code: 1` 和 `data.token`。错误账号或密码返回 `code: 0`，前端显示错误消息。 |
| `POST /api/employee/logout` | 验证 `token` 请求头的 JWT 签名和有效期。成功后前端删除本地登录信息并回到登录页。令牌缺失、过期或无效时返回 `401`。 |

Nginx 会把 `/api/` 前缀映射成 `/admin/`。`src/employee.rs` 中的 `authenticate_employee` 是 Axum 中间件，先于受保护的 handler 执行。登录路由在中间件之外，因为第一次登录时还没有令牌。退出沿用课程的无状态 JWT 做法，清除的是浏览器里的凭证。服务端目前没有撤销列表，已签发的令牌在有效期内仍可通过验证。

## 跟着请求验证

打开本地管理端，按 F12 到 Network，用本地 `admin` 登录，再选择退出。可以看到登录响应有 `data.token`，退出请求则带 `token` 请求头。两个请求的 `X-Rustsky-Upstream` 可能不同，这说明请求落到了不同的 Rust 实例。

也可以在终端验证一个边界情况：下面这条退出请求没有带令牌，应得到 HTTP `401`。

```powershell
curl.exe -i -X POST http://127.0.0.1:8888/api/employee/logout
```

本地环境的启动方式见 [学习笔记 06](06-reverse-proxy-load-balancing.md)。

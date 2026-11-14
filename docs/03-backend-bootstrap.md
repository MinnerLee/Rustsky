# Rustsky 学习笔记 03 · 后端初始工程

第 6 集「Day01-05 后端环境搭建：熟悉项目结构」打开的是课程资料里的 `day01/后端初始工程/sky-take-out`。Java 工程以父 `pom.xml` 管理 `sky-common`、`sky-pojo` 和 `sky-server`。本地参考来自公开镜像的早期 Day 01 提交 `f0081b5`，该提交已推进到后续小节，和视频打开的原始压缩包可能有差异。[课程第 6 集](https://www.bilibili.com/video/BV1TP411v7v6?p=6) · [Java 参考工程](https://github.com/lm101845/SkyTakeOut/tree/f0081b5/02-code/sky-take-out)

## Rust 代码放在哪里

| Java 工程的职责 | Rustsky 的位置 |
| --- | --- |
| `sky-server` 的应用入口和路由 | `src/main.rs` 启动 Tokio、监听端口并挂载 Axum 路由 |
| `sky-common` 的统一结果 | `src/common.rs` 定义泛型 `ApiResult<T>`，统一返回 `code`、`msg`、`data` |
| `sky-pojo` 的登录请求和响应 | `src/employee.rs` 定义 `LoginRequest` 和 `LoginResponse` |
| `sky-server` 的员工登录逻辑 | `src/employee.rs` 的 handler 读 HTTP 请求，`LoginService` 校验账号并签发令牌 |

`Router` 将 `POST /admin/employee/login` 交给 handler。`Json<LoginRequest>` 让 Axum 和 Serde 把请求体转成 Rust 结构体。`State` 给 handler 提供启动时配置的服务。业务函数用 `Result<LoginResponse, LoginError>` 区分成功和失败，handler 再决定 HTTP 状态和 JSON 内容。`#[serde(rename_all = "camelCase")]` 让 Rust 字段 `user_name` 对应前端使用的 `userName`。

登录成功时返回 `code: 1` 和 `data.token`，凭证不正确时返回 `code: 0` 和错误消息。JWT 的签名可以用来校验内容是否被改动。这里令牌包含员工编号 `empId` 和有效期 `exp`，有效期为一小时。后续接入受保护接口时，需要在接受请求前验证签名和有效期。

## 用 PowerShell 验证

先在 Rustsky 项目目录启动服务。默认只监听本机 `127.0.0.1:8080`，可以用 `RUSTSKY_PORT` 修改端口。如果不配置下面两个变量，仍可访问健康检查，员工登录会返回 `503`。

```powershell
Set-Location D:\rust\Rustsky
$env:RUSTSKY_DEV_PASSWORD = 'rustsky-local-demo'
$env:RUSTSKY_JWT_SECRET = [Convert]::ToHexString([System.Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
cargo run --locked
```

另开一个 PowerShell 窗口，检查服务和登录响应：

```powershell
Invoke-RestMethod http://127.0.0.1:8080/health
Invoke-RestMethod http://127.0.0.1:8080/admin/employee/login `
  -Method Post -ContentType application/json `
  -Body '{"username":"admin","password":"rustsky-local-demo"}'
```

把密码换成错误值会得到 `code: 0`。把请求体换成 `{}` 会得到 HTTP `400`。这些行为已通过本机 HTTP 请求验证。

当前阶段只有一个通过环境变量配置的本地演示员工。MySQL 员工表、数据库密码哈希和管理端鉴权中间件留到对应小节。之前的静态预览服务器只分发文件，浏览器联调还需要开发代理把 `/api/employee/login` 转到 Rust 的 `/admin/employee/login`。

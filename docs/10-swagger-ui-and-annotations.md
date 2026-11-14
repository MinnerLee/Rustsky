# Rustsky 学习笔记 10 · Swagger UI 和接口注解

第 14 集介绍 Swagger 的用途，第 15 集介绍怎样用注解写接口说明。Rustsky 用 `utoipa` 生成 OpenAPI，用 Swagger UI 把它显示成可以发送请求的网页。[课程第 14 集](https://www.bilibili.com/video/BV1TP411v7v6?p=14) · [课程第 15 集](https://www.bilibili.com/video/BV1TP411v7v6?p=15)

## 从餐厅的点单规则到能试点单的页面

上一节把接口文档比作餐厅前台和厨房共用的点单规则。用这个比喻继续看三个东西：

| 名称 | 点单场景 | 在 Rustsky 里做什么 |
| --- | --- | --- |
| OpenAPI | 一份有固定格式的点单规则 | 记录窗口地址、请求方法、必填字段和可能的回复。可以通过 `/openapi.json` 读取。 |
| `utoipa` | 把厨房贴在工位旁的说明汇总成规则册的人 | 从 Rust handler 的 `#[utoipa::path]` 和数据类型的 `ToSchema` 生成 OpenAPI。 |
| Swagger UI | 有示例、能试着下单的电子菜单 | 读取 OpenAPI，把每个接口显示成可展开的卡片，并能发真实 HTTP 请求。 |

点 **Execute** 后，Swagger UI 会把表单内容变成一个真正的 HTTP 请求。以员工登录为例，请求会经过 Nginx、Rust 和 MySQL，与管理端登录页走的是同一条路。Swagger UI 只是换了一个方便调试的客户端，因此如果 Rust 停了，页面上也不会凭空出现登录结果。

```text
Rust 注解和数据类型 → utoipa → OpenAPI JSON → Swagger UI 页面
                                               ↓ 点击 Execute
                                      Nginx → Rust → MySQL
```

## 第一次打开，按这个顺序操作

1. 打开 [Swagger UI](http://127.0.0.1:8888/docs/)，展开 **GET /health**，点 **Try it out** → **Execute**。预期看到 HTTP `200`，响应中的 `data` 是 `"ok"`。这说明 Rust 能响应 HTTP 请求。
2. 展开 **POST /api/employee/login**，点 **Try it out**。把 JSON 中的 `username` 改为 `admin`，`password` 填本地配置 `.preview/mysql/local-settings.json` 的 `employeePassword`。执行后看响应中的 `code`。`1` 表示成功，`data.token` 是这次签发的登录令牌。密码错误时通常仍是 HTTP `200`，但 JSON 的 `code` 为 `0`。
3. 如果要试退出，点页面右上的 **Authorize**。在 `EmployeeToken` 里直接粘贴 `data.token` 的值，不带 `Bearer` 前缀，然后执行 **POST /api/employee/logout**。Swagger UI 会自动加上 `token` 请求头。未授权时退出接口返回 `401`。
4. 退出后 Swagger UI 仍可能保留刚才填写的授权值。通过 **Authorize** 面板的 **Logout** 清掉它。Rustsky 的退出让管理端清理本地登录状态，服务端的 JWT 仍会到有效期才失效。

Swagger UI 中的 **Example Value** 是说明用的样例。实际请求结果要看执行后的 **Server response**。样例里的 `<jwt>` 不是有效令牌。

## 注解怎么读

Java 课程中的注解可以按职责对应到 Rust：

| 想说明的内容 | Java 课程 | Rustsky |
| --- | --- | --- |
| 接口归到哪个分组，叫什么 | `@Api`、`@ApiOperation` | `#[utoipa::path(tag = "员工", summary = "员工登录", …)]` |
| 接口方法、路径、入参和响应 | Controller 的映射与 Swagger 注解 | `post`、`path`、`request_body`、`responses` |
| JSON 对象有哪些字段，每个字段是什么意思 | `@ApiModel`、`@ApiModelProperty` | `#[derive(ToSchema)]`、字段的 `///` 注释和 `#[schema(...)]` |
| 哪些接口需要携带登录凭证 | Swagger 的认证配置 | `security(("EmployeeToken" = []))` 关联 `token` 请求头 |

打开 `src/employee.rs` 可以看完整实现。以登录为例，`request_body = LoginRequest` 告诉文档使用现有的 `LoginRequest` 结构体，`ToSchema` 会把 `username: String` 和 `password: String` 写成 JSON 字段。登录响应的 `user_name` 在 Rust 中用下划线，通过 Serde 的 `camelCase` 规则对外显示为 `userName`，文档也沿用这个名字。

两个地方容易混淆。`#[utoipa::path]` 描述接口，不会创建路由，真正注册 Rust 路由的是 Axum 的 `.route()` 和 `.nest()`。`security` 告诉 Swagger UI 调试时加哪个请求头，真正判断令牌是否有效的是后端的 `authenticate_employee` 中间件。浏览器看到的 `/api/employee/login` 由 Nginx 转成 Rust 的 `/admin/employee/login`，所以注解写的是调用者真正使用的地址。

## 之后怎么维护

新增接口时，先注册 Axum 路由，再在 handler 上加 `#[utoipa::path]`，让请求和响应类型派生 `ToSchema`，然后把 handler 加到 `src/api_docs.rs` 的 `paths(...)`。重启服务后打开 Swagger UI，检查示例并实际执行一次。在线 `/openapi.json` 由正在运行的 Rust 代码生成。需要把同一份定义导入 Apifox 时，可以导出文件：

```powershell
cargo run --locked -- --export-openapi | Set-Content -Encoding utf8 docs/openapi.json
```

Apifox 若要求设置服务器地址，填 `http://127.0.0.1:8888`。重启命令见 [学习笔记 06](06-reverse-proxy-load-balancing.md)。[Swagger UI 官方说明](https://swagger.io/tools/swagger-ui/) · [utoipa 接口注解参考](https://docs.rs/utoipa/latest/utoipa/attr.path.html)

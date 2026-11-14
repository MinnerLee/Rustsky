# Rustsky 学习笔记 09 · 接口文档是前后端的约定

第 13 集把课程提前写好的接口定义导入 YApi，让前端、后端和测试都能查阅同一份约定。Rustsky 用 [OpenAPI 文件](openapi.json) 记录当前能运行的登录、退出和健康检查接口。[课程第 13 集](https://www.bilibili.com/video/BV1TP411v7v6?p=13) · [OpenAPI 规范](https://spec.openapis.org/oas/v3.0.3)

## 像餐厅前台和厨房共用的点单规则

餐厅前台和厨房先约定，点单时把菜名、份数、桌号写在单子上。厨房接单后给一个取餐号，菜卖完时告诉前台原因。双方只要遵守约定，前台就能先做点单页面，厨房也能同时准备接单流程。

| 点单规则里的约定 | Rustsky 员工登录里的对应内容 |
| --- | --- |
| 把单子送到哪个窗口，用什么动作提交 | `POST /api/employee/login` |
| 单子上要填什么 | JSON 里的 `username` 和 `password` |
| 办成后拿到什么 | `code: 1`，以及 `data.token` 登录凭证 |
| 办不成如何告知 | 密码错误时 `code: 0`，`msg` 说明原因 |
| 取餐时要出示什么 | 退出请求带 `token` 请求头 |

前端照文档组装请求和显示结果，后端照文档接收字段和返回数据，测试照文档判断行为是否正确。比如前端约定读取 `data.token`，后端却返回 `data.accessToken`，即使密码校验通过，前端也拿不到令牌。联调时先核对这份约定，就知道该修哪一边。只要约定一致，Java 和 Rust 后端都能与同一套前端通信。

## 打开、导入和调试

OpenAPI 是用 JSON 或 YAML 表达接口的标准格式，Apifox 等工具读它后，可以自动列出接口、表单和返回示例。文档本身是说明书，发出请求时仍然需要 Rust 服务实际运行。

1. Rustsky 启动后，打开 [本地 OpenAPI JSON](http://127.0.0.1:8888/openapi.json)。这份文档由 Rust 注解生成，`docs/openapi.json` 是可供文件导入的导出副本。浏览器交互调试见 [Swagger UI 学习笔记](10-swagger-ui-and-annotations.md)。
2. 使用 Apifox 时，在项目的数据导入功能选 **OpenAPI/Swagger**，选择 `docs/openapi.json`。本机可访问 `127.0.0.1` 的桌面客户端也可以尝试用上面的本地 URL 导入。[Apifox 官方导入说明](https://docs.apifox.com/5837253m0)
3. 调试「员工登录」时填写本地 `admin` 的密码。接口的服务器地址为 `http://127.0.0.1:8888`。退出时把登录返回的 `data.token` 填入 `token` 请求头。`/health` 可以通过 Nginx 访问，也可以直连 Rust 的 `8080` 或 `8081` 检查单个实例。

登录失败也可能返回 HTTP `200`。这说明 HTTP 请求处理成功，但登录这件事没成功，前端要继续看 JSON 中的 `code`。而退出时缺少令牌，接口直接返回 HTTP `401`。

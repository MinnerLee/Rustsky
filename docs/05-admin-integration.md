# Rustsky 学习笔记 05 · 前后端联调

这节用员工登录串起课程的 Vue 管理端、Rust API 和 MySQL。[课程第 9 集](https://www.bilibili.com/video/BV1TP411v7v6?p=9)

```text
浏览器 http://127.0.0.1:8888/sky/
    POST /api/employee/login
          ↓ 本地预览代理把 /api/ 改成 /admin/
Rust API http://127.0.0.1:8080/admin/employee/login
          ↓ SQLx 查询 employee，Argon2 校验密码
MySQL rustsky_dev（127.0.0.1:3306）
          ↑ Rust 签发 JWT，代理把 JSON 返回给浏览器
```

`tools/preview.py` 同时提供静态页面和 `/api/` 代理。浏览器始终向 `8888` 发请求，页面和接口因此属于同源，开发环境无需单独配置 CORS。代理保留请求方法、查询参数、JSON 请求体和 `token` 请求头。`frontend/vue.config.js` 中的 Vue 开发代理也使用相同的路径映射。

Vue 登录页调用 `frontend/src/api/employee.ts`，拿到 `code: 1` 后把 `data.token` 存起来。错误密码仍返回 HTTP `200`，业务结果 `code: 0`，这是课程前端读取登录错误的约定。格式不正确的请求返回 HTTP `400`。后端没有运行时，代理返回 HTTP `502`。

## 启动与检查

当前本机预览由 Nginx 提供，启动方式见 [反向代理与负载均衡笔记](06-reverse-proxy-load-balancing.md)。也可以用本节的 Python 单实例代理复现联调。先关闭占用 `8888` 的 Nginx，按 [MySQL 环境笔记](04-mysql-setup.md) 启动 MySQL 和 Rust API，再在 Rustsky 目录运行：

```powershell
python tools/preview.py
```

打开 [本地管理端](http://127.0.0.1:8888/sky/)，用户名填 `admin`，密码取自 `.preview/mysql/local-settings.json` 的 `employeePassword` 字段。浏览器开发者工具的 **Network** 面板可以查看 `employee/login` 请求和响应；截图或分享日志时应避开请求密码和响应令牌。

排查时先访问 `http://127.0.0.1:8080/health` 确认 Rust 服务，再比较 `8080/admin/employee/login` 和 `8888/api/employee/login` 的响应。直连接口成功而代理失败时，检查 `8888` 是否运行的是 `tools/preview.py`。

本节用登录讲解联调。当前工作台还可查询营业状态、订单和统计数据，分别见 [第 18 篇](18-redis-shop-menu.md)、[第 21 篇](21-menu-cache-order-management.md) 和 [第 23 篇](23-reports-workspace-excel.md)。

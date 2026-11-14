# Rustsky 学习笔记 14 · 员工分页查询

第 21–24 集依次讲需求设计、代码开发、功能测试和时间格式完善。Rustsky 实现了同一套分页流程，并在员工列表中显示之前保存的工号。[课程分集](https://www.bilibili.com/video/BV1TP411v7v6?p=21)

## 第 21 集 · 接口如何约定

打开 [员工管理](http://127.0.0.1:8888/sky/#/employee) 时，浏览器请求 `GET /api/employee/page?page=1&pageSize=10`。输入姓名时带上 `name`。`page` 和 `pageSize` 省略时分别默认为 1 和 10，单页最多 100 条。接口需要管理员登录令牌。

响应里 `data.total` 是符合条件的总条数，`data.records` 是当前页的员工数组。例如查到 23 人，每页 10 人，第一页返回 10 条记录，`total` 仍为 23。前端用 23 算出一共有三页。

## 第 22 集 · Rust 和 MySQL 怎样分页

Axum 用 `Query<EmployeePageQuery>` 解析 URL 参数。Nginx 把浏览器的 `/api/employee/page` 转到 Rust 的 `/admin/employee/page`。中间件从 JWT 取得员工 ID，查询先确认该员工是启用的管理员。SQLx 接着在同一个事务里执行两条查询：

1. 用 `COUNT(*)` 计算符合姓名条件的总数。
2. 用相同的条件，按 `update_time DESC, id DESC` 排序，再取 `LIMIT pageSize OFFSET (page - 1) * pageSize`。

比如 `page=2&pageSize=10`，就先跳过 10 条，再取接下来的 10 条。两条记录的修改时间相同时，用 `id` 决定先后，避免排序不确定。姓名搜索用 `LOCATE` 匹配包含的文字，所以用户输入 `%` 时会按字面搜索。

响应只返回列表需要的字段。你之前要求保存的工号以 `employeeNo` 返回，没分配工号的旧员工会显示 `—`。列表不传密码哈希和身份证号。

## 第 23 集 · 怎么验证分页

用 `admin` 登录后打开员工管理，试试查询姓名、清空搜索和切换每页数量。提交新搜索和修改每页数量都会回到第一页。也可以在 [Swagger UI](http://127.0.0.1:8888/docs/) 里登录、授权，再执行 `GET /api/employee/page`，直接看 `total` 和 `records`。

项目提供一份可重复运行的测试。它通过新增接口生成三条虚构员工，验证两页分别返回两条和一条，并核对工号、排序、格式、空结果、超出末页和权限。测试结束会清理本次记录。

```powershell
& D:\rust\Rustsky\tools\test-employee-page.ps1
```

## 第 24 集 · 时间如何显示

课程把最后操作时间格式化为 `yyyy-MM-dd HH:mm:ss`。Rustsky 用 MySQL 的 `DATE_FORMAT` 把 `update_time` 映射成相同格式的 `updateTime` 字符串，例如 `2026-11-11 09:30:00`。这是库内时间的格式化，查询不另外做时区转换。旧记录没有操作时间时返回 `null`，前端显示 `—`。

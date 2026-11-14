# Rustsky 学习笔记 13 · 新增员工的实现、测试和操作人

这次对应课程的新增员工代码开发、功能测试和代码完善 2。第 16 集的 [需求和工号规则](12-create-employee.md) 已经在 Rustsky 中实现，这里重点看一次新增请求怎么走完，以及怎样判断结果真的正确。[第 17 集](https://www.bilibili.com/video/BV1TP411v7v6?p=17) · [第 18 集](https://www.bilibili.com/video/BV1TP411v7v6?p=18) · [第 20 集](https://www.bilibili.com/video/BV1TP411v7v6?p=20)

## 第 17 集 · 把表单接到数据库

```text
Vue 表单 → Nginx 转发 → Axum 验证 JWT 和表单 → SQLx 执行 INSERT → MySQL 保存
```

在 `src/employee/create.rs` 中，`CreateEmployeeRequest` 是请求的数据结构。`employeeNo` 是前端 JSON 字段名，Serde 把它解析成 Rust 的 `employee_no`。Rust 验证字段，把初始密码做 Argon2id 哈希，再用 `.bind(...)` 把值交给 SQL。SQL 模板里的 `?` 表示参数位置，SQLx 会把表单值作为数据发送给数据库。

数据库表里的 `employee_no` 是字符串列，有唯一索引。因此 `RS-0007` 能保留前导零，同一个工号只能保存一次。两个请求同时新增同一个工号时，仍由 MySQL 在写入时检查冲突。Rust 把唯一约束错误转成管理端能显示的「账号或工号已存在」。

## 第 18 集 · 测成功，也测失败

只看 Swagger 返回 `code: 1` 还不足以验证保存结果。脚本会用登录接口取得管理员 JWT，再发送新增请求，最后直接查询 MySQL 核对工号、启用状态、密码哈希、创建与修改时间以及操作人。它也会用新员工的初始密码登录一次。

| 验证场景 | 预期结果 |
| --- | --- |
| 有效管理员新增员工 | `code: 1`，返回员工 ID 和工号，数据库有对应记录 |
| 缺少工号 | HTTP `400` |
| 未带令牌 | HTTP `401` |
| 普通员工提交新增 | HTTP `403` |
| 工号或账号已存在 | HTTP `200`，业务结果 `code: 0` |
| 两个不同账号同时提交同一工号 | 一个成功，一个冲突，数据库只留一条 |

本地服务运行后，在 PowerShell 7 中执行：

```powershell
& D:\rust\Rustsky\tools\test-create-employee.ps1
```

脚本使用随机账号和虚构员工信息，结束时清理本次测试创建的记录。也可以在 [Swagger UI](http://127.0.0.1:8888/docs/) 里先登录并授权，再手动调用 `POST /api/employee`。

## 第 20 集 · 创建人从哪里来

`employee.id` 是新员工自己的编号。`create_user` 和 `update_user` 则记录是谁操作了这条记录。新增时，同一个管理员既是创建人，也是最后一次修改的人，所以两个字段写入相同的管理员 ID。

课程在 Java 拦截器解析 JWT 后，用 `ThreadLocal` 把当前员工 ID 传到业务层。Rustsky 的做法是让 Axum 中间件 `authenticate_employee` 将验证后的 `empId` 放入这次请求的扩展数据。新增 handler 用 `Extension<EmployeeIdentity>` 取出这个 ID，SQL 再确认该员工是启用的 `admin`，并用查到的 ID 写审计字段。请求扩展数据随当前请求结束，每个请求携带自己的身份。

自动测试故意在 JSON 里填入另一个 `createUser` 和 `updateUser`，再查询数据库。正确结果是两个字段都等于管理员登录响应里的 ID。这能检验操作人确实由后端从认证上下文取得。

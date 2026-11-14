# Rustsky 学习笔记 12 · 新增员工与工号

第 16 集是 Day02-01「新增员工：需求分析和接口设计」。Rustsky 在课程的员工表单上增加了必填的工号和初始密码。登录账号用于登录，工号用于标识员工，`id` 是 MySQL 分配的主键。三者各有用途。[课程第 16 集](https://www.bilibili.com/video/BV1TP411v7v6?p=16)

## 表单怎样写到 MySQL

1. Vue 将账号、姓名、工号、手机号、性别、身份证号和初始密码提交到 `POST /api/employee`。Nginx 把它转给 Rust 的 `POST /admin/employee`。
2. Rust 检查令牌和输入字段。数据库写入语句同时确认操作人是启用的 `admin`，并记录 `create_user` 和 `update_user`。
3. Rust 用 Argon2id 计算初始密码哈希，再用 SQLx 的 `.bind()` 把值填入 SQL 参数。数据库只保存哈希。
4. MySQL 将工号写到 `employee.employee_no`。保存成功时返回 `data.id` 和 `data.employeeNo`。账号或工号重复时，接口返回 `code: 0` 和错误消息。

工号用字符串保存，这样 `0007` 的前导零会保留。新工号允许英文字母、数字、横线和下划线，必须以字母或数字开头，最长 32 位。字母统一转为大写，因此 `rs-0007` 和 `RS-0007` 视为同一个工号。原有员工暂时可以没有工号，新建员工时必须填写。

**唯一索引**是数据库自己的约束。即使两个 Rust 实例同时收到相同工号，MySQL 也只允许一条写入成功。这里的迁移是 `db/001-employee-number.sql`，它在原有表上增加列和索引，无需重新导入课程 SQL。

## 自己试一次

1. 用本地 `admin` 登录，打开 [添加员工页面](http://127.0.0.1:8888/sky/#/employee/add)，填写工号和初始密码后保存。测试时使用虚构员工信息。
2. 也可以用 [Swagger UI](http://127.0.0.1:8888/docs/) 调试。先执行员工登录，再把返回的 `data.token` 填到 **Authorize → EmployeeToken**，然后执行 **POST /api/employee**。
3. 用另一个账号填同一个工号，确认返回冲突。要在数据库核对保存结果，可以查询 `SELECT id, username, employee_no FROM employee WHERE employee_no = 'RS-0007';`。这条查询只读编号、账号和工号。

本机已经执行迁移，并给 `rustsky_app` 增加了 `employee` 表所需列的 `INSERT` 权限。换一套数据库时，先按 [MySQL 环境笔记](04-mysql-setup.md) 准备数据，再用管理员在 `rustsky_dev` 执行迁移和授权：

```sql
USE rustsky_dev;
ALTER TABLE employee MODIFY password VARCHAR(255) NOT NULL;
SOURCE D:/rust/Rustsky/db/001-employee-number.sql;
GRANT INSERT (username, name, employee_no, password, phone, sex, id_number,
              status, create_time, update_time, create_user, update_user)
ON rustsky_dev.employee TO 'rustsky_app'@'127.0.0.1';
```

Vue 源码在 `frontend/src/views/employee/addEmployee.vue`。在现有 Node 24 环境构建这套旧版 Vue CLI 时，可以执行 `npm ci --ignore-scripts --legacy-peer-deps`，然后运行 `node --openssl-legacy-provider node_modules/@vue/cli-service/bin/vue-cli-service.js build --dest ../.preview/admin-build`。员工列表见 [第 14 篇](14-employee-pagination.md)，编辑和禁用见 [第 15 篇](15-employee-status-edit-category.md)。

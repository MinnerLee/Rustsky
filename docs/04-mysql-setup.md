# Rustsky 学习笔记 04 · 搭建 MySQL 环境

Java 版和 Rust 版都可以连接同一个 MySQL 实例，复用课程的表和 SQL。语言的差别在应用侧。Java 课程通过 JDBC 和 MyBatis 查询数据库，Rustsky 通过 SQLx 的 `MySqlPool` 建立连接池，并用参数化 SQL 查询数据。[MySQL Windows 安装文档](https://dev.mysql.com/doc/refman/8.4/en/windows-installation.html) · [SQLx MySqlPool 文档](https://docs.rs/sqlx/latest/sqlx/mysql/type.MySqlPool.html)

## 本机已搭好的环境

MySQL 8.4.13 解压在 `.preview/mysql/`，只监听 `127.0.0.1:3306`。`rustsky_dev` 已导入课程的 11 张表并应用项目迁移。应用通过 `rustsky_app` 账号连接，该账号有查询权限和各模块需要的写权限。`.preview/` 已被 Git 忽略，数据库文件和凭证都放在这个目录。MySQL 用的是便携版，重启 Windows 后可以手动启动，数据会保留。

运行完整项目时，用 PowerShell 执行 `& D:\rust\Rustsky\tools\start-local-stack.ps1`，一次启动 MySQL、Redis、Rust 和 Nginx。下面的两个窗口用于单独练习 MySQL 和员工登录。

如果两个进程已经在运行，可以直接验证登录。如果需要重启，在两个 PowerShell 窗口分别执行：

```powershell
# 窗口 1，启动数据库
Set-Location D:\rust\Rustsky
& .\.preview\mysql\mysql-8.4.13-winx64\bin\mysqld.exe --defaults-file=D:/rust/Rustsky/.preview/mysql/my.ini --console
```

```powershell
# 窗口 2，启动 API
Set-Location D:\rust\Rustsky
& .\tools\start-local-api.ps1
```

```powershell
# 验证健康检查和数据库模式的登录
Set-Location D:\rust\Rustsky
$localSettings = Get-Content .preview/mysql/local-settings.json -Raw | ConvertFrom-Json
Invoke-RestMethod http://127.0.0.1:8080/health
Invoke-RestMethod http://127.0.0.1:8080/admin/employee/login `
  -Method Post -ContentType application/json `
  -Body (@{ username = 'admin'; password = $localSettings.employeePassword } | ConvertTo-Json) |
  Select-Object code,msg
```

启动脚本读取本机的 `DATABASE_URL` 和 JWT 密钥。收到登录请求后，Rust 用 `.bind(username)` 将用户名作为数据传入 SQL，查询 `employee` 表，再用 Argon2 验证密码。课程表的 `password` 列从 64 字符扩到 255 字符，用于存放带盐的哈希字符串。只有启用的员工通过校验后才会拿到 JWT。浏览器联调见 [前后端联调笔记](05-admin-integration.md)。

以下是手工复现课程数据库的步骤。新克隆的仓库可按 [第 29 篇](29-run-and-test.md) 运行初始化脚本，自动创建空库并生成管理员账号。已有的 `rustsky_dev` 无需重新导入。

## 1. 安装并确认 MySQL 在运行

在 Windows 安装 MySQL Community Server 8.4 LTS，用安装程序的 MySQL Configurator 配置本机开发实例。默认端口 `3306` 未占用时可直接使用，设置好 `root` 密码，启用 Windows 服务。Workbench 可以用来查看表和执行 SQL，MySQL Server 才是存储数据的程序。

PowerShell 找不到 `mysql` 时，可以从安装目录的 `bin` 下运行 `mysql.exe`，或者把 `bin` 加到 PATH 后重新打开终端。下面的 `-p` 会让客户端提示输入密码。

```powershell
mysql -h 127.0.0.1 -P 3306 -u root -p
```

## 2. 新建独立的开发库并导入

课程讲义用 `sky.sql` 初始化。我们手头的社区镜像文件叫 `sky_take_out.sql`，有 11 张表，包含 `DROP TABLE IF EXISTS`，且没有 `CREATE DATABASE` 或 `USE`。下面在 MySQL 提示符里选择全新的 `rustsky_dev` 库再导入。导入脚本会重建同名表，已有数据的库不要重复执行。[课程第 8 集](https://www.bilibili.com/video/BV1TP411v7v6?p=8) · [SQL 镜像](https://github.com/Danyhug/heima_sky_take_out/blob/HEAD/sky_take_out.sql)

```sql
CREATE DATABASE rustsky_dev CHARACTER SET utf8mb4;
USE rustsky_dev;
SOURCE D:/rust/Rustsky/.preview/course/sky_take_out.sql;
SHOW TABLES;
SELECT COUNT(*) AS employee_count FROM employee;
```

`SOURCE` 是 `mysql` 命令行客户端的命令。如果用 Workbench，先把 `rustsky_dev` 设为默认 schema，再打开并执行 SQL 文件。`SHOW TABLES` 应看到员工、分类、菜品、购物车和订单等共 11 张表。

## 3. 给 Rust 应用创建自己的账号

仍用管理员在 MySQL 执行，先替换示例密码。可以先给读权限验证连接。

```sql
CREATE USER 'rustsky_app'@'127.0.0.1' IDENTIFIED BY 'REPLACE_WITH_LOCAL_PASSWORD';
GRANT SELECT ON rustsky_dev.* TO 'rustsky_app'@'127.0.0.1';
```

退出 MySQL 后，在 PowerShell 验证应用账号可以连接：

```powershell
mysql -h 127.0.0.1 -P 3306 -u rustsky_app -p rustsky_dev -e 'SELECT COUNT(*) FROM employee;'
```

从课程原始数据库复现完整项目时，在选中 `rustsky_dev` 的 MySQL 客户端中，先按 [第 12 篇](12-create-employee.md) 扩展密码列、执行 `001` 并添加新增员工权限，再按编号执行项目 `db/` 目录中的 `002` 至 `012`。这些脚本包含加列和建索引，已有库只执行尚未应用的迁移。

## 4. Rust 连接时会做什么

连接信息还是主机、端口、用户名、密码和库名。SQLx 可以读类似 `mysql://rustsky_app:密码@127.0.0.1:3306/rustsky_dev` 的连接串。连接串可以放在 `DATABASE_URL` 环境变量里，密码有 URL 保留字符时需对密码部分做百分号编码。连接池在应用启动时创建一次，再由多个 HTTP 请求共享。后续登录流程会从员工表查询账号，校验密码，再签发 JWT。

设置 `DATABASE_URL` 后，Rustsky 会在启动时连接数据库，数据库不可达时会报错。未设置时，仍可按上一节笔记运行无数据库的演示模式。

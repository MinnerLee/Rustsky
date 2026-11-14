# Rustsky 学习笔记 07 · 从一次登录认识 Nginx 和 MySQL

在 Rustsky 里，Nginx 是浏览器访问的入口，Rust 判断能不能登录，MySQL 保存员工数据。三个程序分别运行，Rust 通过 HTTP 接收前端请求，通过 MySQL 协议查询数据库。

```text
浏览器 → Nginx :8888 → Rust :8080 或 :8081 → MySQL :3306
         返回页面      校验密码、签发令牌     查 employee 表
```

`127.0.0.1` 代表你正在使用的这台电脑。端口就像这台电脑上不同程序的门牌号。`8888` 是浏览器进来的入口，`8080` 和 `8081` 是两个运行同一份代码的 Rust 进程，`3306` 是数据库程序的入口。

## Nginx 怎样知道该把请求交给谁

打开 `http://127.0.0.1:8888/sky/` 时，Nginx 按 `tools/nginx.conf` 里的 `location /sky/` 找文件，把 HTML、JS、CSS 和铜锅照片发给浏览器。浏览器加载 JS，画出登录表单。

点登录时，JS 向同一个地址发 `POST /api/employee/login`。Nginx 匹配另一个规则：

```nginx
location /api/ {
    proxy_pass http://rustsky_api/admin/;
}
```

这里 `location` 按路径分类，`proxy_pass` 决定转到哪里。因为目标带有 `/admin/`，Nginx 把原来的 `/api/` 前缀替换成 `/admin/`。`rustsky_api` 是配置中定义的后端组，里面有 `127.0.0.1:8080` 和 `127.0.0.1:8081`。浏览器只访问 `8888`，Nginx 代替它与后端通信，这叫**反向代理**。Nginx 默认轮流选一个实例处理请求，这叫**负载均衡**。一笔请求只由选中的实例处理，另一个实例可以接其他请求。

## MySQL 怎样找到员工

把 MySQL 想成一个管理多本账册的程序。`rustsky_dev` 是一套账册，叫**数据库**。`employee` 是其中一张表，类似 Excel 里的工作表。每一行是一个员工，每一列记录一类信息，比如 `id`、`username` 和 `status`。`id` 唯一标识一行，是主键。

Rust 收到请求后，用 SQLx 从连接池取一条到 MySQL 的连接。连接池让请求可以复用已有连接。Rust 会执行类似下面的查询：

```sql
SELECT id, username, name, status
FROM employee
WHERE username = 'admin';
```

`SELECT` 指定要取哪些列，`FROM` 指定表，`WHERE` 指定哪一行。上面是便于阅读的示例。代码用 `WHERE username = ?` 和 `.bind(username)` 传入浏览器提交的用户名。真正的登录查询还会取密码哈希。MySQL 返回员工记录后，Rust 检查账号是否启用，用 Argon2 校验提交的密码，然后签发 JWT 给前端。登录规则在 Rust 代码里，员工记录在 MySQL 里。

Java 课程可以复用同一个 MySQL 库。Java 用 JDBC 和 MyBatis 发送 SQL，Rust 用 SQLx。建库、建表、`SELECT` 的语法都由 MySQL 决定。

## 亲手观察一次

1. 打开 [Rustsky 管理端](http://127.0.0.1:8888/sky/)，按 `F12`，打开 **Network** 面板。用 `admin` 和本机 `.preview/mysql/local-settings.json` 里的 `employeePassword` 登录。在 `employee/login` 的响应头里找 `X-Rustsky-Upstream`。这个值表示实际处理请求的 Rust 端口。
2. 在 Rustsky 目录打开 PowerShell，执行下面的只读查询。`--defaults-extra-file` 让 MySQL 客户端从本机配置读取连接信息。查询只输出员工编号、用户名和状态。

   ```powershell
   Set-Location D:\rust\Rustsky
   & .\.preview\mysql\mysql-8.4.13-winx64\bin\mysql.exe `
     --defaults-extra-file=D:/rust/Rustsky/.preview/mysql/root-client.ini `
     --database=rustsky_dev `
     --execute="SELECT id, username, status FROM employee WHERE username = 'admin';"
   ```

3. 对照结果。MySQL 查到的 `id` 会出现在成功登录的 `data.id` 中。`status = 1` 表示账号启用。此时可以把浏览器请求、Nginx 转发、Rust 校验和数据库记录串在一起。

## 登录出问题时先看哪一层

| 你看到的现象 | 先查什么 |
| --- | --- |
| `8888/sky/` 打不开 | Nginx 是否运行，是否使用了正确的端口 |
| 页面能打开，登录请求返回 `502` | Nginx 是否能连上 Rust，试试 `8080/health` 和 `8081/health` |
| 登录响应是 HTTP `200`，JSON 里的 `code` 是 `0` | Rust 已收到请求，检查账号、密码和账号状态 |
| Rust 登录接口返回 `503` | 检查 MySQL 是否运行，以及 Rust API 的数据库连接和日志 |

本机服务的启动命令见 [学习笔记 06](06-reverse-proxy-load-balancing.md)，MySQL 的建库与导入过程见 [学习笔记 04](04-mysql-setup.md)。

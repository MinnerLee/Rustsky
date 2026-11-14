# Rustsky 学习笔记 29 · 从空仓库环境跑通和验收

从 GitHub 克隆后，用 PowerShell 7 进入仓库根目录。需要 Rust、支持 `--openssl-legacy-provider` 的 Node.js（已用于 Node 24）、MySQL 8.4.13 ZIP、Nginx 1.31.8 Windows ZIP，以及 Redis 的本地服务或 Windows 便携构建。

| 文件 | 解压后的位置 |
| --- | --- |
| `mysqld.exe` | `.preview/mysql/mysql-8.4.13-winx64/bin/mysqld.exe` |
| `nginx.exe` | `.preview/nginx/nginx-1.31.8/nginx.exe` |
| `redis-server.exe` 与 `redis-cli.exe` | `.preview/redis/bin/` 或子目录，也可以设置 `RUSTSKY_REDIS_URL` 连接现有 Redis |

下载入口见 [MySQL](https://dev.mysql.com/downloads/mysql/) · [Nginx](https://nginx.org/en/download.html) · [Redis Windows 预览说明](18-redis-shop-menu.md)。

```powershell
# 首次使用，创建空数据库并生成本机账号
& .\tools\init-local-db.ps1

# 安装前端依赖并构建，修改 Vue 后也可再运行
& .\tools\build-admin.ps1

# 保持这个终端运行
& .\tools\start-local-stack.ps1
```

打开 [管理端](http://127.0.0.1:8888/sky/) 后使用 `admin`，初始密码在 `.preview/mysql/local-settings.json` 的 `employeePassword` 字段。MySQL 保存的数据和开发密钥也在这个本机目录。已有环境可以直接构建和启动。

想改 Vue 时，在另一个 PowerShell 终端进入 `frontend`，设置 `$env:NODE_OPTIONS='--openssl-legacy-provider'` 后执行 `npm run serve`，再访问 [Vue 开发入口](http://127.0.0.1:8082/)。Nginx 仍运行在 8888。

## 按功能验收

```powershell
cargo test --locked
& .\tools\test-create-employee.ps1
& .\tools\test-employee-page.ps1
& .\tools\test-employee-category.ps1
& .\tools\test-customer.ps1
& .\tools\test-dish.ps1 -IncludeSetmeal -IncludeCache -IncludeAutomation -IncludeReports
```

脚本通过真实 MySQL、Redis 与 Nginx 测试带权限的接口，订单超时测试可能需要等待约一分钟。微信支付签名与本地回调测试见 [第 25 篇](25-wechat-payment.md)。

可以再用本机 Chrome 验证页面操作。脚本用随机后缀创建和清理临时员工、菜品、分类与套餐：

```powershell
npm.cmd install --prefix .preview/browser playwright-core
node tools/test-admin-browser.cjs
```

Chrome 安装在非默认路径时用 `RUSTSKY_CHROME` 指向本机的 `chrome.exe`。浏览器测试覆盖员工列表、新增、修改和启停，以及套餐选菜、图片上传、份数回填、上架下架和删除。

前端单元测试可在 `frontend` 目录运行 `npm run test:unit -- --runInBand`，验证面包屑路由和输入校验。使用 Node 24 时沿用开发服务器的 `NODE_OPTIONS` 设置。

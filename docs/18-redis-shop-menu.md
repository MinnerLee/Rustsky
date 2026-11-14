# Rustsky 学习笔记 18 · Redis 营业状态和用户菜单

店铺开关只有两个值，`0` 表示打烊，`1` 表示营业。管理端设置这个开关，用户端读取它。在 Rustsky 中两个 Rust 实例共用 Redis 的 `rustsky:shop:status`，所以 Nginx 把写请求交给一个实例后，另一个实例也能读到相同结果。[Redis 文档](https://redis.io/docs/latest/commands/get/) · [课程店铺状态小节](https://www.bilibili.com/video/BV1TP411v7v6?p=63)

| 操作 | 接口 | Redis 操作 |
| --- | --- | --- |
| 管理员切换营业状态 | `PUT /api/shop/{status}` | `SET rustsky:shop:status 0` 或 `1` |
| 管理端读取营业状态 | `GET /api/shop/status` | `GET rustsky:shop:status` |
| 用户端读取营业状态 | `GET /user/shop/status` | 读取同一个键，尚未设置时返回 `0` |

Redis 用键找到值，MySQL 则用表和查询条件找到记录。这里用 Redis 保存两个实例共用的简单开关。菜单的分类、价格、口味和套餐份数仍由 MySQL 提供，因为这些数据之间有表关系。

## 本地操作

已准备好的本机环境通过 `tools/start-local-stack.ps1` 启动 Redis、MySQL、两个 Rust 实例和 Nginx。Redis 使用经过发布方 SHA-256 校验的 Windows 社区构建，只监听 `127.0.0.1:6379`，并将 AOF 持久化文件写到 `.preview/redis/data/`。AOF 记录对数据的写入，供 Redis 重启时恢复。也可以提前设置 `RUSTSKY_REDIS_URL` 连接已有的 Redis。[Windows 构建发布页](https://github.com/redis-windows/redis-windows/releases/tag/8.10.3)

在 Swagger 登录并授权后，执行 `PUT /api/shop/1`。再访问 [用户端营业状态](http://127.0.0.1:8888/user/shop/status)，可以看到 `data: 1`。切回打烊后 `data` 变回 `0`。如果想先熟悉 Redis 命令，可以用独立的练习键：

```powershell
$cli = 'D:\rust\Rustsky\.preview\redis\bin\Redis-8.10.3-Windows-x64-msys2\redis-cli.exe'
& $cli PING
& $cli SET rustsky:lesson:example hello EX 60
& $cli GET rustsky:lesson:example
```

`EX 60` 让练习键在 60 秒后自动清理。

## 用户菜单查询

管理端新增并启用分类、起售菜品或套餐后，可以从 Swagger 的「菜单」分组调用用户端接口：

- `GET /user/category/list` 取得启用的分类。
- `GET /user/dish/list?categoryId=…` 取得该分类下的在售菜品和口味。
- `GET /user/setmeal/list?categoryId=…` 取得在售套餐，`GET /user/setmeal/dish/{id}` 查询套餐中的菜品和份数。

用户端 API 只返回启用分类中的在售商品。验证脚本会新建临时商品，检查它起售前后在用户菜单中的可见性，最后清理临时记录。菜品和套餐列表接入缓存的过程见 [第 21 篇](21-menu-cache-order-management.md)。

```powershell
& D:\rust\Rustsky\tools\test-dish.ps1 -IncludeSetmeal -IncludeMenu
```

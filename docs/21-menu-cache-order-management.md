# Rustsky 学习笔记 21 · Redis 菜单缓存和订单管理

## 缓存和营业开关为什么用法不同

第 18 篇的营业开关直接写入 Redis。菜品和套餐的原始记录保存在 MySQL，Redis 缓存保存的是某个分类列表查询的结果。用户请求列表时，Rust 先尝试用 Redis 读 JSON，缺失时查询 MySQL 并把结果缓存 60 秒。Redis 暂不可用时走 MySQL。

缓存键形如 `rustsky:menu:17:dish:3`，表示版本 17、菜品、分类 3。管理端改动分类、菜品或套餐后，调用 `INCR rustsky:menu:version`。下一次读取用新版本的键，旧键由 Redis 按 TTL 自动清理。两个 Rust 实例共用版本号。即使旧查询比写操作更晚结束，它也只会填充旧版本。

如果一次改动提交时 Redis 恰好不可用，已经存在的缓存可能继续使用到它的 TTL 到期。购物车和下单的售卖检查仍直接查 MySQL。

## 一笔订单可以怎样变化

| 订单状态 | 下一步 | 对应接口 |
| --- | --- | --- |
| 1 待付款 | 本地模拟付款 | `PUT /user/order/dev/payment` |
| 2 待接单 | 接单或拒单 | `PUT /api/order/confirm`、`PUT /api/order/rejection` |
| 3 待派送 | 开始派送 | `PUT /api/order/delivery/{id}` |
| 4 派送中 | 完成订单 | `PUT /api/order/complete/{id}` |
| 5 已完成 | 查询订单和明细 | `GET /api/order/details/{id}` |
| 6 已取消 | 查询取消或拒单原因 | `GET /api/order/details/{id}` |

例如接单的 SQL 带有 `WHERE id=? AND status=2 AND pay_status=1`。两个人同时接单，只有第一个请求能把状态从 2 改到 3。第二个请求执行时条件已经不成立，所以更新行数为 0。这也能阻止未付款订单直接进入派送。

订单状态和支付状态是两个字段。模拟付款只对本地开发账号开放，并给记录标记 `payment_simulated=1`。拒单可以把这样的模拟支付标记为模拟退款。[第 25 篇](25-wechat-payment.md) 讲解真实支付的商户配置、回调验签和退款确认。

## 复现与练习

运行脚本会对真实 MySQL 和 Redis 验证缓存过期时间、停售后的缓存失效、顾客订单查询、模拟付款、接单、派送、完成和拒单。测试会恢复营业开关并删除自己创建的订单。

```powershell
& D:\rust\Rustsky\tools\test-dish.ps1 -IncludeSetmeal -IncludeCache -IncludeOrderManagement
```

可以在 [Swagger](http://127.0.0.1:8888/docs/) 观察同一条订单。先用顾客授权查自己的明细，再用员工授权查管理端分页。试着在已完成时再次调用完成接口，看看 SQL 更新条件为何匹配不到记录。

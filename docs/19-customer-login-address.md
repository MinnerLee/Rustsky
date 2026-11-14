# Rustsky 学习笔记 19 · 小程序登录和地址簿

课程的微信登录和地址簿分属 Day06 和 Day08。Rust 版实现了相同的登录入口和地址操作，通过 Nginx 的 `/user/` 访问。可在 [Swagger](http://127.0.0.1:8888/docs/) 里看到请求和响应字段。[课程分集](https://www.bilibili.com/video/BV1TP411v7v6/)

## 一次登录里的两个凭证

1. 小程序调用 `wx.login` 拿到短期 `code`，发送给 `POST /user/user/login`。
2. Rust 用 App ID、App Secret 和 `code` 调用微信的 `code2Session`。微信返回的 `openid` 在这个小程序里标识同一位用户。后端只需要用它找到或创建自己的 `user` 记录。
3. Rust 签发有效期一小时的 JWT。访问地址簿时，小程序把 JWT 放进 `authentication` 请求头。管理员用的是另一个签名密钥和 `token` 请求头，员工账号与小程序用户各自有一套身份。

第一次登录时数据库需要插入用户，以后登录只查同一条记录。给 `openid` 加唯一索引后，两次同时登录最多插入一条。另一条请求发现唯一约束冲突后，重新查询已存在的用户 ID。

正式联调在启动 Rust 服务的环境中设置 `RUSTSKY_WX_APP_ID` 和 `RUSTSKY_WX_APP_SECRET`，并设置独立的 `RUSTSKY_USER_JWT_SECRET`。当前本地验证用启动脚本生成的开发凭证，保存在 Git 忽略的 `.preview/mysql/local-settings.json`。配置微信参数后使用微信登录流程，开发凭证随之停用。

## 地址簿里的身份和事务

以 `GET /user/addressBook/12` 为例，`12` 是地址 ID。服务器还要从 JWT 拿到当前用户 ID，SQL 用两个条件查找这条地址：

```sql
SELECT id, consignee, detail
FROM address_book
WHERE id = ? AND user_id = ?;
```

`user_id` 的值由后端认证产生。这样即使用户猜到别人的地址 ID，查、改、删也只能操作自己的地址。

`PUT /user/addressBook/default` 在一个事务里清除旧默认并设置新默认。假如新地址不存在或属于其他用户，事务会回滚，之前的默认地址也会恢复。这个练习能看到 MySQL 事务具体解决的问题。

## 本地练习

运行下方脚本会验证错误凭证被拒、重复登录得到同一个 ID、匿名请求返回 HTTP 401、收货地址增改删，以及设置不存在的默认地址后旧值仍在。脚本结束时恢复旧默认并删除测试地址。

```powershell
& D:\rust\Rustsky\tools\test-customer.ps1
```

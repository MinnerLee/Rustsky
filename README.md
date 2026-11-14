# Rustsky

Rust 版《苍穹外卖》。管理端沿用课程风格的 Vue 2 和 TypeScript，后端用 Axum、SQLx、MySQL 和 Redis，Nginx 提供静态文件并转发接口。

- [学习笔记目录](docs/README.md) 覆盖后端主线和第 171–199 集前端课程。
- [Windows 本地启动](docs/29-run-and-test.md) 包含从空数据库初始化、构建和验收命令。
- [OpenAPI](docs/openapi.json) 包含员工、商品、用户端、订单、支付及运营统计接口。

本机启动后可打开 [管理端](http://127.0.0.1:8888/sky/) 和 [Swagger](http://127.0.0.1:8888/docs/)。本地支持开发账号和模拟付款。真实微信登录、收款和退款需要自己的小程序与商户配置，见 [微信支付笔记](docs/25-wechat-payment.md)。

管理端基于 [HeFeng-Lab/project-sky-admin-vue-ts](https://github.com/HeFeng-Lab/project-sky-admin-vue-ts)，保留上游 [MIT 许可证](frontend/LICENSE)。数据库结构参考 [课程数据库镜像](https://github.com/Danyhug/heima_sky_take_out/blob/HEAD/sky_take_out.sql)。顾客端本地预览的课程资源来源和生成方式见 [小程序笔记](docs/24-miniprogram-local-preview.md)。

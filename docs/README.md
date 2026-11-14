# Rustsky 学习笔记

从项目里已经能运行的功能入手。每篇笔记都对应实际接口或代码，并给出可复现的操作。

| 阶段 | 笔记 |
| --- | --- |
| 认识项目 | [01 项目概览](01-project-overview.md) · [02 管理端前端](02-admin-frontend.md) · [03 Rust 后端骨架](03-backend-bootstrap.md) |
| 本地环境与联调 | [04 MySQL 搭建](04-mysql-setup.md) · [05 前后端联调](05-admin-integration.md) · [06 Nginx 反向代理](06-reverse-proxy-load-balancing.md) · [07 Nginx 和 MySQL 入门](07-nginx-mysql-for-beginners.md) |
| 登录和接口文档 | [08 员工登录](08-employee-login.md) · [09 接口约定](09-api-contract.md) · [10 Swagger](10-swagger-ui-and-annotations.md) · [11 独立开发路线](11-independent-development-roadmap.md) |
| 员工和商品管理 | [12 新增员工](12-create-employee.md) · [13 新增员工实现和测试](13-create-employee-implementation-and-testing.md) · [14 员工分页](14-employee-pagination.md) · [15 员工与分类](15-employee-status-edit-category.md) · [16 菜品](16-dish-management.md) · [17 套餐](17-setmeal-management.md) |
| 用户端和订单 | [18 Redis 营业状态和菜单](18-redis-shop-menu.md) · [19 用户登录和地址簿](19-customer-login-address.md) · [20 购物车和下单](20-cart-order-checkout.md) · [21 菜单缓存和订单管理](21-menu-cache-order-management.md) |
| 通知与运营 | [22 订单定时任务与通知](22-order-automation-notifications.md) · [23 统计、工作台和 Excel](23-reports-workspace-excel.md) |
| 小程序联调 | [24 本地小程序、模拟付款和再来一单](24-miniprogram-local-preview.md) |
| 微信支付 | [25 预下单、验签、查单和退款](25-wechat-payment.md) |
| 前端课程第 171–199 集 | [26 Vue、路由、Vuex 和 TypeScript](26-vue-router-vuex-typescript.md) · [27 员工页面](27-employee-frontend.md) · [28 套餐页面](28-setmeal-frontend.md) |
| 独立运行和验收 | [29 从空库搭建并验证前后端](29-run-and-test.md) |

本机服务启动后可打开 [管理端](http://127.0.0.1:8888/sky/) 和 [Swagger](http://127.0.0.1:8888/docs/)。离线也可阅读 [OpenAPI JSON](openapi.json)。

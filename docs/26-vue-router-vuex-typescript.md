# Rustsky 学习笔记 26 · Vue、路由、Vuex 和 TypeScript

对应课程前端 Day1，第 171–180 集。练习使用已接通 Rust 的管理端。[课程第 171 集](https://www.bilibili.com/video/BV1TP411v7v6?p=171)

| 分集 | 学什么 | 在 Rustsky 里观察什么 |
| --- | --- | --- |
| 171–172 | 课程入口与 Vue CLI 脚手架 | `frontend/package.json` 声明依赖，`src/main.ts` 注册根组件、路由和 Vuex |
| 173–174 | 模板、状态、事件以及组件通信 | 员工搜索用 `v-model` 同步输入，套餐弹窗通过 prop 和事件传递菜品 |
| 175–176 | Vue Router 和嵌套路由 | 登录后进入管理布局，员工列表与编辑表单共享菜单和顶栏 |
| 177–178 | Vuex 管理共享状态 | 员工登录后将 JWT 写入 `UserModule`，多个页面请求时由 Axios 读取 |
| 179–180 | TypeScript 的类型、接口和泛型 | `MealDish` 描述行数据，`MealPayload` 描述实际提交的数据 |

## 在浏览器先看这一条链路

打开 [本地管理端](http://127.0.0.1:8888/sky/)，登录并进入员工管理，按 F12 查看 Network。地址中的 `#/employee` 由 Vue Router 解释，页面会发送 `GET /api/employee/page`。Nginx 把这个 HTTP 请求转给 Rust 的 `/admin/employee/page`。这样你能分清页面路由和 API 路由各自的工作。

1. 在搜索框敲几个字，观察 `v-model` 怎样把输入赋给组件。点击查询后，组件把页码重置为 1，再取当前筛选结果。
2. 打开 [员工编辑页](http://127.0.0.1:8888/sky/#/employee)，从列表点「修改」，观察 `id` 怎样通过 URL 查询参数传到表单。
3. 在 `frontend/src/store/modules/user.ts` 找登录、保存令牌和登出。再去 `frontend/src/utils/request.ts` 看请求拦截器如何写入 `token` 请求头。在 Network 观察字段时可以只检查请求头名称。

## TypeScript 怎么帮你读代码

```ts
interface DishQuantity {
  dishId: number
  copies: number
}
const items: DishQuantity[] = [{ dishId: 7, copies: 3 }]
```

像 C++ 的结构体约束字段一样，`DishQuantity` 告诉编辑器这两个字段都应是数字。JSON 仍需要 Rust 在运行时验证。套餐页面可以显示菜名和原价，提交时只带 ID 和份数，后端从数据库确认商品资料。

开发时可运行 `npm run serve`（Node 24 加 `NODE_OPTIONS=--openssl-legacy-provider`），Vue 开在 `127.0.0.1:8082`，把 API、图片和订单 WebSocket 转给本地 Nginx `8888`。构建和启动完整项目见[仓库说明](../README.md)。

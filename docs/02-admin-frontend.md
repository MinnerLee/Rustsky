# Rustsky 学习笔记 02 · 管理端前端

课程第 5 集用 Nginx 运行已经打包好的管理端。要修改表单、菜单或页面样式，可以直接在 Vue 工程里改组件，再重新构建。

本地 `frontend/` 是从社区镜像 HeFeng-Lab/project-sky-admin-vue-ts 拉取的独立 Git 仓库，基线提交是 `37d34c2fa5fb87b218672bb0f2f6a18861408f32`。源码和原有 `LICENSE` 均已保留。Rustsky 的本地 Git 排除了这个目录，查看前端改动可以用 `git -C frontend status`。

| 找什么 | 看哪里 | 为什么 |
| --- | --- | --- |
| 登录页的布局、表单和样式 | `frontend/src/views/login/index.vue` | Vue 单文件组件把模板、交互逻辑和样式放在一起 |
| 员工登录发什么请求 | `frontend/src/api/employee.ts` | 这里定义登录请求的路径和方法 |
| 前端怎样携带令牌 | `frontend/src/utils/request.ts` | Axios 封装会给已登录请求加 `token` 请求头 |
| 前端请求转发到哪里 | `frontend/vue.config.js`、`frontend/.env.development` | 开发服务器用代理把浏览器请求转给后端 |

以登录为例，开发环境中浏览器向 `localhost:8888/api/employee/login` 发出 POST。Vue 开发服务器去掉 `/api`，将请求转给 `localhost:8080/admin/employee/login`。因此 Rust 后端需要实现的是 `/admin/employee/login`。接口联调时可以在浏览器的 Network 面板对照请求路径、请求体和响应。

这次核对了源码、依赖清单和代理配置。工程使用 Vue 2、Vue CLI 3、Webpack 4 和 `fibers` 4。当前机器的 Node 是 v24，这套旧依赖还没有在本机安装或验证，后续启动前需要先处理版本兼容。

参考：[课程第 5 集](https://www.bilibili.com/video/BV1TP411v7v6?p=5) · [管理端源码镜像](https://github.com/HeFeng-Lab/project-sky-admin-vue-ts/tree/37d34c2fa5fb87b218672bb0f2f6a18861408f32)

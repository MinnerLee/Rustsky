# Rustsky 学习笔记 06 · 反向代理与负载均衡

第 10 集讲反向代理和负载均衡的概念，第 11 集（Day01-10）讲 Nginx 配置。Rustsky 对应的实际配置在 `tools/nginx.conf`。[课程第 10 集](https://www.bilibili.com/video/BV1TP411v7v6?p=10) · [课程第 11 集](https://www.bilibili.com/video/BV1TP411v7v6?p=11)

## 反向代理负责转发

浏览器提交登录表单时，请求先到 `127.0.0.1:8888` 的预览服务。这个服务既能返回登录页，也能将 `/api/` 下的请求转给 Rust。对浏览器来说，页面和 API 都来自同一个地址。

```text
浏览器  POST /api/employee/login
   ↓
预览代理  127.0.0.1:8888
   ↓     POST /admin/employee/login
Rust API 127.0.0.1:8080
   ↓
MySQL    127.0.0.1:3306
```

Rustsky 最初由 `tools/preview.py` 完成转发，现在用 Nginx。先看只有一个 API 实例时的配置。`location` 匹配 `/api/` 前缀，而带 URI 的 `proxy_pass` 把匹配到的前缀替换成 `/admin/`。两个尾部斜杠与这个路径映射有关。[Nginx `proxy_pass` 文档](https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_pass)

```nginx
location /api/ {
    proxy_pass http://127.0.0.1:8080/admin/;
}
```

## 负载均衡负责选后端

如果同一套 Rust API 运行了两个实例，例如 `8080` 和 `8081`，代理可以把不同请求分给两个实例。Nginx 配置中的 `upstream` 声明后端组，默认按轮询分配请求。每个登录请求仍由其中一个实例处理并返回结果。[Nginx HTTP 负载均衡文档](https://nginx.org/en/docs/http/load_balancing.html)

```nginx
# 写在 http 块中
upstream rustsky_api {
    server 127.0.0.1:8080;
    server 127.0.0.1:8081;
}

# 写在 server 块中
location /api/ {
    proxy_pass http://rustsky_api/admin/;
}
```

多实例部署时，后端需要使用同一个数据库配置。以后接入 JWT 验证中间件时，各实例还要用相同的签名密钥，否则在一个实例签发的令牌可能被另一个实例拒绝。应用实例增加后，数据库连接池的连接数也会叠加。

课程配套配置中有 `weight` 示例，权重越大，分到的请求比例越高。Rustsky 的两个实例使用默认相同权重，方便观察交替转发。当前代理用于管理端 `/api/`。配套配置里的 `/user/` 和 `/ws/` 分别对应后续的小程序和 WebSocket 功能，等相应 Rust 接口实现后再接入。

## 怎么在当前项目里观察

在 [本地管理端](http://127.0.0.1:8888/sky/) 打开浏览器开发者工具，登录时观察 **Network** 里的 `employee/login`。浏览器显示的是 `/api/employee/login`，而 Rust 实际注册的是 `/admin/employee/login`。响应头 `X-Rustsky-Upstream` 显示实际处理请求的实例。实测四次登录依次到达 `8080 → 8081 → 8080 → 8081`，每次都读到了同一个 MySQL 员工账号并返回 JWT。

## 重启本地服务

已完成本机环境配置后，在 PowerShell 7.4 或更新版本中用一条命令启动 MySQL、两个 Rust 实例和 Nginx：

```powershell
Set-Location D:\rust\Rustsky
& .\tools\start-local-stack.ps1
```

看到 `Rustsky is ready` 后就可以打开预览。保留这个窗口运行，按 Ctrl+C 停止。启动时如果提示端口已占用，先确认这些服务是否已经在运行。

### 分开启动以便排查

先按 [MySQL 环境笔记](04-mysql-setup.md) 启动 MySQL。然后在两个 PowerShell 窗口中分别启动 Rust API，每个窗口都从 `D:\rust\Rustsky` 运行：

```powershell
$env:RUSTSKY_PORT = '8080' # 另一个窗口改为 8081
& .\tools\start-local-api.ps1
```

在第三个窗口验证并启动 Nginx。便携版位于 `.preview/nginx/`，项目配置位于 `tools/nginx.conf`。

```powershell
$nginx = '.\.preview\nginx\nginx-1.31.8\nginx.exe'
$prefix = (Get-Location).Path.Replace('\', '/') + '/'
& $nginx -p $prefix -c tools/nginx.conf -e stderr -t
if ($LASTEXITCODE -eq 0) { & $nginx -p $prefix -c tools/nginx.conf -e stderr }
```

若代理返回 `502`，分别检查 `8080/health` 和 `8081/health`。Python 预览与 Nginx 共用 `8888`，同一时间只需启动其中一个。[Nginx Windows 运行文档](https://nginx.org/en/docs/windows.html)

mod address;
mod api_docs;
mod cart;
mod category;
mod common;
mod customer;
mod dish;
mod employee;
mod menu;
mod menu_cache;
mod order;
mod report;
mod setmeal;
mod shop;
mod upload;

use axum::{Json, Router, middleware, routing::get};
use common::ApiResult;
use std::{env, error::Error, sync::Arc};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    match env::args().nth(1).as_deref() {
        Some("--hash-dev-password") => {
            println!("{}", employee::hash_dev_password()?);
            return Ok(());
        }
        Some("--export-openapi") => {
            println!("{}", api_docs::export()?);
            return Ok(());
        }
        _ => {}
    }
    let employee = employee::LoginService::from_env().await?.map(Arc::new);
    let customers = customer::from_env(&employee)?;
    let wechat = order::wechat::Client::from_env()?.map(Arc::new);
    if wechat.is_some() && customers.is_none() {
        return Err(std::io::Error::other("WeChat Pay requires configured customer login").into());
    }
    let redis = shop::connection().await?;
    order::automation::start(&employee, wechat.clone());
    let port = env::var("RUSTSKY_PORT").unwrap_or_else(|_| "8080".into());
    let listener = TcpListener::bind(("127.0.0.1", port.parse::<u16>()?)).await?;
    let catalog_admin = Router::new()
        .nest("/admin/category", category::routes(employee.clone()))
        .nest("/admin/dish", dish::routes(employee.clone()))
        .nest("/admin/setmeal", setmeal::routes(employee.clone()))
        .layer(middleware::from_fn_with_state(
            redis.clone(),
            menu_cache::invalidate_after_write,
        ));
    let app = Router::new()
        .route("/health", get(health))
        .nest("/admin/employee", employee::routes(employee.clone()))
        .merge(catalog_admin)
        .nest("/admin/common", upload::routes(employee.clone()))
        .nest("/admin/order", order::admin_routes(employee.clone()))
        .nest("/admin/report", report::routes(employee.clone()))
        .nest(
            "/notify",
            order::wechat::routes(customers.clone(), wechat.clone()),
        )
        .nest(
            "/admin/workspace",
            report::workspace_routes(employee.clone()),
        )
        .nest(
            "/admin/shop",
            shop::admin_routes(employee.clone(), redis.clone()),
        )
        .nest(
            "/user",
            menu::routes(employee, redis.clone())
                .nest("/shop", shop::user_routes(redis.clone()))
                .nest("/user", customer::routes(customers.clone()))
                .nest("/shoppingCart", cart::routes(customers.clone()))
                .nest("/order", order::routes(customers.clone(), redis, wechat))
                .nest("/addressBook", address::routes(customers)),
        )
        .merge(api_docs::routes());

    println!("Rustsky listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "运行状态",
    operation_id = "healthCheck",
    summary = "检查 Rust 服务是否响应",
    security(()),
    responses((status = 200, description = "进程可以响应 HTTP 请求。", body = ApiResult<String>, example = json!({"code": 1, "msg": null, "data": "ok"})))
)]
async fn health() -> Json<ApiResult<&'static str>> {
    Json(ApiResult::success("ok"))
}

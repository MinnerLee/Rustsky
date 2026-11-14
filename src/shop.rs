use crate::{
    common::{ApiResult, NoData},
    employee::{
        EmployeeIdentity, EmployeeState, authenticate_employee,
        manage::{admin_database, denied},
    },
};
use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    middleware,
    routing::{get, put},
};
use redis::{AsyncCommands, aio::ConnectionManager};

const STATUS_KEY: &str = "rustsky:shop:status";
type Reply<T> = (StatusCode, Json<ApiResult<T>>);

#[derive(Clone)]
pub(crate) struct ShopState {
    employees: EmployeeState,
    redis: Option<ConnectionManager>,
}

pub async fn connection() -> Result<Option<ConnectionManager>, Box<dyn std::error::Error>> {
    let Some(url) = std::env::var_os("RUSTSKY_REDIS_URL") else {
        return Ok(None);
    };
    let url = url
        .into_string()
        .map_err(|_| std::io::Error::other("Invalid Redis URL"))?;
    let client = redis::Client::open(url)?;
    Ok(Some(ConnectionManager::new(client).await?))
}

pub fn admin_routes(employees: EmployeeState, redis: Option<ConnectionManager>) -> Router {
    Router::new()
        .route("/status", get(admin_status))
        .route("/{status}", put(set_status))
        .route_layer(middleware::from_fn_with_state(
            employees.clone(),
            authenticate_employee,
        ))
        .with_state(ShopState { employees, redis })
}

pub fn user_routes(redis: Option<ConnectionManager>) -> Router {
    Router::new()
        .route("/status", get(user_status))
        .with_state(redis)
}

#[utoipa::path(get, path="/api/shop/status", tag="店铺", summary="查询店铺营业状态", security(("EmployeeToken" = [])),
    responses((status=200, description="0 打烊，1 营业。", body=ApiResult<i32>)))]
pub(crate) async fn admin_status(State(state): State<ShopState>) -> Reply<i32> {
    status(state.redis).await
}

#[utoipa::path(get, path="/user/shop/status", tag="店铺", summary="用户端查看营业状态", security(()),
    responses((status=200, description="0 打烊，1 营业。", body=ApiResult<i32>)))]
pub(crate) async fn user_status(State(redis): State<Option<ConnectionManager>>) -> Reply<i32> {
    status(redis).await
}

pub(crate) async fn is_open(redis: Option<ConnectionManager>) -> Result<bool, ()> {
    read_status(redis).await.map(|value| value == 1)
}

async fn status(redis: Option<ConnectionManager>) -> Reply<i32> {
    match read_status(redis).await {
        Ok(value) => (StatusCode::OK, Json(ApiResult::success(value))),
        Err(()) => unavailable(),
    }
}

async fn read_status(redis: Option<ConnectionManager>) -> Result<i32, ()> {
    let Some(mut redis) = redis else {
        return Err(());
    };
    match redis.get::<_, Option<i32>>(STATUS_KEY).await {
        Ok(value) => Ok(value.unwrap_or(0)),
        Err(error) => {
            eprintln!("Shop status read failed: {error}");
            Err(())
        }
    }
}

#[utoipa::path(put, path="/api/shop/{status}", tag="店铺", summary="设置店铺营业状态", security(("EmployeeToken" = [])),
    params(("status"=i32, Path, description="0 打烊，1 营业")), responses((status=200, description="状态保存结果。", body=ApiResult<NoData>)))]
pub(crate) async fn set_status(
    State(state): State<ShopState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(status): Path<i32>,
) -> Reply<NoData> {
    if !matches!(status, 0 | 1) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResult::failure("营业状态须为 0 或 1")),
        );
    }
    if let Err(code) = admin_database(&state.employees, actor).await {
        return denied(code);
    }
    let Some(mut redis) = state.redis else {
        return unavailable();
    };
    match redis.set::<_, _, ()>(STATUS_KEY, status).await {
        Ok(()) => (StatusCode::OK, Json(ApiResult::success(NoData))),
        Err(error) => {
            eprintln!("Shop status write failed: {error}");
            unavailable()
        }
    }
}

fn unavailable<T>() -> Reply<T> {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiResult::failure("Redis 暂不可用")),
    )
}

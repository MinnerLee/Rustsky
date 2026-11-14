use crate::{
    common::ApiResult,
    employee::{EmployeeState, LoginService},
    menu_cache,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::collections::BTreeMap;
use utoipa::ToSchema;

type Reply<T> = (StatusCode, Json<ApiResult<T>>);

#[derive(Clone)]
pub(crate) struct MenuState {
    employee: EmployeeState,
    redis: Option<ConnectionManager>,
}

pub fn routes(employee: EmployeeState, redis: Option<ConnectionManager>) -> Router {
    Router::new()
        .route("/category/list", get(categories))
        .route("/dish/list", get(dishes))
        .route("/setmeal/list", get(setmeals))
        .route("/setmeal/dish/{id}", get(meal_dishes))
        .with_state(MenuState { employee, redis })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Filter {
    category_id: Option<i64>,
    r#type: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MenuCategory {
    id: i64,
    name: String,
    r#type: i32,
    sort: i32,
}

#[utoipa::path(get, path="/user/category/list", tag="菜单", summary="查看启用的菜单分类", security(()),
    params(("type"=Option<i32>, Query)), responses((status=200, description="菜单分类。", body=ApiResult<Vec<MenuCategory>>)))]
pub(crate) async fn categories(
    State(state): State<MenuState>,
    Query(filter): Query<Filter>,
) -> Reply<Vec<MenuCategory>> {
    let Some(pool) = state.employee.as_deref().and_then(LoginService::database) else {
        return unavailable();
    };
    let kind = filter.r#type.unwrap_or(0);
    if !(0..=2).contains(&kind) {
        return fail(StatusCode::BAD_REQUEST, "分类类型错误");
    }
    result(sqlx::query_as::<_, (i64, String, i32, i32)>("SELECT id, CONVERT(name USING utf8mb4), type, sort FROM category WHERE status=1 AND type IN (1,2) AND (?=0 OR type=?) ORDER BY sort,id")
        .bind(kind).bind(kind).fetch_all(pool).await.map(|rows| rows.into_iter().map(|(id,name,kind,sort)| MenuCategory { id, name, r#type:kind, sort }).collect()))
}

#[derive(Deserialize, Serialize, ToSchema)]
struct MenuFlavor {
    name: String,
    value: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MenuDish {
    id: i64,
    name: String,
    category_id: i64,
    price: f64,
    image: String,
    description: String,
    flavors: Vec<MenuFlavor>,
}

#[utoipa::path(get, path="/user/dish/list", tag="菜单", summary="查看分类内在售菜品", security(()),
    params(("categoryId"=i64, Query)), responses((status=200, description="含口味的菜品列表。", body=ApiResult<Vec<MenuDish>>)))]
pub(crate) async fn dishes(
    State(state): State<MenuState>,
    Query(filter): Query<Filter>,
) -> Reply<Vec<MenuDish>> {
    let Some(category) = filter.category_id.filter(|id| *id > 0) else {
        return fail(StatusCode::BAD_REQUEST, "请选择菜品分类");
    };
    let Some(pool) = state.employee.as_deref().and_then(LoginService::database) else {
        return unavailable();
    };
    result(menu_cache::load(state.redis, "dish", category, load_dishes(pool, category)).await)
}

async fn load_dishes(pool: &MySqlPool, category: i64) -> Result<Vec<MenuDish>, sqlx::Error> {
    type Row = (
        i64,
        String,
        i64,
        f64,
        String,
        String,
        Option<String>,
        Option<String>,
    );
    let rows = sqlx::query_as::<_,Row>("SELECT d.id,CONVERT(d.name USING utf8mb4),d.category_id,CAST(d.price AS DOUBLE),
        CONVERT(COALESCE(d.image,'') USING utf8mb4),CONVERT(COALESCE(d.description,'') USING utf8mb4),CONVERT(f.name USING utf8mb4),CONVERT(f.value USING utf8mb4)
        FROM dish d JOIN category c ON c.id=d.category_id LEFT JOIN dish_flavor f ON f.dish_id=d.id
        WHERE d.category_id=? AND d.status=1 AND d.price IS NOT NULL AND c.type=1 AND c.status=1 ORDER BY d.id,f.id")
        .bind(category).fetch_all(pool).await;
    rows.map(|rows| {
        let mut dishes = BTreeMap::new();
        for (id, name, category_id, price, image, description, flavor, value) in rows {
            let dish = dishes.entry(id).or_insert_with(|| MenuDish {
                id,
                name,
                category_id,
                price,
                image,
                description,
                flavors: Vec::new(),
            });
            if let (Some(name), Some(value)) = (flavor, value) {
                dish.flavors.push(MenuFlavor { name, value });
            }
        }
        dishes.into_values().collect()
    })
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MenuSetmeal {
    id: i64,
    name: String,
    category_id: i64,
    price: f64,
    image: String,
    description: String,
}

#[utoipa::path(get, path="/user/setmeal/list", tag="菜单", summary="查看分类内在售套餐", security(()),
    params(("categoryId"=i64, Query)), responses((status=200, description="套餐列表。", body=ApiResult<Vec<MenuSetmeal>>)))]
pub(crate) async fn setmeals(
    State(state): State<MenuState>,
    Query(filter): Query<Filter>,
) -> Reply<Vec<MenuSetmeal>> {
    let Some(category) = filter.category_id.filter(|id| *id > 0) else {
        return fail(StatusCode::BAD_REQUEST, "请选择套餐分类");
    };
    let Some(pool) = state.employee.as_deref().and_then(LoginService::database) else {
        return unavailable();
    };
    result(
        menu_cache::load(
            state.redis,
            "setmeal",
            category,
            load_setmeals(pool, category),
        )
        .await,
    )
}

async fn load_setmeals(pool: &MySqlPool, category: i64) -> Result<Vec<MenuSetmeal>, sqlx::Error> {
    sqlx::query_as::<_,(i64,String,i64,f64,String,String)>("SELECT s.id,CONVERT(s.name USING utf8mb4),s.category_id,CAST(s.price AS DOUBLE),
        CONVERT(COALESCE(s.image,'') USING utf8mb4),CONVERT(COALESCE(s.description,'') USING utf8mb4)
        FROM setmeal s JOIN category c ON c.id=s.category_id WHERE s.category_id=? AND s.status=1 AND c.type=2 AND c.status=1 ORDER BY s.id")
        .bind(category).fetch_all(pool).await.map(|rows| rows.into_iter().map(|(id,name,category_id,price,image,description)| MenuSetmeal {id,name,category_id,price,image,description}).collect())
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MenuMealItem {
    name: String,
    image: String,
    description: String,
    copies: i32,
}

#[utoipa::path(get, path="/user/setmeal/dish/{id}", tag="菜单", summary="查看套餐中的菜品和份数", security(()),
    params(("id"=i64, Path)), responses((status=200, description="套餐菜品列表。", body=ApiResult<Vec<MenuMealItem>>)))]
pub(crate) async fn meal_dishes(
    State(state): State<MenuState>,
    Path(id): Path<i64>,
) -> Reply<Vec<MenuMealItem>> {
    let Some(pool) = state.employee.as_deref().and_then(LoginService::database) else {
        return unavailable();
    };
    result(sqlx::query_as::<_,(String,String,String,i32)>("SELECT CONVERT(d.name USING utf8mb4),CONVERT(COALESCE(d.image,'') USING utf8mb4),
        CONVERT(COALESCE(d.description,'') USING utf8mb4),COALESCE(sd.copies,1) FROM setmeal_dish sd
        JOIN setmeal s ON s.id=sd.setmeal_id JOIN category c ON c.id=s.category_id JOIN dish d ON d.id=sd.dish_id
        WHERE s.id=? AND s.status=1 AND c.type=2 AND c.status=1 AND d.status=1 ORDER BY sd.id")
        .bind(id).fetch_all(pool).await.map(|rows| rows.into_iter().map(|(name,image,description,copies)| MenuMealItem {name,image,description,copies}).collect()))
}

fn fail<T>(status: StatusCode, message: &'static str) -> Reply<T> {
    (status, Json(ApiResult::failure(message)))
}
fn unavailable<T>() -> Reply<T> {
    fail(StatusCode::SERVICE_UNAVAILABLE, "菜单暂不可用")
}
fn result<T>(value: Result<T, sqlx::Error>) -> Reply<T> {
    match value {
        Ok(data) => (StatusCode::OK, Json(ApiResult::success(data))),
        Err(error) => {
            eprintln!("Menu read failed: {error}");
            unavailable()
        }
    }
}

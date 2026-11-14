use crate::{
    common::{ApiResult, NoData},
    employee::{
        EmployeeIdentity, EmployeeState, authenticate_employee,
        manage::{admin_database, denied},
    },
};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

type Reply<T> = (StatusCode, Json<ApiResult<T>>);

pub fn routes(service: EmployeeState) -> Router {
    Router::new()
        .route("/", post(create).put(edit).delete(remove))
        .route("/page", get(page))
        .route("/list", get(list))
        .route("/status/{status}", post(set_status))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            authenticate_employee,
        ))
        .with_state(service)
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Category {
    id: i64,
    name: String,
    r#type: Option<i32>,
    sort: i32,
    status: Option<i32>,
    update_time: Option<String>,
}

type CategoryRow = (i64, String, Option<i32>, i32, Option<i32>, Option<String>);

fn category_row((id, name, kind, sort, status, update_time): CategoryRow) -> Category {
    Category {
        id,
        name,
        r#type: kind,
        sort,
        status,
        update_time,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CategoryQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    name: Option<String>,
    r#type: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct CategoryPage {
    total: u64,
    records: Vec<Category>,
}

#[utoipa::path(get, path = "/api/category/page", tag = "分类", summary = "分类分页查询", security(("EmployeeToken" = [])),
    params(("page" = Option<u32>, Query), ("pageSize" = Option<u32>, Query), ("name" = Option<String>, Query), ("type" = Option<i32>, Query)),
    responses((status = 200, description = "分类分页结果。", body = ApiResult<CategoryPage>)))]
pub(crate) async fn page(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(query): Query<CategoryQuery>,
) -> Reply<CategoryPage> {
    let page = query.page.unwrap_or(1);
    let size = query.page_size.unwrap_or(10);
    let kind = query.r#type.unwrap_or(0);
    let name = query.name.as_deref().unwrap_or("").trim();
    if page == 0
        || !(1..=100).contains(&size)
        || !(0..=2).contains(&kind)
        || name.chars().count() > 32
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResult::failure("分类查询参数错误")),
        );
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<CategoryPage, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        let total = sqlx::query_scalar::<_, u64>(
            "SELECT CAST(COUNT(*) AS UNSIGNED) FROM category WHERE (? = 0 OR type = ?) AND (? = '' OR LOCATE(?, name) > 0)",
        ).bind(kind).bind(kind).bind(name).bind(name).fetch_one(&mut *tx).await?;
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT id, CONVERT(name USING utf8mb4), type, sort, status, DATE_FORMAT(update_time, '%Y-%m-%d %H:%i:%s')
             FROM category WHERE (? = 0 OR type = ?) AND (? = '' OR LOCATE(?, name) > 0)
             ORDER BY sort, id LIMIT ? OFFSET ?",
        ).bind(kind).bind(kind).bind(name).bind(name).bind(size).bind(u64::from(page - 1) * u64::from(size)).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(CategoryPage { total, records: rows.into_iter().map(category_row).collect() })
    }.await;
    match result {
        Ok(page) => (StatusCode::OK, Json(ApiResult::success(page))),
        Err(error) => {
            eprintln!("Category page query failed: {error}");
            denied(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

#[utoipa::path(get, path = "/api/category/list", tag = "分类", summary = "查询已启用分类", security(("EmployeeToken" = [])),
    params(("type" = Option<i32>, Query)), responses((status = 200, description = "已启用分类。", body = ApiResult<Vec<Category>>)))]
pub(crate) async fn list(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(query): Query<CategoryQuery>,
) -> Reply<Vec<Category>> {
    let kind = query.r#type.unwrap_or(0);
    if !(0..=2).contains(&kind) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResult::failure("分类类型错误")),
        );
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    match sqlx::query_as::<_, CategoryRow>(
        "SELECT id, CONVERT(name USING utf8mb4), type, sort, status, DATE_FORMAT(update_time, '%Y-%m-%d %H:%i:%s')
         FROM category WHERE status = 1 AND (? = 0 OR type = ?) ORDER BY sort, id",
    ).bind(kind).bind(kind).fetch_all(pool).await {
        Ok(rows) => (StatusCode::OK, Json(ApiResult::success(rows.into_iter().map(category_row).collect()))),
        Err(error) => { eprintln!("Category list query failed: {error}"); denied(StatusCode::SERVICE_UNAVAILABLE) }
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CategoryInput {
    id: Option<i64>,
    name: String,
    sort: i32,
    r#type: Option<i32>,
}

fn valid_category(input: &CategoryInput) -> bool {
    (1..=32).contains(&input.name.trim().chars().count()) && input.sort >= 0
}

#[utoipa::path(post, path = "/api/category", tag = "分类", summary = "新增分类", security(("EmployeeToken" = [])),
    request_body = CategoryInput, responses((status = 200, description = "保存结果，新分类默认禁用。", body = ApiResult<NoData>)))]
pub(crate) async fn create(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    request: Result<Json<CategoryInput>, JsonRejection>,
) -> Reply<NoData> {
    let Ok(Json(input)) = request else {
        return invalid_category();
    };
    let Some(kind @ (1 | 2)) = input.r#type else {
        return invalid_category();
    };
    if !valid_category(&input) {
        return invalid_category();
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    finish_mutation(sqlx::query("INSERT INTO category (type, name, sort, status, create_time, update_time, create_user, update_user)
        VALUES (?, ?, ?, 0, UTC_TIMESTAMP(), UTC_TIMESTAMP(), ?, ?)")
        .bind(kind).bind(input.name.trim()).bind(input.sort).bind(actor.0).bind(actor.0).execute(pool).await, "分类不存在")
}

#[utoipa::path(put, path = "/api/category", tag = "分类", summary = "修改分类", security(("EmployeeToken" = [])),
    request_body = CategoryInput, responses((status = 200, description = "保存结果。", body = ApiResult<NoData>)))]
pub(crate) async fn edit(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    request: Result<Json<CategoryInput>, JsonRejection>,
) -> Reply<NoData> {
    let Ok(Json(input)) = request else {
        return invalid_category();
    };
    let Some(id) = input.id.filter(|id| *id > 0) else {
        return invalid_category();
    };
    if !valid_category(&input) {
        return invalid_category();
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    finish_mutation(sqlx::query("UPDATE category SET name = ?, sort = ?, update_time = UTC_TIMESTAMP(), update_user = ? WHERE id = ?")
        .bind(input.name.trim()).bind(input.sort).bind(actor.0).bind(id).execute(pool).await, "分类不存在")
}

#[derive(Deserialize)]
pub(crate) struct CategoryId {
    id: i64,
}

#[utoipa::path(post, path = "/api/category/status/{status}", tag = "分类", summary = "启用或禁用分类", security(("EmployeeToken" = [])),
    params(("status" = i32, Path), ("id" = i64, Query)), responses((status = 200, description = "状态保存结果。", body = ApiResult<NoData>)))]
pub(crate) async fn set_status(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(status): Path<i32>,
    Query(target): Query<CategoryId>,
) -> Reply<NoData> {
    if !matches!(status, 0 | 1) || target.id <= 0 {
        return invalid_category();
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    finish_mutation(sqlx::query("UPDATE category SET status = ?, update_time = UTC_TIMESTAMP(), update_user = ? WHERE id = ?")
        .bind(status).bind(actor.0).bind(target.id).execute(pool).await, "分类不存在")
}

#[utoipa::path(delete, path = "/api/category", tag = "分类", summary = "删除未被使用的分类", security(("EmployeeToken" = [])),
    params(("id" = i64, Query)), responses((status = 200, description = "删除结果。", body = ApiResult<NoData>)))]
pub(crate) async fn remove(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(target): Query<CategoryId>,
) -> Reply<NoData> {
    if target.id <= 0 {
        return invalid_category();
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    finish_mutation(
        sqlx::query(
            "DELETE FROM category WHERE id = ?
        AND NOT EXISTS (SELECT 1 FROM dish WHERE category_id = ?)
        AND NOT EXISTS (SELECT 1 FROM setmeal WHERE category_id = ?)",
        )
        .bind(target.id)
        .bind(target.id)
        .bind(target.id)
        .execute(pool)
        .await,
        "分类不存在或仍有菜品、套餐在使用",
    )
}

fn invalid_category() -> Reply<NoData> {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResult::failure("请检查分类名称、类型和排序")),
    )
}

fn finish_mutation(
    result: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error>,
    missing: &'static str,
) -> Reply<NoData> {
    match result {
        Ok(result) if result.rows_affected() == 1 => {
            (StatusCode::OK, Json(ApiResult::success(NoData)))
        }
        Ok(_) => (StatusCode::OK, Json(ApiResult::failure(missing))),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            (StatusCode::OK, Json(ApiResult::failure("分类名称已存在")))
        }
        Err(_) => {
            eprintln!("Category write failed");
            denied(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

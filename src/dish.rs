use crate::{
    common::{ApiResult, NoData},
    employee::{
        EmployeeIdentity, EmployeeState, authenticate_employee,
        manage::{admin_database, denied},
    },
};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{MySql, QueryBuilder};
use utoipa::ToSchema;

type Reply<T> = (StatusCode, Json<ApiResult<T>>);

pub fn routes(service: EmployeeState) -> Router {
    Router::new()
        .route("/", post(create).put(edit).delete(remove))
        .route("/page", get(page))
        .route("/list", get(list))
        .route("/{id}", get(details))
        .route("/status/{status}", post(set_status))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            authenticate_employee,
        ))
        .with_state(service)
}

#[derive(Deserialize, Serialize, ToSchema)]
pub(crate) struct Flavor {
    name: String,
    value: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Dish {
    id: i64,
    name: String,
    category_id: i64,
    category_name: Option<String>,
    price: f64,
    image: String,
    description: String,
    status: i32,
    update_time: Option<String>,
}

type DishRow = (
    i64,
    String,
    i64,
    Option<String>,
    f64,
    String,
    String,
    i32,
    Option<String>,
);
const SELECT_DISH: &str = "SELECT d.id, CONVERT(d.name USING utf8mb4), d.category_id, CONVERT(c.name USING utf8mb4),
       CAST(COALESCE(d.price, 0) AS DOUBLE), CONVERT(COALESCE(d.image, '') USING utf8mb4),
       CONVERT(COALESCE(d.description, '') USING utf8mb4), COALESCE(d.status, 0), DATE_FORMAT(d.update_time, '%Y-%m-%d %H:%i:%s')
       FROM dish d LEFT JOIN category c ON c.id = d.category_id";

fn dish_row(
    (id, name, category_id, category_name, price, image, description, status, update_time): DishRow,
) -> Dish {
    Dish {
        id,
        name,
        category_id,
        category_name,
        price,
        image,
        description,
        status,
        update_time,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DishQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    name: Option<String>,
    category_id: Option<i64>,
    status: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct DishPage {
    total: u64,
    records: Vec<Dish>,
}

#[utoipa::path(get, path="/api/dish/page", tag="菜品", summary="菜品分页查询", security(("EmployeeToken" = [])),
    params(("page"=Option<u32>, Query), ("pageSize"=Option<u32>, Query), ("name"=Option<String>, Query), ("categoryId"=Option<i64>, Query), ("status"=Option<i32>, Query)),
    responses((status=200, description="分页结果。", body=ApiResult<DishPage>)))]
pub(crate) async fn page(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(query): Query<DishQuery>,
) -> Reply<DishPage> {
    let page = query.page.unwrap_or(1);
    let size = query.page_size.unwrap_or(10);
    let name = query.name.as_deref().unwrap_or("").trim();
    if page == 0
        || !(1..=100).contains(&size)
        || name.chars().count() > 32
        || query.status.is_some_and(|s| !matches!(s, 0 | 1))
    {
        return fail(StatusCode::BAD_REQUEST, "菜品查询参数错误");
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<DishPage, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        const FILTER: &str = " WHERE (? = '' OR LOCATE(?, d.name) > 0) AND (? IS NULL OR d.category_id = ?) AND (? IS NULL OR d.status = ?)";
        let total_sql = format!("SELECT CAST(COUNT(*) AS UNSIGNED) FROM dish d{FILTER}");
        let rows_sql = format!("{SELECT_DISH}{FILTER} ORDER BY d.update_time DESC, d.id DESC LIMIT ? OFFSET ?");
        let total = sqlx::query_scalar::<_, u64>(&total_sql).bind(name).bind(name).bind(query.category_id).bind(query.category_id)
            .bind(query.status).bind(query.status).fetch_one(&mut *tx).await?;
        let rows = sqlx::query_as::<_, DishRow>(&rows_sql).bind(name).bind(name).bind(query.category_id).bind(query.category_id)
            .bind(query.status).bind(query.status).bind(size).bind(u64::from(page - 1)*u64::from(size)).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(DishPage { total, records: rows.into_iter().map(dish_row).collect() })
    }.await;
    db_reply(result)
}

#[utoipa::path(get, path="/api/dish/list", tag="菜品", summary="获取起售菜品", security(("EmployeeToken" = [])),
    params(("categoryId"=Option<i64>, Query), ("name"=Option<String>, Query)), responses((status=200, description="起售菜品。", body=ApiResult<Vec<Dish>>)))]
pub(crate) async fn list(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(query): Query<DishQuery>,
) -> Reply<Vec<Dish>> {
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let sql = format!(
        "{SELECT_DISH} WHERE d.status = 1 AND (? IS NULL OR d.category_id = ?) AND (? IS NULL OR LOCATE(?,d.name)>0) ORDER BY d.id"
    );
    db_reply(
        sqlx::query_as::<_, DishRow>(&sql)
            .bind(query.category_id)
            .bind(query.category_id)
            .bind(
                query
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty()),
            )
            .bind(
                query
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty()),
            )
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(dish_row).collect()),
    )
}

#[derive(Serialize, ToSchema)]
pub(crate) struct DishDetails {
    #[serde(flatten)]
    dish: Dish,
    flavors: Vec<Flavor>,
}

#[utoipa::path(get, path="/api/dish/{id}", tag="菜品", summary="回显菜品与口味", security(("EmployeeToken" = [])),
    params(("id"=i64, Path)), responses((status=200, description="菜品详情。", body=ApiResult<DishDetails>)))]
pub(crate) async fn details(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(id): Path<i64>,
) -> Reply<DishDetails> {
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<Option<DishDetails>, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        let sql = format!("{SELECT_DISH} WHERE d.id = ?");
        let Some(row) = sqlx::query_as::<_, DishRow>(&sql).bind(id).fetch_optional(&mut *tx).await? else { return Ok(None); };
        let flavors = sqlx::query_as::<_, (String, String)>("SELECT CONVERT(COALESCE(name, '') USING utf8mb4), CONVERT(COALESCE(value, '[]') USING utf8mb4) FROM dish_flavor WHERE dish_id = ? ORDER BY id")
            .bind(id).fetch_all(&mut *tx).await?.into_iter().map(|(name, value)| Flavor { name, value }).collect();
        tx.commit().await?;
        Ok(Some(DishDetails { dish: dish_row(row), flavors }))
    }.await;
    match result {
        Ok(Some(row)) => (StatusCode::OK, Json(ApiResult::success(row))),
        Ok(None) => fail(StatusCode::OK, "菜品不存在"),
        Err(error) => db_reply(Err(error)),
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum Price {
    Number(f64),
    Text(String),
}

impl Price {
    pub(crate) fn decimal(&self) -> Option<String> {
        let price = match self {
            Self::Number(value) => value.to_string(),
            Self::Text(value) => value.trim().to_owned(),
        };
        let (whole, fraction) = price.split_once('.').unwrap_or((&price, ""));
        if whole.is_empty()
            || whole.len() > 8
            || !whole.bytes().all(|c| c.is_ascii_digit())
            || fraction.len() > 2
            || !fraction.bytes().all(|c| c.is_ascii_digit())
            || !price.bytes().any(|c| matches!(c, b'1'..=b'9'))
        {
            return None;
        }
        Some(price)
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DishInput {
    id: Option<i64>,
    name: String,
    category_id: i64,
    price: Price,
    image: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    status: i32,
    #[serde(default)]
    flavors: Vec<Flavor>,
}

#[utoipa::path(post, path="/api/dish", tag="菜品", summary="新增菜品及口味", security(("EmployeeToken" = [])),
    request_body=DishInput, responses((status=200, description="保存结果。", body=ApiResult<NoData>)))]
pub(crate) async fn create(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Json(input): Json<DishInput>,
) -> Reply<NoData> {
    save(service, actor, input, None).await
}

#[utoipa::path(put, path="/api/dish", tag="菜品", summary="修改菜品及口味", security(("EmployeeToken" = [])),
    request_body=DishInput, responses((status=200, description="保存结果。", body=ApiResult<NoData>)))]
pub(crate) async fn edit(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Json(input): Json<DishInput>,
) -> Reply<NoData> {
    let Some(id) = input.id.filter(|id| *id > 0) else {
        return fail(StatusCode::BAD_REQUEST, "缺少菜品 ID");
    };
    save(service, actor, input, Some(id)).await
}

async fn save(
    service: EmployeeState,
    actor: EmployeeIdentity,
    input: DishInput,
    id: Option<i64>,
) -> Reply<NoData> {
    let Some(price) = input.price.decimal() else {
        return fail(StatusCode::BAD_REQUEST, "价格须大于 0，最多两位小数");
    };
    if !(1..=32).contains(&input.name.trim().chars().count())
        || input.category_id <= 0
        || input.image.is_empty()
        || input.image.chars().count() > 255
        || input.description.chars().count() > 255
        || !matches!(input.status, 0 | 1)
        || input.flavors.len() > 20
        || input.flavors.iter().any(|f| {
            !(1..=32).contains(&f.name.trim().chars().count())
                || f.value.chars().count() > 255
                || serde_json::from_str::<Vec<String>>(&f.value).is_err()
        })
    {
        return fail(StatusCode::BAD_REQUEST, "请检查菜品名称、分类、图片和口味");
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<Result<(), &'static str>, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        let valid_category = sqlx::query_scalar::<_, i64>("SELECT id FROM category WHERE id = ? AND type = 1 AND status = 1 FOR UPDATE")
            .bind(input.category_id).fetch_optional(&mut *tx).await?;
        if valid_category.is_none() { return Ok(Err("请选择已启用的菜品分类")); }
        let dish_id = if let Some(id) = id {
            let updated = sqlx::query("UPDATE dish SET name=?, category_id=?, price=?, image=?, description=?, status=?, update_user=?, update_time=UTC_TIMESTAMP() WHERE id=?")
                .bind(input.name.trim()).bind(input.category_id).bind(&price).bind(&input.image).bind(&input.description).bind(input.status).bind(actor.0).bind(id).execute(&mut *tx).await?;
            if updated.rows_affected() != 1 { return Ok(Err("菜品不存在")); }
            if input.status == 0 { stop_related_meals(&mut tx, actor, id).await?; }
            sqlx::query("DELETE FROM dish_flavor WHERE dish_id=?").bind(id).execute(&mut *tx).await?;
            id
        } else {
            let inserted = sqlx::query("INSERT INTO dish (name, category_id, price, image, description, status, create_time, update_time, create_user, update_user)
                VALUES (?, ?, ?, ?, ?, 0, UTC_TIMESTAMP(), UTC_TIMESTAMP(), ?, ?)")
                .bind(input.name.trim()).bind(input.category_id).bind(&price).bind(&input.image).bind(&input.description).bind(actor.0).bind(actor.0).execute(&mut *tx).await?;
            inserted.last_insert_id() as i64
        };
        for flavor in input.flavors {
            sqlx::query("INSERT INTO dish_flavor (dish_id, name, value) VALUES (?, ?, ?)")
                .bind(dish_id).bind(flavor.name.trim()).bind(flavor.value).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(Ok(()))
    }.await;
    mutation_reply(result)
}

#[derive(Deserialize)]
pub(crate) struct DishIds {
    ids: String,
}
#[derive(Deserialize)]
pub(crate) struct StatusIds {
    id: String,
}

pub(crate) fn parse_ids(raw: &str) -> Option<Vec<i64>> {
    let mut ids: Vec<i64> = raw
        .split(',')
        .take(101)
        .map(|id| id.trim().parse::<i64>().ok().filter(|id| *id > 0))
        .collect::<Option<_>>()?;
    if ids.is_empty() || ids.len() > 100 {
        return None;
    }
    ids.sort_unstable();
    ids.dedup();
    Some(ids)
}

#[utoipa::path(delete, path="/api/dish", tag="菜品", summary="删除停售且未被套餐引用的菜品", security(("EmployeeToken" = [])),
    params(("ids"=String, Query)), responses((status=200, description="删除结果。", body=ApiResult<NoData>)))]
pub(crate) async fn remove(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(query): Query<DishIds>,
) -> Reply<NoData> {
    let Some(ids) = parse_ids(&query.ids) else {
        return fail(StatusCode::BAD_REQUEST, "请提供有效菜品 ID");
    };
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<Result<(), &'static str>, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        for id in ids {
            let deleted = sqlx::query("DELETE FROM dish WHERE id=? AND COALESCE(status,0)=0 AND NOT EXISTS (SELECT 1 FROM setmeal_dish WHERE dish_id=?)")
                .bind(id).bind(id).execute(&mut *tx).await?;
            if deleted.rows_affected() != 1 { return Ok(Err("菜品不存在、正在售卖或被套餐使用")); }
            sqlx::query("DELETE FROM dish_flavor WHERE dish_id=?").bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(Ok(()))
    }.await;
    mutation_reply(result)
}

#[utoipa::path(post, path="/api/dish/status/{status}", tag="菜品", summary="起售或停售菜品", security(("EmployeeToken" = [])),
    params(("status"=i32, Path), ("id"=String, Query)), responses((status=200, description="状态保存结果。", body=ApiResult<NoData>)))]
pub(crate) async fn set_status(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(status): Path<i32>,
    Query(query): Query<StatusIds>,
) -> Reply<NoData> {
    let Some(ids) = parse_ids(&query.id).filter(|_| matches!(status, 0 | 1)) else {
        return fail(StatusCode::BAD_REQUEST, "菜品 ID 或状态错误");
    };
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<Result<(), &'static str>, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        let mut query = QueryBuilder::<MySql>::new("UPDATE dish SET status=");
        query
            .push_bind(status)
            .push(", update_user=")
            .push_bind(actor.0)
            .push(", update_time=UTC_TIMESTAMP() WHERE id IN (");
        let mut bound = query.separated(", ");
        for id in &ids {
            bound.push_bind(id);
        }
        bound.push_unseparated(")");
        if query.build().execute(&mut *tx).await?.rows_affected() != ids.len() as u64 {
            return Ok(Err("部分菜品不存在"));
        }
        if status == 0 {
            for id in ids {
                stop_related_meals(&mut tx, actor, id).await?;
            }
        }
        tx.commit().await?;
        Ok(Ok(()))
    }
    .await;
    mutation_reply(result)
}

async fn stop_related_meals(
    connection: &mut sqlx::MySqlConnection,
    actor: EmployeeIdentity,
    dish_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE setmeal SET status=0, update_user=?, update_time=UTC_TIMESTAMP()
        WHERE status=1 AND id IN (SELECT setmeal_id FROM setmeal_dish WHERE dish_id=?)",
    )
    .bind(actor.0)
    .bind(dish_id)
    .execute(connection)
    .await?;
    Ok(())
}

fn fail<T>(status: StatusCode, message: &'static str) -> Reply<T> {
    (status, Json(ApiResult::failure(message)))
}

fn db_reply<T>(result: Result<T, sqlx::Error>) -> Reply<T> {
    match result {
        Ok(data) => (StatusCode::OK, Json(ApiResult::success(data))),
        Err(error) => {
            eprintln!("Dish query failed: {error}");
            denied(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

fn mutation_reply(result: Result<Result<(), &'static str>, sqlx::Error>) -> Reply<NoData> {
    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(ApiResult::success(NoData))),
        Ok(Err(message)) => fail(StatusCode::OK, message),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            fail(StatusCode::OK, "菜品名称已存在")
        }
        Err(error) => {
            eprintln!("Dish write failed: {error}");
            denied(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

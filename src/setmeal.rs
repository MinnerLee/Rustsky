use crate::{
    common::{ApiResult, NoData},
    dish::{Price, parse_ids},
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
use std::collections::HashSet;
use utoipa::ToSchema;

type Reply<T> = (StatusCode, Json<ApiResult<T>>);

pub fn routes(service: EmployeeState) -> Router {
    Router::new()
        .route("/", post(create).put(edit).delete(remove))
        .route("/page", get(page))
        .route("/{id}", get(details))
        .route("/status/{status}", post(set_status))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            authenticate_employee,
        ))
        .with_state(service)
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct Setmeal {
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

type SetmealRow = (
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
const SELECT_MEAL: &str = "SELECT s.id, CONVERT(s.name USING utf8mb4), s.category_id, CONVERT(c.name USING utf8mb4),
    CAST(s.price AS DOUBLE), CONVERT(COALESCE(s.image, '') USING utf8mb4), CONVERT(COALESCE(s.description, '') USING utf8mb4),
    COALESCE(s.status, 0), DATE_FORMAT(s.update_time, '%Y-%m-%d %H:%i:%s') FROM setmeal s LEFT JOIN category c ON c.id=s.category_id";

fn from_row(
    (id, name, category_id, category_name, price, image, description, status, update_time): SetmealRow,
) -> Setmeal {
    Setmeal {
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
pub(crate) struct MealQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    name: Option<String>,
    category_id: Option<i64>,
    status: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MealPage {
    total: u64,
    records: Vec<Setmeal>,
}

#[utoipa::path(get, path="/api/setmeal/page", tag="套餐", summary="套餐分页查询", security(("EmployeeToken" = [])),
    params(("page"=Option<u32>, Query), ("pageSize"=Option<u32>, Query), ("name"=Option<String>, Query), ("categoryId"=Option<i64>, Query), ("status"=Option<i32>, Query)),
    responses((status=200, description="套餐分页结果。", body=ApiResult<MealPage>)))]
pub(crate) async fn page(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(query): Query<MealQuery>,
) -> Reply<MealPage> {
    let page = query.page.unwrap_or(1);
    let size = query.page_size.unwrap_or(10);
    let name = query.name.as_deref().unwrap_or("").trim();
    if page == 0
        || !(1..=100).contains(&size)
        || name.chars().count() > 32
        || query.status.is_some_and(|status| !matches!(status, 0 | 1))
    {
        return fail(StatusCode::BAD_REQUEST, "套餐查询参数错误");
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<MealPage, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        const FILTER: &str = " WHERE (? = '' OR LOCATE(?, s.name)>0) AND (? IS NULL OR s.category_id=?) AND (? IS NULL OR s.status=?)";
        let total_sql = format!("SELECT CAST(COUNT(*) AS UNSIGNED) FROM setmeal s{FILTER}");
        let page_sql = format!("{SELECT_MEAL}{FILTER} ORDER BY s.update_time DESC, s.id DESC LIMIT ? OFFSET ?");
        let total = sqlx::query_scalar::<_, u64>(&total_sql).bind(name).bind(name).bind(query.category_id).bind(query.category_id)
            .bind(query.status).bind(query.status).fetch_one(&mut *tx).await?;
        let rows = sqlx::query_as::<_, SetmealRow>(&page_sql).bind(name).bind(name).bind(query.category_id).bind(query.category_id)
            .bind(query.status).bind(query.status).bind(size).bind(u64::from(page-1)*u64::from(size)).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(MealPage { total, records: rows.into_iter().map(from_row).collect() })
    }.await;
    db_reply(result)
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct MealDish {
    dish_id: i64,
    name: String,
    price: f64,
    copies: i32,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MealDetails {
    #[serde(flatten)]
    meal: Setmeal,
    setmeal_dishes: Vec<MealDish>,
}

#[utoipa::path(get, path="/api/setmeal/{id}", tag="套餐", summary="回显套餐和所含菜品", security(("EmployeeToken" = [])),
    params(("id"=i64, Path)), responses((status=200, description="套餐详情。", body=ApiResult<MealDetails>)))]
pub(crate) async fn details(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(id): Path<i64>,
) -> Reply<MealDetails> {
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<Option<MealDetails>, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        let sql = format!("{SELECT_MEAL} WHERE s.id=?");
        let Some(row) = sqlx::query_as::<_, SetmealRow>(&sql).bind(id).fetch_optional(&mut *tx).await? else { return Ok(None); };
        let items = sqlx::query_as::<_, (i64, String, f64, i32)>("SELECT dish_id, CONVERT(COALESCE(name,'') USING utf8mb4), CAST(COALESCE(price,0) AS DOUBLE), COALESCE(copies,1) FROM setmeal_dish WHERE setmeal_id=? AND dish_id IS NOT NULL ORDER BY id")
            .bind(id).fetch_all(&mut *tx).await?.into_iter().map(|(dish_id, name, price, copies)| MealDish { dish_id, name, price, copies }).collect();
        tx.commit().await?;
        Ok(Some(MealDetails { meal: from_row(row), setmeal_dishes: items }))
    }.await;
    match result {
        Ok(Some(details)) => db_reply(Ok(details)),
        Ok(None) => fail(StatusCode::OK, "套餐不存在"),
        Err(error) => db_reply(Err(error)),
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct MealDishInput {
    dish_id: i64,
    copies: i32,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MealInput {
    id: Option<i64>,
    name: String,
    category_id: i64,
    price: Price,
    image: String,
    setmeal_dishes: Vec<MealDishInput>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    status: i32,
}

#[utoipa::path(post, path="/api/setmeal", tag="套餐", summary="新增套餐", security(("EmployeeToken" = [])),
    request_body=MealInput, responses((status=200, description="新增结果。", body=ApiResult<NoData>)))]
pub(crate) async fn create(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Json(input): Json<MealInput>,
) -> Reply<NoData> {
    save(service, actor, input, None).await
}

#[utoipa::path(put, path="/api/setmeal", tag="套餐", summary="修改套餐", security(("EmployeeToken" = [])),
    request_body=MealInput, responses((status=200, description="修改结果。", body=ApiResult<NoData>)))]
pub(crate) async fn edit(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Json(input): Json<MealInput>,
) -> Reply<NoData> {
    let Some(id) = input.id.filter(|id| *id > 0) else {
        return fail(StatusCode::BAD_REQUEST, "缺少套餐 ID");
    };
    save(service, actor, input, Some(id)).await
}

async fn save(
    service: EmployeeState,
    actor: EmployeeIdentity,
    mut input: MealInput,
    id: Option<i64>,
) -> Reply<NoData> {
    let Some(price) = input.price.decimal() else {
        return fail(StatusCode::BAD_REQUEST, "价格须大于 0，最多两位小数");
    };
    let mut dish_ids = HashSet::new();
    if !(1..=32).contains(&input.name.trim().chars().count())
        || input.category_id <= 0
        || !matches!(input.status, 0 | 1)
        || input.image.is_empty()
        || input.image.chars().count() > 255
        || input.description.chars().count() > 255
        || !(1..=100).contains(&input.setmeal_dishes.len())
        || input.setmeal_dishes.iter().any(|dish| {
            dish.dish_id <= 0 || !(1..=99).contains(&dish.copies) || !dish_ids.insert(dish.dish_id)
        })
    {
        return fail(
            StatusCode::BAD_REQUEST,
            "请检查套餐名称、分类、图片和菜品份数",
        );
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<Result<(), &'static str>, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        let category = sqlx::query_scalar::<_, i64>("SELECT id FROM category WHERE id=? AND type=2 AND status=1 FOR UPDATE")
            .bind(input.category_id).fetch_optional(&mut *tx).await?;
        if category.is_none() { return Ok(Err("请选择已启用的套餐分类")); }
        input.setmeal_dishes.sort_by_key(|dish| dish.dish_id);
        let mut dishes = Vec::with_capacity(input.setmeal_dishes.len());
        for item in input.setmeal_dishes {
            let row = sqlx::query_as::<_, (String, String)>("SELECT CONVERT(name USING utf8mb4), CAST(price AS CHAR) FROM dish WHERE id=? AND status=1 AND price IS NOT NULL FOR UPDATE")
                .bind(item.dish_id).fetch_optional(&mut *tx).await?;
            let Some((name, price)) = row else { return Ok(Err("套餐包含不存在或停售的菜品")); };
            dishes.push((item, name, price));
        }
        let meal_id = if let Some(id) = id {
            let updated = sqlx::query("UPDATE setmeal SET category_id=?, name=?, price=?, status=?, description=?, image=?, update_time=UTC_TIMESTAMP(), update_user=? WHERE id=?")
                .bind(input.category_id).bind(input.name.trim()).bind(&price).bind(input.status).bind(&input.description).bind(&input.image).bind(actor.0).bind(id).execute(&mut *tx).await?;
            if updated.rows_affected() != 1 { return Ok(Err("套餐不存在")); }
            sqlx::query("DELETE FROM setmeal_dish WHERE setmeal_id=?").bind(id).execute(&mut *tx).await?;
            id
        } else {
            let inserted = sqlx::query("INSERT INTO setmeal (category_id,name,price,status,description,image,create_time,update_time,create_user,update_user)
                VALUES (?,?,?,0,?,?,UTC_TIMESTAMP(),UTC_TIMESTAMP(),?,?)")
                .bind(input.category_id).bind(input.name.trim()).bind(&price).bind(&input.description).bind(&input.image).bind(actor.0).bind(actor.0).execute(&mut *tx).await?;
            inserted.last_insert_id() as i64
        };
        for (item, name, price) in dishes {
            sqlx::query("INSERT INTO setmeal_dish (setmeal_id,dish_id,name,price,copies) VALUES (?,?,?,?,?)")
                .bind(meal_id).bind(item.dish_id).bind(name).bind(price).bind(item.copies).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(Ok(()))
    }.await;
    mutation_reply(result)
}

#[derive(Deserialize)]
pub(crate) struct MealIds {
    ids: String,
}
#[derive(Deserialize)]
pub(crate) struct StatusIds {
    id: String,
}

#[utoipa::path(delete, path="/api/setmeal", tag="套餐", summary="删除停售套餐", security(("EmployeeToken" = [])),
    params(("ids"=String, Query)), responses((status=200, description="删除结果。", body=ApiResult<NoData>)))]
pub(crate) async fn remove(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(query): Query<MealIds>,
) -> Reply<NoData> {
    let Some(ids) = parse_ids(&query.ids) else {
        return fail(StatusCode::BAD_REQUEST, "请提供有效套餐 ID");
    };
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<Result<(), &'static str>, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        for id in ids {
            let deleted = sqlx::query("DELETE FROM setmeal WHERE id=? AND COALESCE(status,0)=0")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            if deleted.rows_affected() != 1 {
                return Ok(Err("套餐不存在或正在售卖"));
            }
            sqlx::query("DELETE FROM setmeal_dish WHERE setmeal_id=?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(Ok(()))
    }
    .await;
    mutation_reply(result)
}

#[utoipa::path(post, path="/api/setmeal/status/{status}", tag="套餐", summary="起售或停售套餐", security(("EmployeeToken" = [])),
    params(("status"=i32, Path), ("id"=String, Query)), responses((status=200, description="状态保存结果。", body=ApiResult<NoData>)))]
pub(crate) async fn set_status(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(status): Path<i32>,
    Query(query): Query<StatusIds>,
) -> Reply<NoData> {
    let Some(ids) = parse_ids(&query.id).filter(|_| matches!(status, 0 | 1)) else {
        return fail(StatusCode::BAD_REQUEST, "套餐 ID 或状态错误");
    };
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result: Result<Result<(), &'static str>, sqlx::Error> = async {
        let mut tx = pool.begin().await?;
        for id in ids {
            let updated = sqlx::query("UPDATE setmeal SET status=?, update_user=?, update_time=UTC_TIMESTAMP() WHERE id=?
                AND (?=0 OR (EXISTS (SELECT 1 FROM setmeal_dish sd WHERE sd.setmeal_id=?)
                AND NOT EXISTS (SELECT 1 FROM setmeal_dish sd LEFT JOIN dish d ON d.id=sd.dish_id WHERE sd.setmeal_id=? AND COALESCE(d.status,0)<>1)))")
                .bind(status).bind(actor.0).bind(id).bind(status).bind(id).bind(id).execute(&mut *tx).await?;
            if updated.rows_affected() != 1 { return Ok(Err("套餐不存在或所含菜品未全部起售")); }
        }
        tx.commit().await?;
        Ok(Ok(()))
    }.await;
    mutation_reply(result)
}

fn fail<T>(status: StatusCode, message: &'static str) -> Reply<T> {
    (status, Json(ApiResult::failure(message)))
}

fn db_reply<T>(result: Result<T, sqlx::Error>) -> Reply<T> {
    match result {
        Ok(data) => (StatusCode::OK, Json(ApiResult::success(data))),
        Err(error) => {
            eprintln!("Setmeal query failed: {error}");
            denied(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

fn mutation_reply(result: Result<Result<(), &'static str>, sqlx::Error>) -> Reply<NoData> {
    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(ApiResult::success(NoData))),
        Ok(Err(message)) => fail(StatusCode::OK, message),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            fail(StatusCode::OK, "套餐名称已存在")
        }
        Err(error) => {
            eprintln!("Setmeal write failed: {error}");
            denied(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

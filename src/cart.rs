use crate::{
    common::{ApiResult, NoData},
    customer::{CustomerIdentity, CustomerState, Reply, authenticate, fail},
};
use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, MySql, MySqlPool};
use utoipa::ToSchema;

pub fn routes(state: CustomerState) -> Router {
    Router::new()
        .route("/add", post(add))
        .route("/sub", post(subtract))
        .route("/list", get(list))
        .route("/clean", delete(clean))
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CartInput {
    dish_id: Option<i64>,
    setmeal_id: Option<i64>,
    dish_flavor: Option<String>,
}

impl CartInput {
    fn key(&self) -> Option<(i8, i64, &str)> {
        let flavor = self.dish_flavor.as_deref().unwrap_or_default().trim();
        match (self.dish_id, self.setmeal_id) {
            (Some(id), None) if id > 0 && flavor.chars().count() <= 50 => Some((1, id, flavor)),
            (None, Some(id)) if id > 0 && flavor.is_empty() => Some((2, id, flavor)),
            _ => None,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CartLine {
    id: i64,
    name: String,
    image: String,
    dish_id: Option<i64>,
    setmeal_id: Option<i64>,
    dish_flavor: String,
    number: i32,
    amount: f64,
}

#[utoipa::path(post, path="/user/shoppingCart/add", tag="购物车", summary="添加一份菜品或套餐", security(("CustomerToken"=[])),
    request_body=CartInput, responses((status=200, description="加入结果。", body=ApiResult<NoData>)))]
pub(crate) async fn add(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
    Json(input): Json<CartInput>,
) -> Reply<NoData> {
    let Some(key) = input.key() else {
        return fail(StatusCode::BAD_REQUEST, "请选择一个菜品或套餐并检查口味");
    };
    let Some(service) = state else {
        return unavailable();
    };
    match insert(&service.pool, actor.0, &input, key, 1).await {
        Ok(false) => fail(StatusCode::OK, "商品当前不可售"),
        Ok(true) => reply(Ok(NoData)),
        Err(error) => reply(Err(error)),
    }
}

async fn insert<'e>(
    executor: impl Executor<'e, Database = MySql>,
    user: i64,
    input: &CartInput,
    (kind, id, flavor): (i8, i64, &str),
    quantity: i32,
) -> Result<bool, sqlx::Error> {
    let product = if kind == 1 { "dish" } else { "setmeal" };
    let sql = format!("INSERT INTO shopping_cart (user_id,dish_id,setmeal_id,dish_flavor,name,image,amount,number,create_time)
        SELECT ?,?,?,?,p.name,p.image,p.price,?,UTC_TIMESTAMP() FROM {product} p
        JOIN category c ON c.id=p.category_id
        WHERE p.id=? AND p.status=1 AND p.price IS NOT NULL AND c.status=1 AND c.type=?
        ON DUPLICATE KEY UPDATE name=p.name,image=p.image,amount=p.price,number=shopping_cart.number+?");
    let result = sqlx::query(&sql)
        .bind(user)
        .bind(input.dish_id)
        .bind(input.setmeal_id)
        .bind(flavor)
        .bind(quantity)
        .bind(id)
        .bind(kind)
        .bind(quantity)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() != 0)
}

pub(crate) async fn repeat_order(
    pool: &MySqlPool,
    user: i64,
    order: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    type Item = (Option<i64>, Option<i64>, Option<String>, i32);
    let items = sqlx::query_as::<_, Item>(
        "SELECT d.dish_id,d.setmeal_id,CONVERT(d.dish_flavor USING utf8mb4),d.number
        FROM order_detail d JOIN orders o ON o.id=d.order_id
        WHERE o.id=? AND o.user_id=? AND o.status IN (5,6) ORDER BY d.id",
    )
    .bind(order)
    .bind(user)
    .fetch_all(&mut *tx)
    .await?;
    if items.is_empty() {
        return Ok(false);
    }
    for (dish_id, setmeal_id, dish_flavor, quantity) in items {
        let input = CartInput {
            dish_id,
            setmeal_id,
            dish_flavor,
        };
        let Some(key) = input.key() else {
            return Ok(false);
        };
        if quantity <= 0 || !insert(&mut *tx, user, &input, key, quantity).await? {
            return Ok(false);
        }
    }
    tx.commit().await?;
    Ok(true)
}

#[utoipa::path(post, path="/user/shoppingCart/sub", tag="购物车", summary="减少一份商品", security(("CustomerToken"=[])),
    request_body=CartInput, responses((status=200, description="扣减结果。", body=ApiResult<NoData>)))]
pub(crate) async fn subtract(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
    Json(input): Json<CartInput>,
) -> Reply<NoData> {
    let Some((kind, id, flavor)) = input.key() else {
        return fail(StatusCode::BAD_REQUEST, "请选择一个菜品或套餐并检查口味");
    };
    let Some(service) = state else {
        return unavailable();
    };
    reply(decrement(&service.pool, actor.0, kind, id, flavor).await)
}

async fn decrement(
    pool: &MySqlPool,
    user_id: i64,
    kind: i8,
    product_id: i64,
    flavor: &str,
) -> Result<NoData, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let line = sqlx::query_as::<_, (i64, i32)>(
        "SELECT id,number FROM shopping_cart
        WHERE user_id=? AND item_kind=? AND item_id=? AND flavor_key=? FOR UPDATE",
    )
    .bind(user_id)
    .bind(kind)
    .bind(product_id)
    .bind(flavor)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((id, number)) = line {
        let sql = if number <= 1 {
            "DELETE FROM shopping_cart WHERE id=? AND user_id=?"
        } else {
            "UPDATE shopping_cart SET number=number-1 WHERE id=? AND user_id=?"
        };
        sqlx::query(sql)
            .bind(id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(NoData)
}

#[utoipa::path(get, path="/user/shoppingCart/list", tag="购物车", summary="查看自己的购物车", security(("CustomerToken"=[])),
    responses((status=200, description="购物车明细，amount 为加入时的商品单价。", body=ApiResult<Vec<CartLine>>)))]
pub(crate) async fn list(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
) -> Reply<Vec<CartLine>> {
    let Some(service) = state else {
        return unavailable();
    };
    type Row = (
        i64,
        String,
        String,
        Option<i64>,
        Option<i64>,
        String,
        i32,
        f64,
    );
    let rows = sqlx::query_as::<_, Row>(
        "SELECT id,CONVERT(COALESCE(name,'') USING utf8mb4),
        CONVERT(COALESCE(image,'') USING utf8mb4),dish_id,setmeal_id,
        CONVERT(flavor_key USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,number,CAST(amount AS DOUBLE)
        FROM shopping_cart WHERE user_id=? ORDER BY id",
    )
    .bind(actor.0)
    .fetch_all(&service.pool)
    .await;
    reply(rows.map(|rows| {
        rows.into_iter()
            .map(
                |(id, name, image, dish_id, setmeal_id, dish_flavor, number, amount)| CartLine {
                    id,
                    name,
                    image,
                    dish_id,
                    setmeal_id,
                    dish_flavor,
                    number,
                    amount,
                },
            )
            .collect()
    }))
}

#[utoipa::path(delete, path="/user/shoppingCart/clean", tag="购物车", summary="清空自己的购物车", security(("CustomerToken"=[])),
    responses((status=200, description="清空结果。", body=ApiResult<NoData>)))]
pub(crate) async fn clean(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
) -> Reply<NoData> {
    let Some(service) = state else {
        return unavailable();
    };
    reply(
        sqlx::query("DELETE FROM shopping_cart WHERE user_id=?")
            .bind(actor.0)
            .execute(&service.pool)
            .await
            .map(|_| NoData),
    )
}

fn reply<T>(result: Result<T, sqlx::Error>) -> Reply<T> {
    match result {
        Ok(data) => (StatusCode::OK, Json(ApiResult::success(data))),
        Err(error) => {
            eprintln!("Shopping cart request failed: {error}");
            unavailable()
        }
    }
}

fn unavailable<T>() -> Reply<T> {
    fail(StatusCode::SERVICE_UNAVAILABLE, "购物车暂不可用")
}

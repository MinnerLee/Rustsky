pub(crate) mod automation;
pub(crate) mod manage;
pub(crate) mod query;
pub(crate) mod wechat;

use crate::{
    common::ApiResult,
    customer::{CustomerIdentity, CustomerState, Reply, authenticate, fail},
    employee::{EmployeeState, authenticate_employee},
    shop,
};
use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{get, post, put},
};
use rand_core::{OsRng, RngCore};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::sync::Arc;
use utoipa::ToSchema;

#[derive(Clone)]
pub(crate) struct OrderState {
    customers: CustomerState,
    redis: Option<ConnectionManager>,
    wechat: Option<Arc<wechat::Client>>,
}

pub fn routes(
    customers: CustomerState,
    redis: Option<ConnectionManager>,
    wechat: Option<Arc<wechat::Client>>,
) -> Router {
    Router::new()
        .route("/submit", post(submit))
        .route("/historyOrders", get(query::history))
        .route("/orderDetail/{id}", get(query::customer_details))
        .route("/cancel/{id}", put(manage::customer_cancel))
        .route("/repetition/{id}", post(manage::repeat))
        .route("/dev/payment", put(manage::simulate_payment))
        .route("/payment", put(manage::miniprogram_payment))
        .route("/payment/{number}", get(wechat::status))
        .route(
            "/reminder/{id}",
            get(automation::remind).post(automation::remind),
        )
        .route_layer(middleware::from_fn_with_state(
            customers.clone(),
            authenticate,
        ))
        .with_state(OrderState {
            customers,
            redis,
            wechat,
        })
}

pub fn admin_routes(state: EmployeeState) -> Router {
    Router::new()
        .route("/conditionSearch", get(query::admin_page))
        .route("/details/{id}", get(query::admin_details))
        .route("/statistics", get(query::counts))
        .route("/confirm", put(manage::confirm))
        .route("/rejection", put(manage::reject))
        .route("/cancel", put(manage::admin_cancel))
        .route("/delivery/{id}", put(manage::delivery))
        .route("/complete/{id}", put(manage::complete))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            authenticate_employee,
        ))
        .route("/events", get(automation::socket))
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrderInput {
    address_book_id: i64,
    #[serde(default = "one")]
    pay_method: i32,
    remark: Option<String>,
    #[serde(default = "one")]
    delivery_status: i32,
    estimated_delivery_time: Option<String>,
    #[serde(default = "one")]
    tableware_status: i32,
    #[serde(default)]
    tableware_number: i32,
}

fn one() -> i32 {
    1
}

impl OrderInput {
    fn valid(&self) -> bool {
        self.address_book_id > 0
            && self.pay_method == 1
            && matches!(self.delivery_status, 0 | 1)
            && matches!(self.tableware_status, 0 | 1)
            && (0..=99).contains(&self.tableware_number)
            && self
                .remark
                .as_ref()
                .is_none_or(|value| value.chars().count() <= 100)
            && (self.delivery_status == 1
                || self.estimated_delivery_time.as_ref().is_some_and(|value| {
                    value.len() == 19
                        && value.bytes().enumerate().all(|(i, byte)| match i {
                            4 | 7 => byte == b'-',
                            10 => byte == b' ',
                            13 | 16 => byte == b':',
                            _ => byte.is_ascii_digit(),
                        })
                }))
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrderReceipt {
    id: i64,
    order_number: String,
    order_amount: f64,
    order_time: String,
}

#[utoipa::path(post, path="/user/order/submit", tag="订单", summary="提交购物车并生成待付款订单", security(("CustomerToken"=[])),
    request_body=OrderInput, responses((status=200, description="待付款订单编号、金额和时间。", body=ApiResult<OrderReceipt>)))]
pub(crate) async fn submit(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Json(input): Json<OrderInput>,
) -> Reply<OrderReceipt> {
    if !input.valid() {
        return fail(
            StatusCode::BAD_REQUEST,
            "请检查地址、支付方式、备注和配送时间",
        );
    }
    let Some(service) = state.customers else {
        return fail(StatusCode::SERVICE_UNAVAILABLE, "订单服务暂不可用");
    };
    match shop::is_open(state.redis).await {
        Ok(true) => {}
        Ok(false) => return fail(StatusCode::OK, "店铺已打烊"),
        Err(()) => return fail(StatusCode::SERVICE_UNAVAILABLE, "暂时无法查询营业状态"),
    }
    match checkout(&service.pool, actor.0, input).await {
        Ok(data) => (StatusCode::OK, Json(ApiResult::success(data))),
        Err(CheckoutError::Invalid(message)) => fail(StatusCode::OK, message),
        Err(CheckoutError::Sql(error)) => {
            eprintln!("Order checkout failed: {error}");
            fail(StatusCode::SERVICE_UNAVAILABLE, "订单服务暂不可用")
        }
    }
}

enum CheckoutError {
    Invalid(&'static str),
    Sql(sqlx::Error),
}

impl From<sqlx::Error> for CheckoutError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sql(error)
    }
}

async fn checkout(
    pool: &MySqlPool,
    customer: i64,
    input: OrderInput,
) -> Result<OrderReceipt, CheckoutError> {
    let mut tx = pool.begin().await?;
    let cart = sqlx::query_as::<_, (i64, i32)>(
        "SELECT id,number FROM shopping_cart WHERE user_id=? ORDER BY id FOR UPDATE",
    )
    .bind(customer)
    .fetch_all(&mut *tx)
    .await?;
    if cart.is_empty() {
        return Err(CheckoutError::Invalid("购物车为空"));
    }
    let address = sqlx::query_as::<_, (String, String, String)>(
        "SELECT CONVERT(COALESCE(consignee,'') USING utf8mb4),CONVERT(phone USING utf8mb4),
        CONCAT_WS('',province_name,city_name,district_name,detail)
        FROM address_book WHERE id=? AND user_id=?",
    )
    .bind(input.address_book_id)
    .bind(customer)
    .fetch_optional(&mut *tx)
    .await?
    .filter(|(name, _, address)| !name.is_empty() && !address.is_empty())
    .ok_or(CheckoutError::Invalid("收货地址不存在或不完整"))?;

    let scheduled = input
        .estimated_delivery_time
        .filter(|_| input.delivery_status == 0);
    if let Some(time) = &scheduled {
        let valid: i64 = sqlx::query_scalar(
            "SELECT COALESCE(TIMESTAMPDIFF(SECOND, DATE_ADD(UTC_TIMESTAMP(), INTERVAL 8 HOUR), ?) > 0,0)",
        )
        .bind(time)
        .fetch_one(&mut *tx)
        .await?;
        if valid == 0 {
            return Err(CheckoutError::Invalid("预计送达时间须晚于当前时间"));
        }
    }
    let number = order_number()?;
    let order = sqlx::query("INSERT INTO orders
        (number,status,user_id,address_book_id,order_time,pay_method,pay_status,amount,remark,phone,address,consignee,
        estimated_delivery_time,delivery_status,pack_amount,tableware_status,tableware_number)
        VALUES (?,1,?,?,UTC_TIMESTAMP(),?,0,0,?,?,?,?,DATE_SUB(?, INTERVAL 8 HOUR),?,0,?,?)")
        .bind(&number).bind(customer).bind(input.address_book_id).bind(input.pay_method)
        .bind(input.remark).bind(address.1).bind(address.2).bind(address.0)
        .bind(scheduled).bind(input.delivery_status).bind(input.tableware_status).bind(input.tableware_number)
        .execute(&mut *tx).await?;
    let id = order.last_insert_id() as i64;
    let details = sqlx::query(
        "INSERT INTO order_detail (order_id,name,image,dish_id,setmeal_id,dish_flavor,number,amount)
        SELECT ?,COALESCE(d.name,s.name),COALESCE(d.image,s.image),sc.dish_id,sc.setmeal_id,sc.dish_flavor,sc.number,COALESCE(d.price,s.price)
        FROM shopping_cart sc
        LEFT JOIN dish d ON sc.dish_id=d.id AND d.status=1
        LEFT JOIN setmeal s ON sc.setmeal_id=s.id AND s.status=1
        JOIN category c ON c.id=COALESCE(d.category_id,s.category_id)
        WHERE sc.user_id=? AND sc.number>0 AND c.status=1 AND c.type=IF(sc.dish_id IS NULL,2,1)
        AND COALESCE(d.price,s.price) IS NOT NULL",
    )
    .bind(id)
    .bind(customer)
    .execute(&mut *tx)
    .await?;
    if details.rows_affected() != cart.len() as u64 {
        return Err(CheckoutError::Invalid("购物车有停售或无效商品，请重新确认"));
    }
    let total: i64 = sqlx::query_scalar(
        "SELECT CAST(SUM(amount*number)*100 AS SIGNED) FROM order_detail WHERE order_id=?",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !(1..=9_999_999_999).contains(&total) {
        return Err(CheckoutError::Invalid("订单金额超出允许范围"));
    }
    sqlx::query("UPDATE orders SET amount=(SELECT SUM(amount*number) FROM order_detail WHERE order_id=?) WHERE id=?")
        .bind(id).bind(id).execute(&mut *tx).await?;
    let time = sqlx::query_scalar::<_, String>(
        "SELECT DATE_FORMAT(DATE_ADD(order_time, INTERVAL 8 HOUR), '%Y-%m-%d %H:%i:%s') FROM orders WHERE id=?",
    )
    .bind(id).fetch_one(&mut *tx).await?;
    sqlx::query("DELETE FROM shopping_cart WHERE user_id=?")
        .bind(customer)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(OrderReceipt {
        id,
        order_number: number,
        order_amount: total as f64 / 100.0,
        order_time: time,
    })
}

fn order_number() -> Result<String, CheckoutError> {
    let mut bytes = [0_u8; 16];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| CheckoutError::Invalid("暂时无法生成订单号"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

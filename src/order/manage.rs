use crate::{
    common::NoData,
    customer::{CustomerIdentity, Reply, fail},
    employee::{
        EmployeeIdentity, EmployeeState,
        manage::{admin_database, denied},
    },
};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::{MySqlPool, mysql::MySqlQueryResult};
use utoipa::ToSchema;

use super::{
    OrderState,
    query::{respond, unavailable},
};

#[derive(Deserialize, ToSchema)]
pub(crate) struct OrderId {
    id: i64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Cancellation {
    id: i64,
    cancel_reason: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Rejection {
    id: i64,
    rejection_reason: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaymentInput {
    order_number: String,
    pay_method: i32,
}

#[derive(Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum PaymentResult {
    Simulated { simulated: bool },
    WeChat(super::wechat::MiniPayment),
}

#[utoipa::path(put, path="/api/order/confirm", tag="订单", summary="接单", request_body=OrderId,
    security(("EmployeeToken"=[])), responses((status=200, description="接单结果。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn confirm(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Json(order): Json<OrderId>,
) -> Reply<NoData> {
    admin_transition(
        &state,
        actor,
        order.id,
        "UPDATE orders SET status=3 WHERE id=? AND status=2 AND pay_status=1",
    )
    .await
}

#[utoipa::path(put, path="/api/order/delivery/{id}", tag="订单", summary="开始派送", params(("id"=i64,Path)),
    security(("EmployeeToken"=[])), responses((status=200, description="派送结果。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn delivery(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(id): Path<i64>,
) -> Reply<NoData> {
    admin_transition(
        &state,
        actor,
        id,
        "UPDATE orders SET status=4,delivery_started_at=UTC_TIMESTAMP() WHERE id=? AND status=3 AND pay_status=1",
    )
    .await
}

#[utoipa::path(put, path="/api/order/complete/{id}", tag="订单", summary="完成订单", params(("id"=i64,Path)),
    security(("EmployeeToken"=[])), responses((status=200, description="完成结果。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn complete(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(id): Path<i64>,
) -> Reply<NoData> {
    admin_transition(&state, actor, id, "UPDATE orders SET status=5,delivery_time=UTC_TIMESTAMP() WHERE id=? AND status=4 AND pay_status=1").await
}

async fn admin_transition(
    state: &EmployeeState,
    actor: EmployeeIdentity,
    id: i64,
    sql: &'static str,
) -> Reply<NoData> {
    match admin_database(state, actor).await {
        Ok(pool) => changed(sqlx::query(sql).bind(id).execute(pool).await),
        Err(status) => denied(status),
    }
}

#[utoipa::path(put, path="/api/order/cancel", tag="订单", summary="管理端取消订单", request_body=Cancellation,
    security(("EmployeeToken"=[])), responses((status=200, description="取消结果。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn admin_cancel(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Json(input): Json<Cancellation>,
) -> Reply<NoData> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    admin_end(
        pool,
        input.id,
        &input.cancel_reason,
        "cancel_reason",
        "status IN (1,2,3,4)",
    )
    .await
}

#[utoipa::path(put, path="/api/order/rejection", tag="订单", summary="管理端拒接待接单订单", request_body=Rejection,
    security(("EmployeeToken"=[])), responses((status=200, description="拒单结果。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn reject(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Json(input): Json<Rejection>,
) -> Reply<NoData> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    admin_end(
        pool,
        input.id,
        &input.rejection_reason,
        "rejection_reason",
        "status=2",
    )
    .await
}

async fn admin_end(
    pool: &MySqlPool,
    id: i64,
    reason: &str,
    column: &'static str,
    state: &'static str,
) -> Reply<NoData> {
    if !(1..=255).contains(&reason.trim().chars().count()) {
        return fail(StatusCode::BAD_REQUEST, "请填写原因");
    }
    let sql = format!(
        "UPDATE orders SET status=6, {column}=?, cancel_time=UTC_TIMESTAMP(),
        wx_refund_state=IF(pay_status=1 AND wx_transaction_id IS NOT NULL,'PENDING',wx_refund_state),
        pay_status=IF(pay_status=1 AND payment_simulated=1,2,pay_status) WHERE id=? AND {state}
        AND ((pay_status=0 AND wx_payment_started=0) OR (pay_status=1 AND (payment_simulated=1 OR wx_transaction_id IS NOT NULL)))"
    );
    changed(
        sqlx::query(&sql)
            .bind(reason.trim())
            .bind(id)
            .execute(pool)
            .await,
    )
}

#[utoipa::path(put, path="/user/order/cancel/{id}", tag="订单", summary="顾客取消自己的未发起付款或待接单订单", params(("id"=i64,Path)),
    security(("CustomerToken"=[])), responses((status=200, description="取消结果。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn customer_cancel(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Path(id): Path<i64>,
) -> Reply<NoData> {
    let Some(service) = state.customers else {
        return unavailable();
    };
    changed(
        sqlx::query(
            "UPDATE orders SET status=6,cancel_time=UTC_TIMESTAMP(),cancel_reason='用户取消',
        wx_refund_state=IF(pay_status=1 AND wx_transaction_id IS NOT NULL,'PENDING',wx_refund_state),
        pay_status=IF(pay_status=1 AND payment_simulated=1,2,pay_status) WHERE id=? AND user_id=?
        AND ((status=1 AND pay_status=0 AND wx_payment_started=0)
        OR (status=2 AND pay_status=1 AND (payment_simulated=1 OR wx_transaction_id IS NOT NULL)))",
        )
        .bind(id)
        .bind(actor.0)
        .execute(&service.pool)
        .await,
    )
}

#[utoipa::path(put, path="/user/order/dev/payment", tag="订单", summary="本地模拟支付结果", request_body=OrderId,
    security(("CustomerToken"=[])), responses((status=200, description="待付款订单进入待接单。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn simulate_payment(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Json(input): Json<OrderId>,
) -> Reply<NoData> {
    let Some(service) = state
        .customers
        .filter(|service| service.dev_login_enabled())
    else {
        return fail(StatusCode::NOT_FOUND, "本地模拟支付未启用");
    };
    payment_reply(
        record_simulated_payment(&service.pool, actor.0, input.id).await,
        NoData,
    )
}

#[utoipa::path(put, path="/user/order/payment", tag="订单", summary="小程序预下单或本地模拟付款", request_body=PaymentInput,
    security(("CustomerToken"=[])), responses((status=200, description="小程序收银台参数或模拟支付结果。", body=crate::common::ApiResult<PaymentResult>)))]
pub(crate) async fn miniprogram_payment(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Json(input): Json<PaymentInput>,
) -> Reply<PaymentResult> {
    let Some(service) = state.customers else {
        return unavailable();
    };
    if input.pay_method != 1 || !super::wechat::valid_number(&input.order_number) {
        return fail(StatusCode::OK, "请选择微信支付并提供订单号");
    }
    if let Some(client) = state.wechat {
        return super::wechat::start(&client, &service.pool, actor.0, &input.order_number).await;
    }
    if !service.dev_login_enabled() {
        return fail(StatusCode::SERVICE_UNAVAILABLE, "微信支付未配置");
    }
    let order = sqlx::query_scalar::<_, i64>("SELECT id FROM orders WHERE number=? AND user_id=?")
        .bind(input.order_number)
        .bind(actor.0)
        .fetch_optional(&service.pool)
        .await;
    let result = match order {
        Ok(Some(id)) => record_simulated_payment(&service.pool, actor.0, id).await,
        Ok(None) => Ok(false),
        Err(error) => Err(error),
    };
    payment_reply(result, PaymentResult::Simulated { simulated: true })
}

async fn record_simulated_payment(
    pool: &MySqlPool,
    user: i64,
    id: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query(
        "UPDATE orders SET status=2,pay_status=1,payment_simulated=1,checkout_time=UTC_TIMESTAMP()
            WHERE id=? AND user_id=? AND status=1 AND pay_status=0
            AND user_id IN (SELECT id FROM `user` WHERE openid='rustsky-dev-customer')",
    )
    .bind(id)
    .bind(user)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Ok(false);
    }
    super::automation::add_event(&mut tx, id, 1).await?;
    tx.commit().await?;
    Ok(true)
}

fn payment_reply<T>(result: Result<bool, sqlx::Error>, data: T) -> Reply<T> {
    match result {
        Ok(true) => respond(Ok(data)),
        Ok(false) => fail(StatusCode::OK, "订单不存在或当前状态不能执行此操作"),
        Err(error) => respond(Err(error)),
    }
}

#[utoipa::path(post, path="/user/order/repetition/{id}", tag="订单", summary="把历史订单的在售商品重新加入购物车", params(("id"=i64,Path)),
    security(("CustomerToken"=[])), responses((status=200, description="加入结果。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn repeat(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Path(id): Path<i64>,
) -> Reply<NoData> {
    let Some(service) = state.customers else {
        return unavailable();
    };
    match crate::cart::repeat_order(&service.pool, actor.0, id).await {
        Ok(true) => respond(Ok(NoData)),
        Ok(false) => fail(StatusCode::OK, "订单不可再次下单或其中有商品已停售"),
        Err(error) => respond(Err(error)),
    }
}

fn changed(result: Result<MySqlQueryResult, sqlx::Error>) -> Reply<NoData> {
    match result {
        Ok(result) if result.rows_affected() == 1 => respond(Ok(NoData)),
        Ok(_) => fail(StatusCode::OK, "订单不存在或当前状态不能执行此操作"),
        Err(error) => respond(Err(error)),
    }
}

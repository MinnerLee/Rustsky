mod client;

use super::{OrderState, automation::add_event, query::respond};
use crate::{
    common::ApiResult,
    customer::{CustomerIdentity, CustomerState, Reply, fail},
};
use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
pub(crate) use client::Client;
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::sync::Arc;
use utoipa::ToSchema;

#[derive(Debug)]
enum Error {
    Invalid,
    Network,
    Protocol,
    NotFound,
    Provider(StatusCode),
    Database(sqlx::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(status) => write!(fmt, "provider HTTP {status}"),
            Self::Database(error) => write!(fmt, "database: {error}"),
            error => write!(fmt, "{error:?}"),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Deserialize)]
struct Transaction {
    appid: String,
    mchid: String,
    out_trade_no: String,
    trade_state: String,
    success_time: Option<String>,
    transaction_id: Option<String>,
    amount: Option<Amount>,
    payer: Option<Payer>,
}

#[derive(Deserialize)]
struct Amount {
    total: i64,
    currency: String,
}

#[derive(Deserialize)]
struct Payer {
    openid: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MiniPayment {
    time_stamp: String,
    nonce_str: String,
    package_str: String,
    pay_sign: String,
    sign_type: &'static str,
}

pub(super) fn valid_number(value: &str) -> bool {
    (6..=32).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

pub(super) async fn start(
    client: &Client,
    pool: &MySqlPool,
    user: i64,
    number: &str,
) -> Reply<super::manage::PaymentResult> {
    let order = (async {
        let mut tx = pool.begin().await?;
        type PrepayOrder = (i64, String, i64, String, Option<String>, i64);
        let entry = sqlx::query_as::<_, PrepayOrder>(
            "SELECT o.id,CONVERT(u.openid USING utf8mb4),CAST(o.amount*100 AS SIGNED),
            DATE_FORMAT(DATE_ADD(o.order_time,INTERVAL 15 MINUTE),'%Y-%m-%dT%H:%i:%s+00:00'),
            CONVERT(o.wx_prepay_id USING utf8mb4),
            CAST(COALESCE(o.wx_prepay_requested_at > DATE_SUB(UTC_TIMESTAMP(),INTERVAL 10 SECOND),0) AS SIGNED)
            FROM orders o JOIN `user` u ON u.id=o.user_id
            WHERE o.number=? AND o.user_id=? AND o.status=1 AND o.pay_status=0
            AND o.amount BETWEEN 0.01 AND 21474836.47
            AND u.openid IS NOT NULL AND u.openid <> 'rustsky-dev-customer'
            AND o.order_time > DATE_SUB(UTC_TIMESTAMP(),INTERVAL 13 MINUTE) FOR UPDATE",
        )
        .bind(number)
        .bind(user)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((id, _, _, _, None, 0)) = &entry {
            sqlx::query("UPDATE orders SET wx_payment_started=1,wx_prepay_requested_at=UTC_TIMESTAMP() WHERE id=?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok::<_, sqlx::Error>(entry)
    })
    .await;
    let (id, openid, cents, expiry) = match order {
        Ok(Some((_, _, _, _, Some(prepay_id), _))) => {
            return payment_parameters(client, &prepay_id);
        }
        Ok(Some((id, openid, cents, expiry, None, 0))) => (id, openid, cents, expiry),
        Ok(Some(_)) => return fail(StatusCode::OK, "正在发起微信支付，请稍后重试"),
        Ok(None) => return fail(StatusCode::OK, "订单不存在或已超过发起支付时间"),
        Err(error) => return respond(Err(error)),
    };
    let Ok(cents) = i32::try_from(cents) else {
        return fail(StatusCode::OK, "订单金额超出微信支付范围");
    };
    match client.prepay(number, &openid, cents, &expiry).await {
        Ok(prepay_id) => {
            let result = sqlx::query(
                "UPDATE orders SET wx_prepay_id=? WHERE id=? AND status=1 AND pay_status=0",
            )
            .bind(&prepay_id)
            .bind(id)
            .execute(pool)
            .await;
            match result {
                Ok(result) if result.rows_affected() == 1 => payment_parameters(client, &prepay_id),
                Ok(_) => fail(StatusCode::OK, "订单状态已变化，请查询订单"),
                Err(error) => respond(Err(error)),
            }
        }
        Err(error) => {
            eprintln!("WeChat prepay: {error}");
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "微信预下单暂不可用，请查询订单后重试",
            )
        }
    }
}

fn payment_parameters(client: &Client, prepay_id: &str) -> Reply<super::manage::PaymentResult> {
    match client.mini_payment(prepay_id) {
        Ok(payment) => respond(Ok(super::manage::PaymentResult::WeChat(payment))),
        Err(error) => {
            eprintln!("WeChat mini-program payment signature: {error}");
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "暂时无法生成小程序付款参数",
            )
        }
    }
}

async fn record_paid(client: &Client, pool: &MySqlPool, payment: Transaction) -> Result<(), Error> {
    if payment.appid != client.appid
        || payment.mchid != client.mchid
        || payment.trade_state != "SUCCESS"
        || !valid_number(&payment.out_trade_no)
    {
        return Err(Error::Invalid);
    }
    let amount = payment
        .amount
        .filter(|amount| amount.currency == "CNY" && amount.total > 0)
        .ok_or(Error::Invalid)?;
    let payer = payment.payer.ok_or(Error::Invalid)?;
    let payment_id = payment
        .transaction_id
        .filter(|id| id.len() <= 32 && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        .ok_or(Error::Invalid)?;
    let paid_at = chrono::DateTime::parse_from_rfc3339(
        payment.success_time.as_deref().ok_or(Error::Invalid)?,
    )
    .map_err(|_| Error::Invalid)?
    .naive_utc()
    .format("%Y-%m-%d %H:%M:%S")
    .to_string();
    let mut tx = pool.begin().await?;
    type OrderRow = (i64, i32, i8, i8, i64, String, Option<String>);
    let (id, status, pay_status, simulated, cents, openid, existing) = sqlx::query_as::<_, OrderRow>(
        "SELECT o.id,o.status,o.pay_status,o.payment_simulated,CAST(o.amount*100 AS SIGNED),
        CONVERT(u.openid USING utf8mb4),CONVERT(o.wx_transaction_id USING utf8mb4)
        FROM orders o JOIN `user` u ON u.id=o.user_id WHERE o.number=? AND o.wx_payment_started=1 FOR UPDATE",
    ).bind(&payment.out_trade_no).fetch_optional(&mut *tx).await?.ok_or(Error::Invalid)?;
    if cents != amount.total || openid != payer.openid || simulated != 0 {
        return Err(Error::Invalid);
    }
    if existing.as_deref() == Some(&payment_id) {
        tx.commit().await?;
        return Ok(());
    }
    if existing.is_some() || status != 1 || pay_status != 0 {
        return Err(Error::Invalid);
    }
    sqlx::query(
        "UPDATE orders SET status=2,pay_status=1,wx_transaction_id=?,checkout_time=? WHERE id=?",
    )
    .bind(payment_id)
    .bind(paid_at)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    add_event(&mut tx, id, 1).await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) fn routes(customers: CustomerState, client: Option<Arc<Client>>) -> Router {
    Router::new()
        .route("/wechat/payment", post(notification))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .with_state((customers, client))
}

type NotifyState = (CustomerState, Option<Arc<Client>>);

#[utoipa::path(post, path="/notify/wechat/payment", tag="订单", summary="微信支付 V3 支付与退款通知",
    security(()), responses((status=204, description="通知已入库"), (status=400, description="通知无效"), (status=503, description="暂时无法确认通知")))]
pub(crate) async fn notification(
    State((customers, client)): State<NotifyState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (Some(service), Some(client)) = (customers, client) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let result = handle_notification(&client, &service.pool, &headers, &body).await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(Error::Invalid) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"code":"FAIL","message":"Invalid payment notification"})),
        )
            .into_response(),
        Err(error) => {
            eprintln!("WeChat payment callback: {error}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

async fn handle_notification(
    client: &Client,
    pool: &MySqlPool,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), Error> {
    let (event, data) = client.callback::<serde_json::Value>(headers, body)?;
    if event == "TRANSACTION.SUCCESS" {
        let payment = serde_json::from_value(data).map_err(|_| Error::Invalid)?;
        return record_paid(client, pool, payment).await;
    }
    #[derive(Deserialize)]
    struct RefundNotice {
        mchid: String,
        transaction_id: String,
        out_trade_no: String,
        out_refund_no: String,
        refund_status: String,
        amount: client::RefundAmount,
    }
    let notice: RefundNotice = serde_json::from_value(data).map_err(|_| Error::Invalid)?;
    if event != format!("REFUND.{}", notice.refund_status)
        || notice.mchid != client.mchid
        || !valid_number(&notice.out_trade_no)
        || notice.out_refund_no != format!("rs_{}", notice.out_trade_no)
        || !matches!(
            notice.refund_status.as_str(),
            "SUCCESS" | "CLOSED" | "ABNORMAL"
        )
    {
        return Err(Error::Invalid);
    }
    let row = sqlx::query_as::<_, (i64, i64)>(
        "SELECT id,CAST(amount*100 AS SIGNED) FROM orders
        WHERE number=? AND wx_transaction_id=? AND status=6 AND wx_refund_state IS NOT NULL",
    )
    .bind(&notice.out_trade_no)
    .bind(notice.transaction_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::Invalid)?;
    if notice.amount.total != row.1 || notice.amount.refund != row.1 {
        return Err(Error::Invalid);
    }
    save_refund(pool, row.0, &notice.refund_status).await
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaymentStatus {
    pay_status: i8,
}

#[utoipa::path(get, path="/user/order/payment/{number}", tag="订单", summary="确认本人的付款状态",
    params(("number"=String,Path)), security(("CustomerToken"=[])),
    responses((status=200, description="支付状态", body=ApiResult<PaymentStatus>)))]
pub(crate) async fn status(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Path(number): Path<String>,
) -> Reply<PaymentStatus> {
    let Some(service) = state.customers else {
        return fail(StatusCode::SERVICE_UNAVAILABLE, "订单服务暂不可用");
    };
    if !valid_number(&number) {
        return fail(StatusCode::BAD_REQUEST, "订单号格式错误");
    }
    let row = sqlx::query_as::<_, (i8, i8)>(
        "SELECT pay_status,wx_payment_started FROM orders WHERE number=? AND user_id=?",
    )
    .bind(&number)
    .bind(actor.0)
    .fetch_optional(&service.pool)
    .await;
    let (pay_status, started) = match row {
        Ok(Some(row)) => row,
        Ok(None) => return fail(StatusCode::OK, "订单不存在"),
        Err(error) => return respond(Err(error)),
    };
    if pay_status == 0
        && started == 1
        && let Some(client) = state.wechat
    {
        let claim = sqlx::query("UPDATE orders SET wx_payment_checked_at=UTC_TIMESTAMP()
            WHERE number=? AND user_id=? AND status=1 AND pay_status=0
            AND (wx_payment_checked_at IS NULL OR wx_payment_checked_at <= DATE_SUB(UTC_TIMESTAMP(),INTERVAL 3 SECOND))")
            .bind(&number).bind(actor.0).execute(&service.pool).await;
        match claim {
            Ok(result) if result.rows_affected() == 0 => {
                return respond(Ok(PaymentStatus { pay_status }));
            }
            Ok(_) => {}
            Err(error) => return respond(Err(error)),
        }
        if let Err(error) = reconcile(&client, &service.pool, &number, false).await {
            eprintln!("WeChat payment query: {error}");
            return fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "暂时无法确认付款结果，请稍后查询",
            );
        }
        return respond(
            sqlx::query_scalar::<_, i8>(
                "SELECT pay_status FROM orders WHERE number=? AND user_id=?",
            )
            .bind(&number)
            .bind(actor.0)
            .fetch_one(&service.pool)
            .await
            .map(|pay_status| PaymentStatus { pay_status }),
        );
    }
    respond(Ok(PaymentStatus { pay_status }))
}

async fn reconcile(
    client: &Client,
    pool: &MySqlPool,
    number: &str,
    expired: bool,
) -> Result<(), Error> {
    if !valid_number(number) {
        return Err(Error::Invalid);
    }
    match client.query(number).await {
        Ok(payment)
            if payment.out_trade_no != number
                || payment.appid != client.appid
                || payment.mchid != client.mchid =>
        {
            Err(Error::Invalid)
        }
        Ok(payment) if payment.trade_state == "SUCCESS" => record_paid(client, pool, payment).await,
        Ok(payment) if expired && payment.trade_state == "CLOSED" => {
            close_local(pool, number).await
        }
        Ok(payment) if expired && payment.trade_state == "NOTPAY" => {
            client.close(number).await?;
            close_local(pool, number).await
        }
        Ok(_) | Err(Error::NotFound) if !expired => Ok(()),
        Err(Error::NotFound) => close_local(pool, number).await,
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}

async fn close_local(pool: &MySqlPool, number: &str) -> Result<(), Error> {
    sqlx::query(
        "UPDATE orders SET status=6,cancel_reason='支付超时',cancel_time=UTC_TIMESTAMP()
        WHERE number=? AND status=1 AND pay_status=0 AND wx_payment_started=1
        AND order_time < DATE_SUB(UTC_TIMESTAMP(), INTERVAL 15 MINUTE)",
    )
    .bind(number)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn maintain(client: &Client, pool: &MySqlPool) -> Result<(), sqlx::Error> {
    let pending = sqlx::query_as::<_, (i64, String)>("SELECT id,CONVERT(number USING utf8mb4) FROM orders
        WHERE status=1 AND pay_status=0 AND wx_payment_started=1 AND order_time<DATE_SUB(UTC_TIMESTAMP(), INTERVAL 15 MINUTE)
        AND (wx_payment_checked_at IS NULL OR wx_payment_checked_at < DATE_SUB(UTC_TIMESTAMP(),INTERVAL 1 MINUTE))
        ORDER BY wx_payment_checked_at,id LIMIT 10").fetch_all(pool).await?;
    for (id, number) in pending {
        let claimed = sqlx::query("UPDATE orders SET wx_payment_checked_at=UTC_TIMESTAMP() WHERE id=? AND status=1
            AND (wx_payment_checked_at IS NULL OR wx_payment_checked_at < DATE_SUB(UTC_TIMESTAMP(),INTERVAL 1 MINUTE))")
            .bind(id).execute(pool).await?.rows_affected() == 1;
        if claimed && let Err(error) = reconcile(client, pool, &number, true).await {
            eprintln!("WeChat overdue payment reconciliation: {error}");
        }
    }
    let refunds = sqlx::query_as::<_, (i64,String,i64,String)>("SELECT id,CONVERT(number USING utf8mb4),CAST(amount*100 AS SIGNED),
        CONVERT(wx_refund_state USING utf8mb4) FROM orders WHERE wx_refund_state IN ('PENDING','PROCESSING')
        AND (wx_refund_checked_at IS NULL OR wx_refund_checked_at<DATE_SUB(UTC_TIMESTAMP(), INTERVAL 1 MINUTE))
        ORDER BY wx_refund_checked_at,id LIMIT 10").fetch_all(pool).await?;
    for (id, number, cents, state) in refunds {
        let claimed = sqlx::query("UPDATE orders SET wx_refund_checked_at=UTC_TIMESTAMP() WHERE id=? AND wx_refund_state IN ('PENDING','PROCESSING')
            AND (wx_refund_checked_at IS NULL OR wx_refund_checked_at<DATE_SUB(UTC_TIMESTAMP(), INTERVAL 1 MINUTE))")
            .bind(id).execute(pool).await?.rows_affected() == 1;
        if claimed && let Err(error) = update_refund(client, pool, id, &number, cents, &state).await
        {
            eprintln!("WeChat refund reconciliation: {error}");
        }
    }
    Ok(())
}

async fn update_refund(
    client: &Client,
    pool: &MySqlPool,
    id: i64,
    number: &str,
    cents: i64,
    state: &str,
) -> Result<(), Error> {
    if !valid_number(number) {
        return Err(Error::Invalid);
    }
    let amount = i32::try_from(cents).map_err(|_| Error::Invalid)?;
    let refund = client.refund(number, amount, state).await?;
    if refund.out_trade_no != number
        || refund.out_refund_no != format!("rs_{number}")
        || refund.amount.total != cents
        || refund.amount.refund != cents
        || !matches!(
            refund.status.as_str(),
            "SUCCESS" | "CLOSED" | "PROCESSING" | "ABNORMAL"
        )
    {
        return Err(Error::Invalid);
    }
    save_refund(pool, id, &refund.status).await
}

async fn save_refund(pool: &MySqlPool, id: i64, status: &str) -> Result<(), Error> {
    sqlx::query("UPDATE orders SET wx_refund_state=?,pay_status=IF(?='SUCCESS',2,pay_status)
        WHERE id=? AND status=6 AND pay_status=1
        AND (wx_refund_state IN ('PENDING','PROCESSING') OR (wx_refund_state='ABNORMAL' AND ?='SUCCESS'))")
        .bind(status).bind(status).bind(id).bind(status).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        client::tests::{notice, signed, test_client},
        *,
    };
    use serde_json::json;

    #[tokio::test]
    #[ignore = "requires the migrated local Rustsky MySQL database"]
    async fn signed_notifications_update_mysql_once_and_refund_only_on_success() {
        let pool = sqlx::MySqlPool::connect(&std::env::var("RUSTSKY_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let mut client = test_client();
        let number = super::super::order_number().unwrap_or_else(|_| panic!("test order number"));
        let payer = format!("test-{number}");
        let user =
            sqlx::query("INSERT INTO `user` (openid,create_time) VALUES (?,UTC_TIMESTAMP())")
                .bind(&payer)
                .execute(&pool)
                .await
                .unwrap()
                .last_insert_id();
        let order = sqlx::query(
            "INSERT INTO orders(number,user_id,address_book_id,order_time,amount)
            VALUES (?, ?, 1, UTC_TIMESTAMP(), 59.50)",
        )
        .bind(&number)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        client.test_api = Some(format!("http://{}", listener.local_addr().unwrap()));
        let client = Arc::new(client);
        let app = Router::new()
            .route(
                "/v3/pay/transactions/jsapi",
                post(
                    |State((client, calls)): State<(Arc<Client>, Arc<AtomicUsize>)>| async move {
                        calls.fetch_add(1, Ordering::Relaxed);
                        let body = r#"{"prepay_id":"example-cache-session"}"#;
                        (
                            signed(&client, body.as_bytes(), chrono::Utc::now().timestamp()),
                            body,
                        )
                    },
                ),
            )
            .route("/v3/pay/transactions/out-trade-no/{number}", axum::routing::get(
                |State((client, _)): State<(Arc<Client>, Arc<AtomicUsize>)>, Path(number): Path<String>| async move {
                    let body = json!({"appid":client.appid,"mchid":client.mchid,"out_trade_no":number,"trade_state":"NOTPAY"}).to_string();
                    (signed(&client, body.as_bytes(), chrono::Utc::now().timestamp()), body)
                }))
            .route("/v3/pay/transactions/out-trade-no/{number}/close", post(
                |State((client, _)): State<(Arc<Client>, Arc<AtomicUsize>)>| async move {
                    (StatusCode::NO_CONTENT, signed(&client, b"", chrono::Utc::now().timestamp()))
                }))
            .with_state((client.clone(), calls.clone()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let user_id = user as i64;
        let _ = tokio::join!(
            start(&client, &pool, user_id, &number),
            start(&client, &pool, user_id, &number)
        );
        let reused = start(&client, &pool, user_id, &number).await;
        let reuse_bytes = axum::body::to_bytes(reused.into_response().into_body(), 4096)
            .await
            .unwrap();
        let reuse_json: serde_json::Value = serde_json::from_slice(&reuse_bytes).unwrap();
        let expired_number =
            super::super::order_number().unwrap_or_else(|_| panic!("test order number"));
        let expired_order = sqlx::query("INSERT INTO orders(number,user_id,address_book_id,order_time,amount,wx_payment_started)
            VALUES (?, ?, 1, DATE_SUB(UTC_TIMESTAMP(), INTERVAL 20 MINUTE), 59.50, 1)")
            .bind(&expired_number).bind(user).execute(&pool).await.unwrap().last_insert_id();
        let closed = reconcile(&client, &pool, &expired_number, true).await;
        let closed_status = sqlx::query_scalar::<_, i32>("SELECT status FROM orders WHERE id=?")
            .bind(expired_order)
            .fetch_one(&pool)
            .await;
        server.abort();
        let txn: String = number.bytes().map(|b| char::from(b'0' + b % 10)).collect();
        let payload = json!({"appid":client.appid,"mchid":client.mchid,"out_trade_no":number,
            "trade_state":"SUCCESS","transaction_id":txn,"success_time":"2026-11-14T12:00:00+08:00",
            "amount":{"total":5950,"currency":"CNY"},"payer":{"openid":payer}});
        let mut wrong = payload.clone();
        wrong["amount"]["total"] = json!(5951);
        let (headers, bytes) = notice(&client, "TRANSACTION.SUCCESS", wrong);
        let rejected = handle_notification(&client, &pool, &headers, &bytes).await;
        let (headers, bytes) = notice(&client, "TRANSACTION.SUCCESS", payload);
        let (paid, duplicate) = tokio::join!(
            handle_notification(&client, &pool, &headers, &bytes),
            handle_notification(&client, &pool, &headers, &bytes),
        );
        let paid_row = sqlx::query_as::<_, (i32,i8,i64)>("SELECT status,pay_status,(SELECT COUNT(*) FROM order_event WHERE order_id=orders.id AND type=1)
            FROM orders WHERE id=?").bind(order).fetch_one(&pool).await;
        sqlx::query("UPDATE orders SET status=6,wx_refund_state='PENDING' WHERE id=?")
            .bind(order)
            .execute(&pool)
            .await
            .unwrap();
        let (headers, bytes) = notice(
            &client,
            "REFUND.SUCCESS",
            json!({"mchid":client.mchid,
            "transaction_id":txn,"out_trade_no":number,"out_refund_no":format!("rs_{number}"),"refund_status":"SUCCESS",
            "amount":{"total":5950,"refund":5950}}),
        );
        let refund = handle_notification(&client, &pool, &headers, &bytes).await;
        let refund_row = sqlx::query_as::<_, (i8, String)>(
            "SELECT pay_status,CONVERT(wx_refund_state USING utf8mb4) FROM orders WHERE id=?",
        )
        .bind(order)
        .fetch_one(&pool)
        .await;
        sqlx::query("DELETE FROM order_event WHERE order_id=?")
            .bind(order)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM orders WHERE id=?")
            .bind(order)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM orders WHERE id=?")
            .bind(expired_order)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM `user` WHERE id=?")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(rejected, Err(Error::Invalid)));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        closed.unwrap();
        assert_eq!(closed_status.unwrap(), 6);
        assert_eq!(
            reuse_json["data"]["packageStr"],
            "prepay_id=example-cache-session"
        );
        paid.unwrap();
        duplicate.unwrap();
        assert_eq!(paid_row.unwrap(), (2, 1, 1));
        refund.unwrap();
        assert_eq!(refund_row.unwrap(), (2, "SUCCESS".to_string()));
    }
}

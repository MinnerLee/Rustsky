use crate::{
    common::NoData,
    customer::{CustomerIdentity, Reply, fail},
    employee::{EmployeeIdentity, EmployeeState, LoginService, manage::admin_database},
};
use axum::{
    Extension,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use sqlx::{MySql, MySqlPool, Transaction};
use std::{sync::Arc, time::Duration};
use tokio::time::{MissedTickBehavior, interval, sleep, timeout};

use super::{
    OrderState,
    query::{respond, unavailable},
};

pub(crate) fn start(employees: &EmployeeState, wechat: Option<Arc<super::wechat::Client>>) {
    let Some(pool) = employees
        .as_deref()
        .and_then(LoginService::database)
        .cloned()
    else {
        return;
    };
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(60)).await;
            if let Err(error) = maintain(&pool).await {
                eprintln!("Order maintenance failed: {error}");
            }
            if let Some(client) = &wechat
                && let Err(error) = super::wechat::maintain(client, &pool).await
            {
                eprintln!("WeChat order maintenance failed: {error}");
            }
        }
    });
}

async fn maintain(pool: &MySqlPool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE orders SET status=6,cancel_time=UTC_TIMESTAMP(),cancel_reason='支付超时'
        WHERE status=1 AND pay_status=0 AND wx_payment_started=0 AND order_time < DATE_SUB(UTC_TIMESTAMP(), INTERVAL 15 MINUTE)")
        .execute(pool).await?;
    sqlx::query("UPDATE orders SET status=5,delivery_time=UTC_TIMESTAMP()
        WHERE status=4 AND pay_status=1 AND delivery_started_at < DATE_SUB(UTC_TIMESTAMP(), INTERVAL 1 HOUR)")
        .execute(pool).await?;
    sqlx::query(
        "DELETE FROM order_event WHERE created_at < DATE_SUB(UTC_TIMESTAMP(), INTERVAL 7 DAY)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn add_event(
    tx: &mut Transaction<'_, MySql>,
    id: i64,
    kind: i8,
) -> Result<(), sqlx::Error> {
    // Serialize event IDs with commit order so readers can advance a single cursor.
    sqlx::query_scalar::<_, i8>("SELECT id FROM order_event_gate WHERE id=1 FOR UPDATE")
        .fetch_one(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO order_event (order_id,type,created_at) VALUES (?,?,UTC_TIMESTAMP())")
        .bind(id)
        .bind(kind)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[utoipa::path(get, path="/user/order/reminder/{id}", tag="订单", summary="顾客对自己的待接单订单催单",
    params(("id"=i64,Path)), security(("CustomerToken"=[])),
    responses((status=200, description="催单结果。", body=crate::common::ApiResult<NoData>)))]
pub(crate) async fn remind(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Path(id): Path<i64>,
) -> Reply<NoData> {
    let Some(service) = state.customers else {
        return unavailable();
    };
    match add_reminder(&service.pool, actor.0, id).await {
        Ok(true) => respond(Ok(NoData)),
        Ok(false) => fail(StatusCode::OK, "订单当前无法催单，请稍后再试"),
        Err(error) => respond(Err(error)),
    }
}

async fn add_reminder(pool: &MySqlPool, user: i64, id: i64) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let eligible = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM orders WHERE id=? AND user_id=? AND status=2 AND pay_status=1 FOR UPDATE",
    )
    .bind(id)
    .bind(user)
    .fetch_optional(&mut *tx)
    .await?;
    if eligible.is_none() {
        return Ok(false);
    }
    let recent = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM order_event WHERE order_id=? AND type=2
        AND created_at >= DATE_SUB(UTC_TIMESTAMP(), INTERVAL 1 MINUTE) LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if recent.is_some() {
        return Ok(false);
    }
    add_event(&mut tx, id, 2).await?;
    tx.commit().await?;
    Ok(true)
}

pub(crate) async fn socket(
    State(state): State<EmployeeState>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if state.is_none() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    upgrade
        .max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |socket| send_events(state, socket))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderEvent {
    #[serde(rename = "type")]
    kind: i8,
    order_id: i64,
    content: String,
}

async fn send_events(state: EmployeeState, mut socket: WebSocket) {
    let Ok(Some(Ok(Message::Text(token)))) = timeout(Duration::from_secs(5), socket.recv()).await
    else {
        return;
    };
    let Some(id) = state
        .as_deref()
        .and_then(|service| service.employee_id(&token))
    else {
        return;
    };
    let Ok(pool) = admin_database(&state, EmployeeIdentity(id)).await else {
        return;
    };
    let Ok(mut cursor) =
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(id),0) FROM order_event")
            .fetch_one(pool)
            .await
    else {
        return;
    };
    if socket
        .send(Message::Text(r#"{"type":0}"#.into()))
        .await
        .is_err()
    {
        return;
    }
    let mut poll = interval(Duration::from_secs(1));
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut heartbeat = interval(Duration::from_secs(25));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            frame = socket.recv() => if !matches!(frame, Some(Ok(Message::Pong(_) | Message::Ping(_)))) { return; },
            _ = heartbeat.tick() => {
                if state.as_deref().and_then(|service| service.employee_id(&token)).is_none()
                    || admin_database(&state, EmployeeIdentity(id)).await.is_err()
                    || socket.send(Message::Ping(Vec::new().into())).await.is_err()
                {
                    return;
                }
            }
            _ = poll.tick() => {
                let Ok(events) = sqlx::query_as::<_, (i64, i8, i64, String)>(
                    "SELECT e.id,e.type,e.order_id,CONVERT(COALESCE(o.number,'') USING utf8mb4)
                    FROM order_event e JOIN orders o ON o.id=e.order_id WHERE e.id>? ORDER BY e.id LIMIT 50",
                ).bind(cursor).fetch_all(pool).await else { return; };
                for (id, kind, order_id, number) in events {
                    cursor = id;
                    let action = if kind == 1 { "待接单" } else { "顾客催单" };
                    let event = OrderEvent { kind, order_id, content: format!("订单 {number} {action}") };
                    if let Ok(json) = serde_json::to_string(&event)
                        && socket.send(Message::Text(json.into())).await.is_err()
                    {
                        return;
                    }
                }
            }
        }
    }
}

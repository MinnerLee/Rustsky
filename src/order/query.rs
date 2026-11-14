use crate::{
    common::ApiResult,
    customer::{CustomerIdentity, Reply, fail},
    employee::{
        EmployeeIdentity, EmployeeState,
        manage::{admin_database, denied},
    },
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::{MySql, MySqlPool, QueryBuilder, Row, mysql::MySqlRow};
use utoipa::{IntoParams, ToSchema};

use super::OrderState;

#[derive(Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub(crate) struct OrderFilter {
    page: Option<u32>,
    page_size: Option<u32>,
    status: Option<i32>,
    number: Option<String>,
    phone: Option<String>,
    begin_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrderView {
    id: i64,
    number: String,
    status: i32,
    pay_status: i8,
    refund_state: Option<String>,
    pay_method: i32,
    amount: f64,
    consignee: String,
    phone: String,
    address: String,
    remark: String,
    order_time: String,
    checkout_time: Option<String>,
    delivery_time: Option<String>,
    estimated_delivery_time: Option<String>,
    cancel_reason: String,
    rejection_reason: String,
    pack_amount: i32,
    order_dishes: String,
    order_detail_list: Vec<OrderItem>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct OrderItem {
    name: String,
    image: String,
    dish_id: Option<i64>,
    setmeal_id: Option<i64>,
    dish_flavor: String,
    number: i32,
    amount: f64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct OrderPage {
    total: i64,
    records: Vec<OrderView>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Counts {
    to_be_confirmed: i64,
    confirmed: i64,
    delivery_in_progress: i64,
}

const ORDER_COLUMNS: &str = "SELECT o.id, CONVERT(COALESCE(o.number,'') USING utf8mb4) AS number,
    o.status,o.pay_status,o.pay_method,CAST(o.amount AS DOUBLE) AS amount,
    CONVERT(o.wx_refund_state USING utf8mb4) AS refund_state,
    CONVERT(COALESCE(o.consignee,'') USING utf8mb4) AS consignee,CONVERT(COALESCE(o.phone,'') USING utf8mb4) AS phone,
    CONVERT(COALESCE(o.address,'') USING utf8mb4) AS address,CONVERT(COALESCE(o.remark,'') USING utf8mb4) AS remark,
    DATE_FORMAT(DATE_ADD(o.order_time, INTERVAL 8 HOUR), '%Y-%m-%d %H:%i:%s') AS order_time,
    DATE_FORMAT(DATE_ADD(o.checkout_time, INTERVAL 8 HOUR), '%Y-%m-%d %H:%i:%s') AS checkout_time,
    DATE_FORMAT(DATE_ADD(o.delivery_time, INTERVAL 8 HOUR), '%Y-%m-%d %H:%i:%s') AS delivery_time,
    DATE_FORMAT(DATE_ADD(o.estimated_delivery_time, INTERVAL 8 HOUR), '%Y-%m-%d %H:%i:%s') AS estimated_delivery_time,
    CONVERT(COALESCE(o.cancel_reason,'') USING utf8mb4) AS cancel_reason,
    CONVERT(COALESCE(o.rejection_reason,'') USING utf8mb4) AS rejection_reason,
    CAST(COALESCE(o.pack_amount,0) AS SIGNED) AS pack_amount";

fn order(row: MySqlRow) -> Result<OrderView, sqlx::Error> {
    Ok(OrderView {
        id: row.try_get("id")?,
        number: row.try_get("number")?,
        status: row.try_get("status")?,
        pay_status: row.try_get("pay_status")?,
        refund_state: row.try_get("refund_state")?,
        pay_method: row.try_get("pay_method")?,
        amount: row.try_get("amount")?,
        consignee: row.try_get("consignee")?,
        phone: row.try_get("phone")?,
        address: row.try_get("address")?,
        remark: row.try_get("remark")?,
        order_time: row.try_get("order_time")?,
        checkout_time: row.try_get("checkout_time")?,
        delivery_time: row.try_get("delivery_time")?,
        estimated_delivery_time: row.try_get("estimated_delivery_time")?,
        cancel_reason: row.try_get("cancel_reason")?,
        rejection_reason: row.try_get("rejection_reason")?,
        pack_amount: row.try_get::<i64, _>("pack_amount")? as i32,
        order_dishes: String::new(),
        order_detail_list: Vec::new(),
    })
}

fn filter<'a>(query: &mut QueryBuilder<'a, MySql>, user: Option<i64>, filter: &'a OrderFilter) {
    query.push(" FROM orders o WHERE 1=1");
    if let Some(user) = user {
        query.push(" AND o.user_id=").push_bind(user);
    }
    if let Some(status) = filter.status.filter(|status| *status > 0) {
        query.push(" AND o.status=").push_bind(status);
    }
    if let Some(number) = filter.number.as_deref().filter(|value| !value.is_empty()) {
        query
            .push(" AND INSTR(o.number,")
            .push_bind(number)
            .push(")>0");
    }
    if let Some(phone) = filter.phone.as_deref().filter(|value| !value.is_empty()) {
        query
            .push(" AND INSTR(o.phone,")
            .push_bind(phone)
            .push(")>0");
    }
    if let Some(begin) = filter
        .begin_time
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        query
            .push(" AND o.order_time>=DATE_SUB(")
            .push_bind(begin)
            .push(", INTERVAL 8 HOUR)");
    }
    if let Some(end) = filter.end_time.as_deref().filter(|value| !value.is_empty()) {
        query
            .push(" AND o.order_time<=DATE_SUB(")
            .push_bind(end)
            .push(", INTERVAL 8 HOUR)");
    }
}

async fn page(
    pool: &MySqlPool,
    user: Option<i64>,
    criteria: OrderFilter,
) -> Result<OrderPage, sqlx::Error> {
    let size = u64::from(criteria.page_size.unwrap_or(10).clamp(1, 100));
    let offset = u64::from(criteria.page.unwrap_or(1).saturating_sub(1)) * size;
    let mut count = QueryBuilder::new("SELECT COUNT(*)");
    filter(&mut count, user, &criteria);
    let total = count.build_query_scalar().fetch_one(pool).await?;
    let mut sql = QueryBuilder::new(ORDER_COLUMNS);
    filter(&mut sql, user, &criteria);
    sql.push(" ORDER BY o.id DESC LIMIT ")
        .push_bind(size)
        .push(" OFFSET ")
        .push_bind(offset);
    let rows = sql.build().fetch_all(pool).await?;
    let mut records = rows.into_iter().map(order).collect::<Result<Vec<_>, _>>()?;
    let ids = records.iter().map(|order| order.id).collect::<Vec<_>>();
    attach_items(pool, &mut records, &ids).await?;
    Ok(OrderPage { total, records })
}

async fn details(
    pool: &MySqlPool,
    user: Option<i64>,
    id: i64,
) -> Result<Option<OrderView>, sqlx::Error> {
    let mut sql = QueryBuilder::<MySql>::new(ORDER_COLUMNS);
    sql.push(" FROM orders o WHERE o.id=").push_bind(id);
    if let Some(user) = user {
        sql.push(" AND o.user_id=").push_bind(user);
    }
    let Some(row) = sql.build().fetch_optional(pool).await? else {
        return Ok(None);
    };
    let mut record = order(row)?;
    attach_items(pool, std::slice::from_mut(&mut record), &[id]).await?;
    Ok(Some(record))
}

async fn attach_items(
    pool: &MySqlPool,
    orders: &mut [OrderView],
    ids: &[i64],
) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        return Ok(());
    }
    type ItemRow = (
        i64,
        String,
        String,
        Option<i64>,
        Option<i64>,
        String,
        i32,
        f64,
    );
    let mut sql = QueryBuilder::<MySql>::new("SELECT order_id,CONVERT(COALESCE(name,'') USING utf8mb4),
        CONVERT(COALESCE(image,'') USING utf8mb4),dish_id,setmeal_id,
        CONVERT(COALESCE(dish_flavor,'') USING utf8mb4),number,CAST(amount AS DOUBLE) FROM order_detail WHERE order_id IN (");
    let mut separated = sql.separated(",");
    for id in ids {
        separated.push_bind(*id);
    }
    sql.push(") ORDER BY id");
    let rows = sql.build_query_as::<ItemRow>().fetch_all(pool).await?;
    let mut items = std::collections::HashMap::<i64, Vec<OrderItem>>::new();
    for (order_id, name, image, dish_id, setmeal_id, dish_flavor, number, amount) in rows {
        items.entry(order_id).or_default().push(OrderItem {
            name,
            image,
            dish_id,
            setmeal_id,
            dish_flavor,
            number,
            amount,
        });
    }
    for order in orders {
        order.order_detail_list = items.remove(&order.id).unwrap_or_default();
        order.order_dishes = order
            .order_detail_list
            .iter()
            .map(|item| format!("{}*{}", item.name, item.number))
            .collect::<Vec<_>>()
            .join("；");
    }
    Ok(())
}

#[utoipa::path(get, path="/api/order/conditionSearch", tag="订单", summary="管理端按条件分页查订单", params(OrderFilter),
    security(("EmployeeToken"=[])), responses((status=200, description="订单分页。", body=ApiResult<OrderPage>)))]
pub(crate) async fn admin_page(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(criteria): Query<OrderFilter>,
) -> Reply<OrderPage> {
    match admin_database(&state, actor).await {
        Ok(pool) => respond(page(pool, None, criteria).await),
        Err(status) => denied(status),
    }
}

#[utoipa::path(get, path="/api/order/details/{id}", tag="订单", summary="管理端查看订单详情", params(("id"=i64,Path)),
    security(("EmployeeToken"=[])), responses((status=200, description="订单详情。", body=ApiResult<OrderView>)))]
pub(crate) async fn admin_details(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(id): Path<i64>,
) -> Reply<OrderView> {
    match admin_database(&state, actor).await {
        Ok(pool) => respond_details(details(pool, None, id).await),
        Err(status) => denied(status),
    }
}

#[utoipa::path(get, path="/api/order/statistics", tag="订单", summary="查询待接单、待派送和派送中数量",
    security(("EmployeeToken"=[])), responses((status=200, description="订单状态数量。", body=ApiResult<Counts>)))]
pub(crate) async fn counts(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
) -> Reply<Counts> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    respond(sqlx::query_as::<_, (i64,i64,i64)>("SELECT COUNT(CASE WHEN status=2 THEN 1 END),
        COUNT(CASE WHEN status=3 THEN 1 END),COUNT(CASE WHEN status=4 THEN 1 END) FROM orders WHERE status IN (2,3,4)")
        .fetch_one(pool).await.map(|(to_be_confirmed,confirmed,delivery_in_progress)| Counts { to_be_confirmed,confirmed,delivery_in_progress }))
}

#[utoipa::path(get, path="/user/order/historyOrders", tag="订单", summary="顾客查询自己的订单", params(OrderFilter),
    security(("CustomerToken"=[])), responses((status=200, description="订单分页。", body=ApiResult<OrderPage>)))]
pub(crate) async fn history(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Query(criteria): Query<OrderFilter>,
) -> Reply<OrderPage> {
    let Some(service) = state.customers else {
        return unavailable();
    };
    respond(page(&service.pool, Some(actor.0), criteria).await)
}

#[utoipa::path(get, path="/user/order/orderDetail/{id}", tag="订单", summary="顾客查看自己的订单详情", params(("id"=i64,Path)),
    security(("CustomerToken"=[])), responses((status=200, description="订单详情。", body=ApiResult<OrderView>)))]
pub(crate) async fn customer_details(
    State(state): State<OrderState>,
    Extension(actor): Extension<CustomerIdentity>,
    Path(id): Path<i64>,
) -> Reply<OrderView> {
    let Some(service) = state.customers else {
        return unavailable();
    };
    respond_details(details(&service.pool, Some(actor.0), id).await)
}

fn respond_details(result: Result<Option<OrderView>, sqlx::Error>) -> Reply<OrderView> {
    match result {
        Ok(Some(order)) => respond(Ok(order)),
        Ok(None) => fail(StatusCode::OK, "订单不存在"),
        Err(error) => respond(Err(error)),
    }
}

pub(super) fn respond<T>(result: Result<T, sqlx::Error>) -> Reply<T> {
    match result {
        Ok(data) => (StatusCode::OK, Json(ApiResult::success(data))),
        Err(error) => {
            eprintln!("Order request failed: {error}");
            unavailable()
        }
    }
}

pub(super) fn unavailable<T>() -> Reply<T> {
    fail(StatusCode::SERVICE_UNAVAILABLE, "订单服务暂不可用")
}

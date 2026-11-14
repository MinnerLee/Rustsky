use crate::{
    common::ApiResult,
    customer::Reply,
    employee::{
        EmployeeIdentity, EmployeeState, authenticate_employee,
        manage::{admin_database, denied},
    },
};
use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{Days, FixedOffset, NaiveDate, Utc};
use rust_xlsxwriter::{Format, Workbook, XlsxError};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::collections::BTreeMap;
use utoipa::{IntoParams, ToSchema};

pub fn routes(state: EmployeeState) -> Router {
    Router::new()
        .route("/turnoverStatistics", get(turnover))
        .route("/userStatistics", get(users))
        .route("/ordersStatistics", get(orders))
        .route("/top10", get(top))
        .route("/dataOverView", get(overview))
        .route("/export", get(export))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            authenticate_employee,
        ))
        .with_state(state)
}

pub fn workspace_routes(state: EmployeeState) -> Router {
    Router::new()
        .route("/businessData", get(business))
        .route("/overviewOrders", get(workspace_orders))
        .route("/overviewDishes", get(workspace_dishes))
        .route("/overviewSetmeals", get(workspace_setmeals))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            authenticate_employee,
        ))
        .with_state(state)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct DateFilter {
    begin: Option<String>,
    end: Option<String>,
}

struct DateRange {
    dates: Vec<String>,
    begin: String,
    end_exclusive: String,
}

impl DateRange {
    fn new(begin: NaiveDate, end: NaiveDate) -> Option<Self> {
        let span = end.signed_duration_since(begin).num_days();
        if !(0..366).contains(&span) || begin < NaiveDate::from_ymd_opt(2000, 1, 1)? {
            return None;
        }
        let dates = begin
            .iter_days()
            .take(span as usize + 1)
            .map(|day| day.to_string())
            .collect();
        Some(Self {
            dates,
            begin: begin.to_string(),
            end_exclusive: end.succ_opt()?.to_string(),
        })
    }

    fn from_filter(filter: DateFilter, default_days: u64) -> Option<Self> {
        let (begin, end) = match (filter.begin, filter.end) {
            (None, None) => {
                let today = today();
                (
                    today.checked_sub_days(Days::new(default_days))?,
                    today.pred_opt()?,
                )
            }
            (Some(begin), Some(end)) if begin.len() == 10 && end.len() == 10 => (
                NaiveDate::parse_from_str(&begin, "%Y-%m-%d").ok()?,
                NaiveDate::parse_from_str(&end, "%Y-%m-%d").ok()?,
            ),
            _ => return None,
        };
        Self::new(begin, end)
    }
}

fn today() -> NaiveDate {
    Utc::now()
        .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
        .date_naive()
}

#[derive(Clone, Copy, Default)]
struct OrderDay {
    order_count: i64,
    valid_count: i64,
    turnover_cents: i64,
}

async fn order_days(pool: &MySqlPool, range: &DateRange) -> Result<Vec<OrderDay>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String,i64,i64,i64)>("SELECT DATE_FORMAT(DATE_ADD(order_time, INTERVAL 8 HOUR), '%Y-%m-%d'),
        COUNT(*),COUNT(CASE WHEN status=5 AND pay_status=1 THEN 1 END),
        CAST(COALESCE(SUM(CASE WHEN status=5 AND pay_status=1 THEN amount ELSE 0 END),0)*100 AS SIGNED)
        FROM orders WHERE order_time>=DATE_SUB(?, INTERVAL 8 HOUR) AND order_time<DATE_SUB(?, INTERVAL 8 HOUR)
        GROUP BY 1")
        .bind(&range.begin).bind(&range.end_exclusive).fetch_all(pool).await?;
    let mut by_date = rows
        .into_iter()
        .map(|(date, order_count, valid_count, turnover_cents)| {
            (
                date,
                OrderDay {
                    order_count,
                    valid_count,
                    turnover_cents,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    Ok(range
        .dates
        .iter()
        .map(|date| by_date.remove(date).unwrap_or_default())
        .collect())
}

struct UserDay {
    new_count: i64,
    total_count: i64,
}

async fn user_days(pool: &MySqlPool, range: &DateRange) -> Result<Vec<UserDay>, sqlx::Error> {
    let mut total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM `user` WHERE create_time < DATE_SUB(?, INTERVAL 8 HOUR)",
    )
    .bind(&range.begin)
    .fetch_one(pool)
    .await?;
    let mut by_date = sqlx::query_as::<_,(String,i64)>("SELECT DATE_FORMAT(DATE_ADD(create_time, INTERVAL 8 HOUR), '%Y-%m-%d'), COUNT(*)
        FROM `user` WHERE create_time>=DATE_SUB(?, INTERVAL 8 HOUR) AND create_time<DATE_SUB(?, INTERVAL 8 HOUR)
        GROUP BY 1")
        .bind(&range.begin).bind(&range.end_exclusive).fetch_all(pool).await?
        .into_iter().collect::<BTreeMap<_,_>>();
    Ok(range
        .dates
        .iter()
        .map(|date| {
            let new_count = by_date.remove(date).unwrap_or(0);
            total += new_count;
            UserDay {
                new_count,
                total_count: total,
            }
        })
        .collect())
}

fn csv(values: impl IntoIterator<Item = impl ToString>) -> String {
    values
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn money(cents: i64) -> f64 {
    cents as f64 / 100.0
}

fn ratio(numerator: i64, denominator: i64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Turnover {
    date_list: String,
    turnover_list: String,
}

#[utoipa::path(get, path="/api/report/turnoverStatistics", tag="统计", summary="查询每日营业额", params(DateFilter),
    security(("EmployeeToken"=[])), responses((status=200, description="日期和营业额序列。", body=ApiResult<Turnover>)))]
pub(crate) async fn turnover(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(filter): Query<DateFilter>,
) -> Reply<Turnover> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let Some(range) = DateRange::from_filter(filter, 7) else {
        return invalid();
    };
    reply(order_days(pool, &range).await.map(|days| {
        Turnover {
            date_list: csv(&range.dates),
            turnover_list: csv(days
                .iter()
                .map(|day| format!("{:.2}", money(day.turnover_cents)))),
        }
    }))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Users {
    date_list: String,
    total_user_list: String,
    new_user_list: String,
}

#[utoipa::path(get, path="/api/report/userStatistics", tag="统计", summary="查询每日新增用户和累计用户", params(DateFilter),
    security(("EmployeeToken"=[])), responses((status=200, description="日期和用户序列。", body=ApiResult<Users>)))]
pub(crate) async fn users(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(filter): Query<DateFilter>,
) -> Reply<Users> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let Some(range) = DateRange::from_filter(filter, 7) else {
        return invalid();
    };
    reply(user_days(pool, &range).await.map(|days| Users {
        date_list: csv(&range.dates),
        total_user_list: csv(days.iter().map(|day| day.total_count)),
        new_user_list: csv(days.iter().map(|day| day.new_count)),
    }))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Orders {
    date_list: String,
    order_count_list: String,
    valid_order_count_list: String,
    total_order_count: i64,
    valid_order_count: i64,
    order_completion_rate: f64,
}

#[utoipa::path(get, path="/api/report/ordersStatistics", tag="统计", summary="查询订单数和完成率", params(DateFilter),
    security(("EmployeeToken"=[])), responses((status=200, description="日期、订单数、有效订单数和完成率。", body=ApiResult<Orders>)))]
pub(crate) async fn orders(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(filter): Query<DateFilter>,
) -> Reply<Orders> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let Some(range) = DateRange::from_filter(filter, 7) else {
        return invalid();
    };
    reply(order_days(pool, &range).await.map(|days| {
        let total = days.iter().map(|day| day.order_count).sum();
        let valid = days.iter().map(|day| day.valid_count).sum();
        Orders {
            date_list: csv(&range.dates),
            order_count_list: csv(days.iter().map(|day| day.order_count)),
            valid_order_count_list: csv(days.iter().map(|day| day.valid_count)),
            total_order_count: total,
            valid_order_count: valid,
            order_completion_rate: ratio(valid, total),
        }
    }))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Top {
    name_list: String,
    number_list: String,
    names: Vec<String>,
    quantities: Vec<i64>,
}

#[utoipa::path(get, path="/api/report/top10", tag="统计", summary="查询已完成订单中的销量前十", params(DateFilter),
    security(("EmployeeToken"=[])), responses((status=200, description="按销量排序的商品。", body=ApiResult<Top>)))]
pub(crate) async fn top(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(filter): Query<DateFilter>,
) -> Reply<Top> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let Some(range) = DateRange::from_filter(filter, 7) else {
        return invalid();
    };
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT CONVERT(od.name USING utf8mb4),CAST(SUM(od.number) AS SIGNED) AS sold
        FROM order_detail od JOIN orders o ON o.id=od.order_id
        WHERE o.status=5 AND o.pay_status=1 AND od.name IS NOT NULL
        AND o.order_time>=DATE_SUB(?, INTERVAL 8 HOUR) AND o.order_time<DATE_SUB(?, INTERVAL 8 HOUR)
        GROUP BY od.name ORDER BY sold DESC,od.name LIMIT 10",
    )
    .bind(&range.begin)
    .bind(&range.end_exclusive)
    .fetch_all(pool)
    .await;
    reply(rows.map(|rows| {
        let (names, quantities): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        Top {
            name_list: csv(&names),
            number_list: csv(&quantities),
            names,
            quantities,
        }
    }))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Overview {
    turnover: f64,
    valid_order_count: i64,
    order_completion_rate: f64,
    unit_price: f64,
    new_users: i64,
    total_users: i64,
}

async fn aggregate(pool: &MySqlPool, range: &DateRange) -> Result<Overview, sqlx::Error> {
    let (orders, users) = tokio::try_join!(order_days(pool, range), user_days(pool, range))?;
    let total = orders.iter().map(|day| day.order_count).sum();
    let valid = orders.iter().map(|day| day.valid_count).sum();
    let turnover_cents = orders.iter().map(|day| day.turnover_cents).sum();
    Ok(Overview {
        turnover: money(turnover_cents),
        valid_order_count: valid,
        order_completion_rate: ratio(valid, total),
        unit_price: ratio(turnover_cents, valid) / 100.0,
        new_users: users.iter().map(|day| day.new_count).sum(),
        total_users: users.last().map_or(0, |day| day.total_count),
    })
}

#[utoipa::path(get, path="/api/report/dataOverView", tag="统计", summary="查询指定日期内的经营概览", params(DateFilter),
    security(("EmployeeToken"=[])), responses((status=200, description="经营概览。", body=ApiResult<Overview>)))]
pub(crate) async fn overview(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(filter): Query<DateFilter>,
) -> Reply<Overview> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let Some(range) = DateRange::from_filter(filter, 7) else {
        return invalid();
    };
    reply(aggregate(pool, &range).await)
}

#[utoipa::path(get, path="/api/workspace/businessData", tag="统计", summary="工作台今日经营概览",
    security(("EmployeeToken"=[])), responses((status=200, description="经营概览。", body=ApiResult<Overview>)))]
pub(crate) async fn business(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
) -> Reply<Overview> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let now = today();
    let range = DateRange::new(now, now).expect("current date is supported");
    reply(aggregate(pool, &range).await)
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceOrders {
    waiting_orders: i64,
    delivered_orders: i64,
    completed_orders: i64,
    cancelled_orders: i64,
    all_orders: i64,
}

#[utoipa::path(get, path="/api/workspace/overviewOrders", tag="统计", summary="工作台今日订单概览",
    security(("EmployeeToken"=[])), responses((status=200, description="今日各状态订单数。", body=ApiResult<WorkspaceOrders>)))]
pub(crate) async fn workspace_orders(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
) -> Reply<WorkspaceOrders> {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let now = today();
    let range = DateRange::new(now, now).expect("current date is supported");
    reply(sqlx::query_as::<_,(i64,i64,i64,i64,i64)>("SELECT COUNT(CASE WHEN status=2 THEN 1 END),
        COUNT(CASE WHEN status IN (3,4) THEN 1 END),COUNT(CASE WHEN status=5 THEN 1 END),COUNT(CASE WHEN status=6 THEN 1 END),COUNT(*)
        FROM orders WHERE order_time>=DATE_SUB(?, INTERVAL 8 HOUR) AND order_time<DATE_SUB(?, INTERVAL 8 HOUR)")
        .bind(&range.begin).bind(&range.end_exclusive).fetch_one(pool).await.map(|(waiting_orders,delivered_orders,completed_orders,cancelled_orders,all_orders)|
            WorkspaceOrders { waiting_orders,delivered_orders,completed_orders,cancelled_orders,all_orders }))
}

#[derive(Serialize, ToSchema)]
pub(crate) struct CatalogCounts {
    sold: i64,
    discontinued: i64,
}

#[utoipa::path(get, path="/api/workspace/overviewDishes", tag="统计", summary="工作台菜品状态概览",
    security(("EmployeeToken"=[])), responses((status=200, description="起售和停售数量。", body=ApiResult<CatalogCounts>)))]
pub(crate) async fn workspace_dishes(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
) -> Reply<CatalogCounts> {
    catalog(&state, actor, "dish").await
}

#[utoipa::path(get, path="/api/workspace/overviewSetmeals", tag="统计", summary="工作台套餐状态概览",
    security(("EmployeeToken"=[])), responses((status=200, description="起售和停售数量。", body=ApiResult<CatalogCounts>)))]
pub(crate) async fn workspace_setmeals(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
) -> Reply<CatalogCounts> {
    catalog(&state, actor, "setmeal").await
}

async fn catalog(
    state: &EmployeeState,
    actor: EmployeeIdentity,
    table: &'static str,
) -> Reply<CatalogCounts> {
    let pool = match admin_database(state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let sql = format!(
        "SELECT COUNT(CASE WHEN status=1 THEN 1 END),COUNT(CASE WHEN status=0 THEN 1 END) FROM {table}"
    );
    reply(
        sqlx::query_as::<_, (i64, i64)>(&sql)
            .fetch_one(pool)
            .await
            .map(|(sold, discontinued)| CatalogCounts { sold, discontinued }),
    )
}

#[utoipa::path(get, path="/api/report/export", tag="统计", summary="导出运营数据 Excel，默认最近 30 个完整自然日", params(DateFilter),
    security(("EmployeeToken"=[])), responses((status=200, description="运营数据 Excel 文件。", content_type="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")))]
pub(crate) async fn export(
    State(state): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Query(filter): Query<DateFilter>,
) -> Response {
    let pool = match admin_database(&state, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied::<()>(status).into_response(),
    };
    let Some(range) = DateRange::from_filter(filter, 30) else {
        return invalid::<()>().into_response();
    };
    let (orders, users) = match tokio::try_join!(order_days(pool, &range), user_days(pool, &range))
    {
        Ok(data) => data,
        Err(error) => return reply::<()>(Err(error)).into_response(),
    };
    match tokio::task::spawn_blocking(move || workbook(range.dates, orders, users)).await {
        Ok(Ok(bytes)) => (
            [
                (
                    header::CONTENT_TYPE,
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                ),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=rustsky-operation-report.xlsx",
                ),
            ],
            bytes,
        )
            .into_response(),
        result => {
            eprintln!("Report export failed: {:?}", result.err());
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResult::<()>::failure("报表生成失败")),
            )
                .into_response()
        }
    }
}

fn workbook(
    dates: Vec<String>,
    orders: Vec<OrderDay>,
    users: Vec<UserDay>,
) -> Result<Vec<u8>, XlsxError> {
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    let heading = Format::new().set_bold();
    let currency = Format::new().set_num_format("0.00");
    let percentage = Format::new().set_num_format("0.00%");
    sheet.set_name("每日运营数据")?;
    sheet.set_column_width(0, 14)?;
    for (col, label) in [
        "日期",
        "营业额（元）",
        "全部订单数",
        "有效订单数",
        "订单完成率",
        "新增用户",
        "累计用户",
    ]
    .into_iter()
    .enumerate()
    {
        sheet.write_string_with_format(0, col as u16, label, &heading)?;
        if col > 0 {
            sheet.set_column_width(col as u16, 17)?;
        }
    }
    for (i, ((date, order), user)) in dates.into_iter().zip(orders).zip(users).enumerate() {
        let row = i as u32 + 1;
        sheet.write_string(row, 0, date)?;
        sheet.write_number_with_format(row, 1, money(order.turnover_cents), &currency)?;
        sheet.write_number(row, 2, order.order_count as f64)?;
        sheet.write_number(row, 3, order.valid_count as f64)?;
        sheet.write_number_with_format(
            row,
            4,
            ratio(order.valid_count, order.order_count),
            &percentage,
        )?;
        sheet.write_number(row, 5, user.new_count as f64)?;
        sheet.write_number(row, 6, user.total_count as f64)?;
    }
    sheet.set_freeze_panes(1, 0)?;
    book.save_to_buffer()
}

fn reply<T>(result: Result<T, sqlx::Error>) -> Reply<T> {
    match result {
        Ok(value) => (StatusCode::OK, Json(ApiResult::success(value))),
        Err(error) => {
            eprintln!("Report query failed: {error}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiResult::failure("统计数据暂不可用")),
            )
        }
    }
}

fn invalid<T>() -> Reply<T> {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResult::failure("请同时选择起止日期，最多查询 366 天")),
    )
}

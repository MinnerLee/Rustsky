use super::{Accounts, EmployeeIdentity, LoginService};
use crate::common::{ApiResult, NoData};
use axum::{
    Extension, Json,
    extract::{Query, State, rejection::QueryRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

#[derive(Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub(crate) struct EmployeePageQuery {
    /// 从 1 开始。省略时取第 1 页。
    #[param(minimum = 1, default = 1)]
    page: Option<u32>,
    /// 每页 1 至 100 条，默认 10 条。
    #[param(minimum = 1, maximum = 100, default = 10)]
    page_size: Option<u32>,
    /// 按姓名包含的文字筛选，留空时查询全部。
    #[param(max_length = 32, example = "张")]
    name: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct EmployeeListRow {
    id: i64,
    name: String,
    username: String,
    /// 旧员工尚未分配工号时为 null。
    employee_no: Option<String>,
    phone: String,
    /// 1 表示启用，0 表示禁用。
    status: i32,
    /// 格式为 YYYY-MM-DD HH:mm:ss，缺少时间时为 null。
    #[schema(example = "2026-11-11 09:30:00")]
    update_time: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct EmployeePage {
    total: u64,
    records: Vec<EmployeeListRow>,
}

#[utoipa::path(
    get,
    path = "/api/employee/page",
    tag = "员工",
    operation_id = "listEmployees",
    summary = "员工分页查询",
    description = "启用的管理员可按姓名搜索员工，结果按最近修改时间和员工 ID 降序排列。",
    params(EmployeePageQuery),
    security(("EmployeeToken" = [])),
    responses(
        (status = 200, description = "返回匹配总数及当前页记录。", body = ApiResult<EmployeePage>),
        (status = 400, description = "分页参数不正确。", body = ApiResult<NoData>),
        (status = 401, description = "令牌缺失、无效或过期。", body = ApiResult<NoData>),
        (status = 403, description = "当前账号无权查询员工。", body = ApiResult<NoData>),
        (status = 503, description = "数据库暂不可用。", body = ApiResult<NoData>)
    )
)]
pub(crate) async fn list_employees(
    State(service): State<Option<Arc<LoginService>>>,
    Extension(actor): Extension<EmployeeIdentity>,
    query: Result<Query<EmployeePageQuery>, QueryRejection>,
) -> (StatusCode, Json<ApiResult<EmployeePage>>) {
    let failure = |status, message| (status, Json(ApiResult::failure(message)));
    let Ok(Query(query)) = query else {
        return failure(StatusCode::BAD_REQUEST, "分页参数格式错误");
    };
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(10);
    let name = query.name.as_deref().unwrap_or("").trim();
    if page == 0 || !(1..=100).contains(&page_size) || name.chars().count() > 32 {
        return failure(
            StatusCode::BAD_REQUEST,
            "页码须从 1 开始，每页 1 至 100 条，姓名最多 32 个字符",
        );
    }
    let Some(Accounts::Database(pool)) = service.as_deref().map(|service| &service.accounts) else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "查询员工需要连接数据库");
    };

    match load_page(pool, actor, page, page_size, name).await {
        Ok(Some(page)) => (StatusCode::OK, Json(ApiResult::success(page))),
        Ok(None) => failure(StatusCode::FORBIDDEN, "当前账号无权查询员工"),
        Err(error) => {
            eprintln!("Employee page query failed: {error}");
            failure(StatusCode::SERVICE_UNAVAILABLE, "暂时无法查询员工")
        }
    }
}

async fn load_page(
    pool: &MySqlPool,
    actor: EmployeeIdentity,
    page: u32,
    page_size: u32,
    name: &str,
) -> Result<Option<EmployeePage>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let authorized = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM employee WHERE id = ? AND username = 'admin' AND status = 1",
    )
    .bind(actor.0)
    .fetch_optional(&mut *transaction)
    .await?;
    if authorized.is_none() {
        return Ok(None);
    }

    let total = sqlx::query_scalar::<_, u64>(
        "SELECT CAST(COUNT(*) AS UNSIGNED) FROM employee WHERE ? = '' OR LOCATE(?, name) > 0",
    )
    .bind(name)
    .bind(name)
    .fetch_one(&mut *transaction)
    .await?;

    let offset = u64::from(page - 1) * u64::from(page_size);
    type EmployeeRow = (
        i64,
        String,
        String,
        Option<String>,
        String,
        i32,
        Option<String>,
    );
    let rows = sqlx::query_as::<_, EmployeeRow>(
        "SELECT id, CONVERT(name USING utf8mb4), CONVERT(username USING utf8mb4),
                CONVERT(employee_no USING utf8mb4), CONVERT(phone USING utf8mb4),
                status, DATE_FORMAT(update_time, '%Y-%m-%d %H:%i:%s')
         FROM employee WHERE ? = '' OR LOCATE(?, name) > 0
         ORDER BY update_time DESC, id DESC LIMIT ? OFFSET ?",
    )
    .bind(name)
    .bind(name)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let records = rows
        .into_iter()
        .map(
            |(id, name, username, employee_no, phone, status, update_time)| EmployeeListRow {
                id,
                name,
                username,
                employee_no,
                phone,
                status,
                update_time,
            },
        )
        .collect();
    Ok(Some(EmployeePage { total, records }))
}

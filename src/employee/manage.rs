use super::{EmployeeIdentity, EmployeeState, LoginService};
use crate::common::{ApiResult, NoData};
use axum::{
    Extension, Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use utoipa::ToSchema;

pub(crate) async fn is_admin(
    pool: &MySqlPool,
    actor: EmployeeIdentity,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT id FROM employee WHERE id = ? AND username = 'admin' AND status = 1",
    )
    .bind(actor.0)
    .fetch_optional(pool)
    .await?
    .is_some())
}

pub(crate) async fn admin_database(
    service: &EmployeeState,
    actor: EmployeeIdentity,
) -> Result<&MySqlPool, StatusCode> {
    let pool = service
        .as_deref()
        .and_then(LoginService::database)
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    match is_admin(pool, actor).await {
        Ok(true) => Ok(pool),
        Ok(false) => Err(StatusCode::FORBIDDEN),
        Err(error) => {
            eprintln!("Administrator lookup failed: {error}");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

pub(crate) fn denied<T>(status: StatusCode) -> (StatusCode, Json<ApiResult<T>>) {
    let message = if status == StatusCode::FORBIDDEN {
        "当前账号没有管理权限"
    } else {
        "数据库暂不可用"
    };
    (status, Json(ApiResult::failure(message)))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EmployeeDetails {
    id: i64,
    username: String,
    name: String,
    employee_no: Option<String>,
    phone: String,
    sex: String,
    id_number: String,
    status: i32,
}

#[utoipa::path(get, path = "/api/employee/{id}", tag = "员工", summary = "查询员工资料",
    params(("id" = i64, Path, description = "员工 ID")), security(("EmployeeToken" = [])),
    responses((status = 200, description = "员工资料，或员工不存在。", body = ApiResult<EmployeeDetails>),
    (status = 401, description = "令牌无效。"), (status = 403, description = "需要管理员。")))]
pub(crate) async fn employee_details(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(id): Path<i64>,
) -> (StatusCode, Json<ApiResult<EmployeeDetails>>) {
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    type Row = (
        i64,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        i32,
    );
    match sqlx::query_as::<_, Row>(
        "SELECT id, CONVERT(username USING utf8mb4), CONVERT(name USING utf8mb4), CONVERT(employee_no USING utf8mb4),
                CONVERT(phone USING utf8mb4), CONVERT(sex USING utf8mb4), CONVERT(id_number USING utf8mb4), status
         FROM employee WHERE id = ?",
    ).bind(id).fetch_optional(pool).await {
        Ok(Some((id, username, name, employee_no, phone, sex, id_number, status))) => (StatusCode::OK, Json(ApiResult::success(EmployeeDetails { id, username, name, employee_no, phone, sex, id_number, status }))),
        Ok(None) => (StatusCode::OK, Json(ApiResult::failure("员工不存在"))),
        Err(error) => { eprintln!("Employee detail query failed: {error}"); denied(StatusCode::SERVICE_UNAVAILABLE) }
    }
}

#[derive(Deserialize)]
pub(crate) struct EmployeeIdQuery {
    id: i64,
}

#[utoipa::path(post, path = "/api/employee/status/{status}", tag = "员工", summary = "启用或禁用员工",
    params(("status" = i32, Path, description = "0 禁用，1 启用"), ("id" = i64, Query, description = "员工 ID")),
    security(("EmployeeToken" = [])), responses((status = 200, description = "状态保存结果。", body = ApiResult<NoData>),
    (status = 400, description = "状态或 ID 不正确。"), (status = 401, description = "令牌无效。"), (status = 403, description = "需要管理员。")))]
pub(crate) async fn set_employee_status(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    Path(status): Path<i32>,
    Query(target): Query<EmployeeIdQuery>,
) -> (StatusCode, Json<ApiResult<NoData>>) {
    if !matches!(status, 0 | 1) || target.id <= 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResult::failure("员工 ID 或状态不正确")),
        );
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result = sqlx::query("UPDATE employee SET status = ?, update_time = UTC_TIMESTAMP(), update_user = ? WHERE id = ? AND username <> 'admin'")
        .bind(status).bind(actor.0).bind(target.id).execute(pool).await;
    mutation_result(result)
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditEmployeeRequest {
    id: i64,
    username: String,
    name: String,
    phone: String,
    sex: String,
    id_number: String,
    employee_no: Option<String>,
}

#[utoipa::path(put, path = "/api/employee", tag = "员工", summary = "修改员工资料",
    request_body = EditEmployeeRequest, security(("EmployeeToken" = [])), responses(
    (status = 200, description = "资料保存结果。", body = ApiResult<NoData>),
    (status = 400, description = "资料格式不正确。"), (status = 401, description = "令牌无效。"), (status = 403, description = "需要管理员。")))]
pub(crate) async fn edit_employee(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    request: Result<Json<EditEmployeeRequest>, JsonRejection>,
) -> (StatusCode, Json<ApiResult<NoData>>) {
    let Ok(Json(request)) = request else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResult::failure("员工资料格式错误")),
        );
    };
    let username = request.username.trim();
    let name = request.name.trim();
    let phone = request.phone.trim().as_bytes();
    let id_number = request.id_number.trim().as_bytes();
    let valid_id = (id_number.len() == 15 && id_number.iter().all(u8::is_ascii_digit))
        || (id_number.len() == 18
            && id_number[..17].iter().all(u8::is_ascii_digit)
            && (id_number[17].is_ascii_digit() || matches!(id_number[17], b'X' | b'x')));
    if request.id <= 0
        || !(3..=20).contains(&username.len())
        || !username
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        || !(1..=12).contains(&name.chars().count())
        || phone.len() != 11
        || phone[0] != b'1'
        || !(b'3'..=b'9').contains(&phone[1])
        || !phone.iter().all(u8::is_ascii_digit)
        || !matches!(request.sex.as_str(), "0" | "1")
        || !valid_id
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResult::failure(
                "请检查账号、姓名、手机号、性别和身份证号",
            )),
        );
    }
    let employee_no = request
        .employee_no
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if employee_no.is_some_and(|number| {
        number.len() > 32
            || !number.as_bytes()[0].is_ascii_alphanumeric()
            || !number
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    }) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResult::failure("工号格式错误")),
        );
    }
    let pool = match admin_database(&service, actor).await {
        Ok(pool) => pool,
        Err(status) => return denied(status),
    };
    let result = sqlx::query("UPDATE employee SET username = ?, name = ?, phone = ?, sex = ?, id_number = ?,
                              employee_no = COALESCE(?, employee_no), update_time = UTC_TIMESTAMP(), update_user = ?
                              WHERE id = ? AND username <> 'admin'")
        .bind(username).bind(name).bind(request.phone.trim()).bind(request.sex)
        .bind(request.id_number.trim().to_ascii_uppercase()).bind(employee_no.map(str::to_ascii_uppercase))
        .bind(actor.0).bind(request.id).execute(pool).await;
    mutation_result(result)
}

fn mutation_result(
    result: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error>,
) -> (StatusCode, Json<ApiResult<NoData>>) {
    match result {
        Ok(result) if result.rows_affected() == 1 => {
            (StatusCode::OK, Json(ApiResult::success(NoData)))
        }
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResult::failure("员工不存在或管理员账号不可修改")),
        ),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            (StatusCode::OK, Json(ApiResult::failure("账号或工号已存在")))
        }
        Err(_) => {
            eprintln!("Employee update failed");
            denied(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

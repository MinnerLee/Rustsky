use super::{Accounts, EmployeeIdentity, LoginService, hash_password};
use crate::common::{ApiResult, NoData};
use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateEmployeeRequest {
    #[schema(example = "zhangsan", min_length = 3, max_length = 20)]
    username: String,
    #[schema(example = "张三", min_length = 1, max_length = 12)]
    name: String,
    /// 工号保留前导零，英文字母保存为大写。
    #[schema(example = "RS-0007", min_length = 1, max_length = 32)]
    employee_no: String,
    #[schema(example = "13900000000")]
    phone: String,
    /// 0 表示女，1 表示男。
    #[schema(example = "1")]
    sex: String,
    #[schema(example = "000000000000000000")]
    id_number: String,
    /// 由管理员设置，8 至 64 个字符。
    #[schema(format = Password, min_length = 8, max_length = 64)]
    initial_password: String,
}

impl CreateEmployeeRequest {
    fn validate(&self) -> Result<(), &'static str> {
        let username = self.username.trim();
        if !(3..=20).contains(&username.len())
            || !username
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Err("账号须为 3 至 20 位小写字母或数字");
        }
        if !(1..=12).contains(&self.name.trim().chars().count()) {
            return Err("姓名须为 1 至 12 个字符");
        }
        let employee_no = self.employee_no.trim();
        if !(1..=32).contains(&employee_no.len())
            || !employee_no.as_bytes()[0].is_ascii_alphanumeric()
            || !employee_no
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        {
            return Err("工号须以英文字母或数字开头，最多 32 位，可含横线和下划线");
        }
        let phone = self.phone.trim().as_bytes();
        if phone.len() != 11
            || phone[0] != b'1'
            || !(b'3'..=b'9').contains(&phone[1])
            || !phone.iter().all(u8::is_ascii_digit)
        {
            return Err("请填写 11 位手机号");
        }
        if !matches!(self.sex.as_str(), "0" | "1") {
            return Err("请选择性别");
        }
        let id_number = self.id_number.trim().as_bytes();
        if id_number.len() != 18
            || !id_number[..17].iter().all(u8::is_ascii_digit)
            || !(id_number[17].is_ascii_digit() || matches!(id_number[17], b'X' | b'x'))
        {
            return Err("身份证号须为 18 位，最后一位可为 X");
        }
        if !(8..=64).contains(&self.initial_password.chars().count())
            || self.initial_password.trim().is_empty()
        {
            return Err("初始密码须为 8 至 64 个字符");
        }
        Ok(())
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreatedEmployee {
    id: u64,
    employee_no: String,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(untagged)]
enum CreateEmployeeOutcome {
    Success(ApiResult<CreatedEmployee>),
    Duplicate(ApiResult<NoData>),
}

#[utoipa::path(
    post,
    path = "/api/employee",
    tag = "员工",
    operation_id = "createEmployee",
    summary = "新增员工",
    description = "仅启用的管理员 admin 可新增。账号和工号唯一，初始密码以 Argon2id 哈希保存。",
    request_body = CreateEmployeeRequest,
    security(("EmployeeToken" = [])),
    responses(
        (status = 200, description = "code=1 返回员工 ID 和保存的工号。账号或工号冲突时 code=0。", body = CreateEmployeeOutcome),
        (status = 400, description = "请求格式或字段不符合要求。", body = ApiResult<NoData>),
        (status = 401, description = "登录令牌缺失或无效。", body = ApiResult<NoData>),
        (status = 403, description = "当前账号无权新增员工。", body = ApiResult<NoData>),
        (status = 413, description = "请求体超过 8 KiB。", body = ApiResult<NoData>),
        (status = 500, description = "无法生成密码哈希。", body = ApiResult<NoData>),
        (status = 503, description = "数据库暂不可用。", body = ApiResult<NoData>)
    )
)]
pub(crate) async fn create_employee(
    State(service): State<Option<Arc<LoginService>>>,
    Extension(actor): Extension<EmployeeIdentity>,
    request: Result<Json<CreateEmployeeRequest>, JsonRejection>,
) -> (StatusCode, Json<ApiResult<CreatedEmployee>>) {
    let failure = |status, message| (status, Json(ApiResult::failure(message)));
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return failure(StatusCode::PAYLOAD_TOO_LARGE, "新增员工请求过长");
        }
        Err(_) => return failure(StatusCode::BAD_REQUEST, "新增员工请求格式错误"),
    };
    if let Err(message) = request.validate() {
        return failure(StatusCode::BAD_REQUEST, message);
    }
    let Some(Accounts::Database(pool)) = service.as_deref().map(|service| &service.accounts) else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "新增员工需要连接数据库");
    };
    let password = request.initial_password;
    let Ok(Ok(password_hash)) = tokio::task::spawn_blocking(move || hash_password(&password)).await
    else {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "暂时无法保存员工密码");
    };
    let employee_no = request.employee_no.trim().to_ascii_uppercase();

    // Check the administrator and insert in the same statement.
    let inserted = sqlx::query(
        "INSERT INTO employee (username, name, employee_no, password, phone, sex, id_number,
                               status, create_time, update_time, create_user, update_user)
         SELECT ?, ?, ?, ?, ?, ?, ?, 1, UTC_TIMESTAMP(), UTC_TIMESTAMP(), admin.id, admin.id
         FROM employee AS admin WHERE admin.id = ? AND admin.username = 'admin' AND admin.status = 1",
    )
    .bind(request.username.trim())
    .bind(request.name.trim())
    .bind(&employee_no)
    .bind(password_hash)
    .bind(request.phone.trim())
    .bind(&request.sex)
    .bind(request.id_number.trim().to_ascii_uppercase())
    .bind(actor.0)
    .execute(pool)
    .await;

    match inserted {
        Ok(result) if result.rows_affected() == 1 => (
            StatusCode::OK,
            Json(ApiResult::success(CreatedEmployee {
                id: result.last_insert_id(),
                employee_no,
            })),
        ),
        Ok(_) => failure(StatusCode::FORBIDDEN, "当前账号无权新增员工"),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            failure(StatusCode::OK, "账号或工号已存在")
        }
        Err(_) => {
            eprintln!("Employee insert failed");
            failure(StatusCode::SERVICE_UNAVAILABLE, "暂时无法保存员工")
        }
    }
}

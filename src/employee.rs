use crate::common::{ApiResult, NoData};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sqlx::{MySqlPool, mysql::MySqlPoolOptions};
use std::{
    env, io,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub(crate) struct LoginRequest {
    /// 用户名去掉首尾空白后应有 1–32 个字符。
    #[schema(example = "admin", min_length = 1)]
    username: String,
    /// 员工密码。调试时填写自己的本地密码，最多 1024 字节。
    #[schema(format = Password, min_length = 1)]
    password: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    id: u64,
    user_name: String,
    name: String,
    /// 有效期为一小时的员工 JWT。
    token: String,
}

// Both outcomes use HTTP 200 and differ in whether the envelope contains employee data.
#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(untagged)]
enum LoginOutcome {
    Success(ApiResult<LoginResponse>),
    InvalidCredentials(ApiResult<NoData>),
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Claims {
    emp_id: u64,
    exp: u64,
}

pub struct LoginService {
    accounts: Accounts,
    signing_key: EncodingKey,
    verification_key: DecodingKey,
}

pub(crate) type EmployeeState = Option<Arc<LoginService>>;

enum Accounts {
    Demo(String),
    Database(MySqlPool),
}

pub fn hash_dev_password() -> Result<String, Box<dyn std::error::Error>> {
    let password = env::var("RUSTSKY_DEV_PASSWORD")?;
    if password.trim().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Password cannot be blank").into());
    }
    Ok(hash_password(&password)?)
}

fn hash_password(password: &str) -> io::Result<String> {
    let salt = SaltString::generate(&mut rand_core::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| io::Error::other(error.to_string()))
}

impl LoginService {
    pub(crate) fn database(&self) -> Option<&MySqlPool> {
        match &self.accounts {
            Accounts::Database(pool) => Some(pool),
            Accounts::Demo(_) => None,
        }
    }

    pub async fn from_env() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        if let Some(url) = env::var_os("DATABASE_URL") {
            let secret = env::var("RUSTSKY_JWT_SECRET")?;
            if secret.len() < 32 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "JWT secret needs at least 32 bytes",
                )
                .into());
            }
            let url = url
                .into_string()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid DATABASE_URL"))?;
            let pool = MySqlPoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await?;
            return Ok(Some(Self {
                accounts: Accounts::Database(pool),
                signing_key: EncodingKey::from_secret(secret.as_bytes()),
                verification_key: DecodingKey::from_secret(secret.as_bytes()),
            }));
        }
        let password = env::var("RUSTSKY_DEV_PASSWORD");
        let secret = env::var("RUSTSKY_JWT_SECRET");
        match (password, secret) {
            (Err(env::VarError::NotPresent), Err(env::VarError::NotPresent)) => Ok(None),
            (Ok(password), Ok(secret)) if !password.trim().is_empty() && secret.len() >= 32 => {
                Ok(Some(Self {
                    accounts: Accounts::Demo(password),
                    signing_key: EncodingKey::from_secret(secret.as_bytes()),
                    verification_key: DecodingKey::from_secret(secret.as_bytes()),
                }))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Set RUSTSKY_DEV_PASSWORD and RUSTSKY_JWT_SECRET together (JWT secret needs at least 32 bytes)",
            ).into()),
        }
    }

    async fn login(&self, request: LoginRequest) -> Result<LoginResponse, LoginError> {
        let username = request.username.trim();
        let (id, user_name, name) = match &self.accounts {
            Accounts::Demo(password) => {
                if username != "admin"
                    || !bool::from(password.as_bytes().ct_eq(request.password.as_bytes()))
                {
                    return Err(LoginError::InvalidCredentials);
                }
                (1, "admin".into(), "本地管理员".into())
            }
            Accounts::Database(pool) => {
                let (id, user_name, name, password_hash, status) =
                    sqlx::query_as::<_, (i64, String, String, String, i32)>(
                        // The course schema uses binary collations, which SQLx reports as VARBINARY.
                        "SELECT id, CONVERT(username USING utf8mb4), CONVERT(name USING utf8mb4),
                                CONVERT(password USING utf8mb4), status
                         FROM employee WHERE username = ?",
                    )
                    .bind(username)
                    .fetch_optional(pool)
                    .await
                    .map_err(|error| {
                        eprintln!("Employee lookup failed: {error}");
                        LoginError::Unavailable
                    })?
                    .ok_or(LoginError::InvalidCredentials)?;
                if status != 1 {
                    return Err(LoginError::InvalidCredentials);
                }
                let verified = tokio::task::spawn_blocking(move || {
                    PasswordHash::new(&password_hash).is_ok_and(|hash| {
                        Argon2::default()
                            .verify_password(request.password.as_bytes(), &hash)
                            .is_ok()
                    })
                })
                .await
                .map_err(|_| LoginError::Unavailable)?;
                if !verified {
                    return Err(LoginError::InvalidCredentials);
                }
                (
                    u64::try_from(id).map_err(|_| LoginError::Unavailable)?,
                    user_name,
                    name,
                )
            }
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| LoginError::Signing)?;
        let claims = Claims {
            emp_id: id,
            exp: now.as_secs() + 3600,
        };
        let token = encode(&Header::default(), &claims, &self.signing_key)
            .map_err(|_| LoginError::Signing)?;
        Ok(LoginResponse {
            id,
            user_name,
            name,
            token,
        })
    }

    pub(crate) fn employee_id(&self, token: &str) -> Option<i64> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 0;
        let token = decode::<Claims>(token, &self.verification_key, &validation).ok()?;
        i64::try_from(token.claims.emp_id).ok().filter(|id| *id > 0)
    }
}

enum LoginError {
    InvalidCredentials,
    Signing,
    Unavailable,
}

pub fn routes(service: EmployeeState) -> Router {
    let protected = Router::new()
        .route(
            "/",
            post(create::create_employee)
                .put(manage::edit_employee)
                .layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route("/logout", post(logout))
        .route("/page", get(page::list_employees))
        .route("/{id}", get(manage::employee_details))
        .route("/status/{status}", post(manage::set_employee_status))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            authenticate_employee,
        ));

    Router::new()
        .route("/login", post(login).layer(DefaultBodyLimit::max(8 * 1024)))
        .merge(protected)
        .with_state(service)
}

#[derive(Clone, Copy)]
pub(crate) struct EmployeeIdentity(pub(crate) i64);

pub(crate) async fn authenticate_employee(
    State(service): State<EmployeeState>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get("token")
        .and_then(|value| value.to_str().ok());
    if let (Some(service), Some(token)) = (service, token)
        && let Some(id) = service.employee_id(token)
    {
        if let Some(pool) = service.database() {
            let enabled =
                sqlx::query_scalar::<_, i64>("SELECT id FROM employee WHERE id = ? AND status = 1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await;
            match enabled {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(ApiResult::<NoData>::failure("登录已失效，请重新登录")),
                    )
                        .into_response();
                }
                Err(error) => {
                    eprintln!("Employee session lookup failed: {error}");
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(ApiResult::<NoData>::failure("暂时无法验证员工状态")),
                    )
                        .into_response();
                }
            }
        }
        request.extensions_mut().insert(EmployeeIdentity(id));
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiResult::<NoData>::failure("登录已失效，请重新登录")),
    )
        .into_response()
}

#[utoipa::path(
    post,
    path = "/api/employee/login",
    tag = "员工",
    operation_id = "employeeLogin",
    summary = "员工登录",
    request_body = LoginRequest,
    security(()),
    responses(
        (status = 200, description = "code=1 时返回员工信息和 JWT，code=0 时表示凭证无效或账号禁用。", body = LoginOutcome,
            examples(
                ("成功" = (value = json!({"code": 1, "msg": null, "data": {"id": 10, "userName": "admin", "name": "本地管理员", "token": "<jwt>"}}))),
                ("密码错误" = (value = json!({"code": 0, "msg": "用户名或密码错误", "data": null})))
            )),
        (status = 400, description = "请求格式或字段长度错误。", body = ApiResult<NoData>),
        (status = 413, description = "请求体超过 8 KiB。", body = ApiResult<NoData>),
        (status = 500, description = "令牌签发失败。", body = ApiResult<NoData>),
        (status = 503, description = "登录未配置或数据库暂不可用。", body = ApiResult<NoData>)
    )
)]
pub(crate) async fn login(
    State(service): State<Option<Arc<LoginService>>>,
    request: Result<Json<LoginRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => {
            let status = match error.status() {
                StatusCode::PAYLOAD_TOO_LARGE => StatusCode::PAYLOAD_TOO_LARGE,
                _ => StatusCode::BAD_REQUEST,
            };
            return (
                status,
                Json(ApiResult::failure("登录请求格式错误或内容过长")),
            );
        }
    };
    let username = request.username.trim();
    if username.is_empty()
        || username.chars().count() > 32
        || request.password.is_empty()
        || request.password.len() > 1024
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResult::failure("用户名或密码长度不正确")),
        );
    }
    let Some(service) = service else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResult::failure("本地登录未配置")),
        );
    };
    match service.login(request).await {
        Ok(employee) => (StatusCode::OK, Json(ApiResult::success(employee))),
        Err(LoginError::InvalidCredentials) => {
            (StatusCode::OK, Json(ApiResult::failure("用户名或密码错误")))
        }
        Err(LoginError::Signing) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResult::failure("登录暂不可用")),
        ),
        Err(LoginError::Unavailable) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResult::failure("数据库暂不可用")),
        ),
    }
}

#[utoipa::path(
    post,
    path = "/api/employee/logout",
    tag = "员工",
    operation_id = "employeeLogout",
    summary = "员工退出",
    description = "验证令牌后由前端清除本地登录信息。已签发的 JWT 可以使用到有效期结束。",
    security(("EmployeeToken" = [])),
    responses(
        (status = 200, description = "退出成功。", body = ApiResult<NoData>, example = json!({"code": 1, "msg": null, "data": null})),
        (status = 401, description = "令牌缺失、无效或过期。", body = ApiResult<NoData>, example = json!({"code": 0, "msg": "登录已失效，请重新登录", "data": null}))
    )
)]
pub(crate) async fn logout() -> Json<ApiResult<NoData>> {
    Json(ApiResult::success(NoData))
}
pub(crate) mod create;
pub(crate) mod manage;
pub(crate) mod page;

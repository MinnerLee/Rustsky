use crate::{
    common::{ApiResult, NoData},
    employee::{EmployeeState, LoginService},
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::post,
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::{
    env, io,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use utoipa::ToSchema;

pub(crate) type CustomerState = Option<Arc<CustomerService>>;
pub(crate) type Reply<T> = (StatusCode, Json<ApiResult<T>>);
const AUDIENCE: &str = "rustsky:customer";

pub(crate) struct CustomerService {
    pub(crate) pool: MySqlPool,
    auth: LoginProvider,
    signing_key: EncodingKey,
    verification_key: DecodingKey,
}

enum LoginProvider {
    Disabled,
    Dev(String),
    WeChat {
        http: reqwest::Client,
        app_id: String,
        secret: String,
    },
}

impl CustomerService {
    pub(crate) fn dev_login_enabled(&self) -> bool {
        matches!(self.auth, LoginProvider::Dev(_))
    }
}

pub fn from_env(employees: &EmployeeState) -> Result<CustomerState, Box<dyn std::error::Error>> {
    let Some(key) = env::var_os("RUSTSKY_USER_JWT_SECRET") else {
        return Ok(None);
    };
    let key = key
        .into_string()
        .map_err(|_| io::Error::other("Invalid customer JWT secret"))?;
    if key.len() < 32 {
        return Err(io::Error::other("Customer JWT secret must have at least 32 bytes").into());
    }
    let pool = employees
        .as_deref()
        .and_then(LoginService::database)
        .ok_or_else(|| io::Error::other("Customer accounts require MySQL"))?;
    let app_id = env::var("RUSTSKY_WX_APP_ID").ok();
    let secret = env::var("RUSTSKY_WX_APP_SECRET").ok();
    let auth = match (app_id, secret) {
        (Some(app_id), Some(secret)) if !app_id.is_empty() && !secret.is_empty() => {
            LoginProvider::WeChat {
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()?,
                app_id,
                secret,
            }
        }
        (None, None) => env::var("RUSTSKY_DEV_USER_CODE")
            .ok()
            .filter(|code| code.len() >= 16)
            .map_or(LoginProvider::Disabled, LoginProvider::Dev),
        _ => return Err(io::Error::other("Set both WeChat App ID and App Secret").into()),
    };
    Ok(Some(Arc::new(CustomerService {
        pool: pool.clone(),
        auth,
        signing_key: EncodingKey::from_secret(key.as_bytes()),
        verification_key: DecodingKey::from_secret(key.as_bytes()),
    })))
}

pub fn routes(state: CustomerState) -> Router {
    Router::new().route("/login", post(login)).with_state(state)
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CustomerLogin {
    code: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct CustomerSession {
    id: i64,
    openid: String,
    token: String,
}

#[derive(Serialize, Deserialize)]
struct CustomerClaims {
    #[serde(rename = "userId")]
    user_id: i64,
    aud: String,
    exp: u64,
}

#[derive(Deserialize)]
struct WeChatSession {
    openid: Option<String>,
    errcode: Option<i32>,
}

#[utoipa::path(post, path="/user/user/login", tag="用户", summary="微信小程序用户登录", security(()), request_body=CustomerLogin,
    responses((status=200, description="用户标识和令牌。", body=ApiResult<CustomerSession>)))]
pub(crate) async fn login(
    State(state): State<CustomerState>,
    Json(input): Json<CustomerLogin>,
) -> Reply<CustomerSession> {
    let Some(service) = state else {
        return fail(StatusCode::SERVICE_UNAVAILABLE, "用户登录未配置");
    };
    if input.code.is_empty() || input.code.len() > 128 {
        return fail(StatusCode::BAD_REQUEST, "登录凭证格式错误");
    }
    let openid = match &service.auth {
        LoginProvider::Disabled => return fail(StatusCode::SERVICE_UNAVAILABLE, "用户登录未配置"),
        LoginProvider::Dev(code) if bool::from(input.code.as_bytes().ct_eq(code.as_bytes())) => {
            "rustsky-dev-customer".to_string()
        }
        LoginProvider::Dev(_) => return fail(StatusCode::OK, "用户登录凭证无效"),
        LoginProvider::WeChat {
            http,
            app_id,
            secret,
        } => {
            let response = http
                .get("https://api.weixin.qq.com/sns/jscode2session")
                .query(&[
                    ("appid", app_id.as_str()),
                    ("secret", secret.as_str()),
                    ("js_code", input.code.as_str()),
                    ("grant_type", "authorization_code"),
                ])
                .send()
                .await;
            let Ok(response) = response.and_then(reqwest::Response::error_for_status) else {
                return fail(StatusCode::SERVICE_UNAVAILABLE, "微信登录服务暂不可用");
            };
            let Ok(session) = response.json::<WeChatSession>().await else {
                return fail(StatusCode::SERVICE_UNAVAILABLE, "微信登录响应格式错误");
            };
            let Some(openid) = session.openid.filter(|value| {
                !value.is_empty() && value.len() <= 45 && session.errcode.unwrap_or(0) == 0
            }) else {
                return fail(StatusCode::OK, "用户登录凭证无效");
            };
            openid
        }
    };
    let id = match get_or_create(&service.pool, &openid).await {
        Ok(id) => id,
        Err(error) => {
            eprintln!("Customer login lookup failed: {error}");
            return fail(StatusCode::SERVICE_UNAVAILABLE, "用户数据暂不可用");
        }
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "暂时无法签发令牌");
    };
    let claims = CustomerClaims {
        user_id: id,
        aud: AUDIENCE.into(),
        exp: now.as_secs() + 3600,
    };
    match encode(&Header::default(), &claims, &service.signing_key) {
        Ok(token) => (
            StatusCode::OK,
            Json(ApiResult::success(CustomerSession { id, openid, token })),
        ),
        Err(_) => fail(StatusCode::INTERNAL_SERVER_ERROR, "暂时无法签发令牌"),
    }
}

async fn get_or_create(pool: &MySqlPool, openid: &str) -> Result<i64, sqlx::Error> {
    let lookup =
        || sqlx::query_scalar::<_, i64>("SELECT id FROM `user` WHERE openid=?").bind(openid);
    if let Some(id) = lookup().fetch_optional(pool).await? {
        return Ok(id);
    }
    match sqlx::query("INSERT INTO `user` (openid,create_time) VALUES (?,UTC_TIMESTAMP())")
        .bind(openid)
        .execute(pool)
        .await
    {
        Ok(result) => Ok(result.last_insert_id() as i64),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            lookup().fetch_one(pool).await
        }
        Err(error) => Err(error),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CustomerIdentity(pub i64);

pub(crate) async fn authenticate(
    State(state): State<CustomerState>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get("authentication")
        .and_then(|value| value.to_str().ok());
    if let (Some(service), Some(token)) = (state, token) {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 0;
        validation.set_audience(&[AUDIENCE]);
        if let Ok(claims) = decode::<CustomerClaims>(token, &service.verification_key, &validation)
        {
            let id = claims.claims.user_id;
            match sqlx::query_scalar::<_, i64>("SELECT id FROM `user` WHERE id=?")
                .bind(id)
                .fetch_optional(&service.pool)
                .await
            {
                Ok(Some(_)) => {
                    request.extensions_mut().insert(CustomerIdentity(id));
                    return next.run(request).await;
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("Customer session check failed: {error}");
                    return fail::<NoData>(StatusCode::SERVICE_UNAVAILABLE, "用户数据暂不可用")
                        .into_response();
                }
            }
        }
    }
    fail::<NoData>(StatusCode::UNAUTHORIZED, "请先登录用户端").into_response()
}

pub(crate) fn fail<T>(status: StatusCode, message: &'static str) -> Reply<T> {
    (status, Json(ApiResult::failure(message)))
}

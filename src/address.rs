use crate::{
    common::{ApiResult, NoData},
    customer::{CustomerIdentity, CustomerState, Reply, authenticate, fail},
};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub fn routes(state: CustomerState) -> Router {
    Router::new()
        .route("/", post(create).put(edit).delete(remove))
        .route("/list", get(list))
        .route("/default", get(default_address).put(set_default))
        .route("/{id}", get(details))
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state)
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Address {
    id: i64,
    consignee: String,
    sex: String,
    phone: String,
    province_code: String,
    province_name: String,
    city_code: String,
    city_name: String,
    district_code: String,
    district_name: String,
    detail: String,
    label: String,
    is_default: i8,
}

type AddressRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i8,
);
const SELECT: &str = "SELECT id,CONVERT(COALESCE(consignee,'') USING utf8mb4),CONVERT(COALESCE(sex,'') USING utf8mb4),CONVERT(phone USING utf8mb4),
    COALESCE(province_code,''),COALESCE(province_name,''),COALESCE(city_code,''),COALESCE(city_name,''),COALESCE(district_code,''),COALESCE(district_name,''),
    COALESCE(detail,''),COALESCE(label,''),is_default FROM address_book WHERE user_id=?";

fn row(
    (
        id,
        consignee,
        sex,
        phone,
        province_code,
        province_name,
        city_code,
        city_name,
        district_code,
        district_name,
        detail,
        label,
        is_default,
    ): AddressRow,
) -> Address {
    Address {
        id,
        consignee,
        sex,
        phone,
        province_code,
        province_name,
        city_code,
        city_name,
        district_code,
        district_name,
        detail,
        label,
        is_default,
    }
}

#[utoipa::path(get, path="/user/addressBook/list", tag="地址簿", summary="查询本人地址", security(("CustomerToken"=[])),
    responses((status=200, description="地址列表。", body=ApiResult<Vec<Address>>)))]
pub(crate) async fn list(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
) -> Reply<Vec<Address>> {
    let Some(service) = state else {
        return unavailable();
    };
    let sql = format!("{SELECT} ORDER BY is_default DESC,id DESC");
    response(
        sqlx::query_as::<_, AddressRow>(&sql)
            .bind(actor.0)
            .fetch_all(&service.pool)
            .await
            .map(|rows| rows.into_iter().map(row).collect()),
    )
}

#[utoipa::path(get, path="/user/addressBook/{id}", tag="地址簿", summary="查询一条本人地址", security(("CustomerToken"=[])),
    params(("id"=i64,Path)), responses((status=200, description="地址详情。", body=ApiResult<Address>)))]
pub(crate) async fn details(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
    Path(id): Path<i64>,
) -> Reply<Address> {
    let Some(service) = state else {
        return unavailable();
    };
    let sql = format!("{SELECT} AND id=?");
    match sqlx::query_as::<_, AddressRow>(&sql)
        .bind(actor.0)
        .bind(id)
        .fetch_optional(&service.pool)
        .await
    {
        Ok(Some(value)) => response(Ok(row(value))),
        Ok(None) => fail(StatusCode::OK, "地址不存在"),
        Err(error) => response(Err(error)),
    }
}

#[utoipa::path(get, path="/user/addressBook/default", tag="地址簿", summary="查询本人的默认地址", security(("CustomerToken"=[])),
    responses((status=200, description="默认地址。", body=ApiResult<Address>)))]
pub(crate) async fn default_address(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
) -> Reply<Address> {
    let Some(service) = state else {
        return unavailable();
    };
    let sql = format!("{SELECT} AND is_default=1 ORDER BY id DESC LIMIT 1");
    match sqlx::query_as::<_, AddressRow>(&sql)
        .bind(actor.0)
        .fetch_optional(&service.pool)
        .await
    {
        Ok(Some(value)) => response(Ok(row(value))),
        Ok(None) => fail(StatusCode::OK, "尚未设置默认地址"),
        Err(error) => response(Err(error)),
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddressInput {
    id: Option<i64>,
    consignee: String,
    phone: String,
    detail: String,
    sex: Option<String>,
    label: Option<String>,
    province_code: Option<String>,
    province_name: Option<String>,
    city_code: Option<String>,
    city_name: Option<String>,
    district_code: Option<String>,
    district_name: Option<String>,
}

impl AddressInput {
    fn valid(&self) -> bool {
        let phone = self.phone.trim().as_bytes();
        (1..=50).contains(&self.consignee.trim().chars().count())
            && (1..=200).contains(&self.detail.trim().chars().count())
            && phone.len() == 11
            && phone[0] == b'1'
            && (b'3'..=b'9').contains(&phone[1])
            && phone.iter().all(u8::is_ascii_digit)
            && self
                .sex
                .as_deref()
                .is_none_or(|sex| matches!(sex, "" | "0" | "1"))
            && self
                .label
                .as_deref()
                .is_none_or(|label| label.chars().count() <= 100)
            && [&self.province_code, &self.city_code, &self.district_code]
                .iter()
                .all(|value| value.as_deref().is_none_or(|s| s.chars().count() <= 12))
            && [&self.province_name, &self.city_name, &self.district_name]
                .iter()
                .all(|value| value.as_deref().is_none_or(|s| s.chars().count() <= 32))
    }
}

#[utoipa::path(post, path="/user/addressBook", tag="地址簿", summary="新增收货地址", security(("CustomerToken"=[])),
    request_body=AddressInput, responses((status=200, description="保存结果。", body=ApiResult<NoData>)))]
pub(crate) async fn create(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
    Json(input): Json<AddressInput>,
) -> Reply<NoData> {
    if !input.valid() {
        return fail(StatusCode::BAD_REQUEST, "请检查收货人、手机号和详细地址");
    }
    let Some(service) = state else {
        return unavailable();
    };
    let result = sqlx::query("INSERT INTO address_book (user_id,consignee,phone,detail,sex,label,province_code,province_name,city_code,city_name,district_code,district_name,is_default)
        VALUES (?,?,?,?,?,?,?,?,?,?,?,?,0)").bind(actor.0).bind(input.consignee.trim()).bind(input.phone.trim()).bind(input.detail.trim())
        .bind(input.sex).bind(input.label).bind(input.province_code).bind(input.province_name).bind(input.city_code).bind(input.city_name)
        .bind(input.district_code).bind(input.district_name).execute(&service.pool).await;
    changed(result)
}

#[utoipa::path(put, path="/user/addressBook", tag="地址簿", summary="修改本人收货地址", security(("CustomerToken"=[])),
    request_body=AddressInput, responses((status=200, description="保存结果。", body=ApiResult<NoData>)))]
pub(crate) async fn edit(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
    Json(input): Json<AddressInput>,
) -> Reply<NoData> {
    let Some(id) = input.id.filter(|id| *id > 0) else {
        return fail(StatusCode::BAD_REQUEST, "缺少地址 ID");
    };
    if !input.valid() {
        return fail(StatusCode::BAD_REQUEST, "请检查收货人、手机号和详细地址");
    }
    let Some(service) = state else {
        return unavailable();
    };
    changed(sqlx::query("UPDATE address_book SET consignee=?,phone=?,detail=?,sex=?,label=?,province_code=?,province_name=?,city_code=?,city_name=?,district_code=?,district_name=? WHERE id=? AND user_id=?")
        .bind(input.consignee.trim()).bind(input.phone.trim()).bind(input.detail.trim()).bind(input.sex).bind(input.label)
        .bind(input.province_code).bind(input.province_name).bind(input.city_code).bind(input.city_name).bind(input.district_code).bind(input.district_name)
        .bind(id).bind(actor.0).execute(&service.pool).await)
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct AddressId {
    id: i64,
}

#[utoipa::path(put, path="/user/addressBook/default", tag="地址簿", summary="设为默认收货地址", security(("CustomerToken"=[])),
    request_body=AddressId, responses((status=200, description="保存结果。", body=ApiResult<NoData>)))]
pub(crate) async fn set_default(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
    Json(target): Json<AddressId>,
) -> Reply<NoData> {
    let Some(service) = state else {
        return unavailable();
    };
    let result: Result<bool, sqlx::Error> = async {
        let mut tx = service.pool.begin().await?;
        sqlx::query("UPDATE address_book SET is_default=0 WHERE user_id=?")
            .bind(actor.0)
            .execute(&mut *tx)
            .await?;
        let changed = sqlx::query("UPDATE address_book SET is_default=1 WHERE user_id=? AND id=?")
            .bind(actor.0)
            .bind(target.id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed == 0 {
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }
    .await;
    match result {
        Ok(true) => response(Ok(NoData)),
        Ok(false) => fail(StatusCode::OK, "地址不存在"),
        Err(error) => response(Err(error)),
    }
}

#[utoipa::path(delete, path="/user/addressBook", tag="地址簿", summary="删除本人收货地址", security(("CustomerToken"=[])),
    params(("id"=i64, Query)), responses((status=200, description="删除结果。", body=ApiResult<NoData>)))]
pub(crate) async fn remove(
    State(state): State<CustomerState>,
    Extension(actor): Extension<CustomerIdentity>,
    Query(target): Query<AddressId>,
) -> Reply<NoData> {
    let Some(service) = state else {
        return unavailable();
    };
    changed(
        sqlx::query("DELETE FROM address_book WHERE id=? AND user_id=?")
            .bind(target.id)
            .bind(actor.0)
            .execute(&service.pool)
            .await,
    )
}

fn changed(result: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error>) -> Reply<NoData> {
    match result {
        Ok(value) if value.rows_affected() == 1 => response(Ok(NoData)),
        Ok(_) => fail(StatusCode::OK, "地址不存在"),
        Err(error) => response(Err(error)),
    }
}

fn response<T>(result: Result<T, sqlx::Error>) -> Reply<T> {
    match result {
        Ok(data) => (StatusCode::OK, Json(ApiResult::success(data))),
        Err(error) => {
            eprintln!("Address request failed: {error}");
            unavailable()
        }
    }
}
fn unavailable<T>() -> Reply<T> {
    fail(StatusCode::SERVICE_UNAVAILABLE, "收货地址暂不可用")
}

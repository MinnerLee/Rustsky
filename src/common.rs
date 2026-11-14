use serde::Serialize;
use utoipa::{
    PartialSchema, ToSchema,
    openapi::RefOr,
    openapi::schema::{Object, Schema, Type},
};

#[derive(Serialize)]
pub struct NoData;

impl PartialSchema for NoData {
    fn schema() -> RefOr<Schema> {
        Object::with_type(Type::Null).into()
    }
}

impl ToSchema for NoData {}

#[derive(Serialize, ToSchema)]
pub struct ApiResult<T> {
    /// 业务结果。1 表示成功，0 表示失败。
    code: u8,
    /// 失败原因，成功时为 null。
    #[schema(required = true)]
    msg: Option<&'static str>,
    /// 返回的数据，无数据时为 null。
    #[schema(required = true, no_recursion)]
    data: Option<T>,
}

impl<T> ApiResult<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 1,
            msg: None,
            data: Some(data),
        }
    }

    pub fn failure(message: &'static str) -> Self {
        Self {
            code: 0,
            msg: Some(message),
            data: None,
        }
    }
}

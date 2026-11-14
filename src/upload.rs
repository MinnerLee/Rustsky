use crate::{
    common::ApiResult,
    employee::{
        EmployeeIdentity, EmployeeState, authenticate_employee,
        manage::{admin_database, denied},
    },
};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    middleware,
    routing::post,
};
use rand_core::{OsRng, RngCore};

const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;

pub fn routes(service: EmployeeState) -> Router {
    Router::new()
        .route(
            "/upload",
            post(upload).layer(DefaultBodyLimit::max(MAX_IMAGE_BYTES + 8192)),
        )
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            authenticate_employee,
        ))
        .with_state(service)
}

#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
struct ImageUpload {
    #[schema(format = Binary)]
    file: String,
}

#[utoipa::path(post, path="/api/common/upload", tag="菜品", summary="上传菜品图片", security(("EmployeeToken" = [])),
    request_body(content = ImageUpload, content_type = "multipart/form-data"),
    responses((status=200, description="图片访问路径。", body=ApiResult<String>)))]
pub(crate) async fn upload(
    State(service): State<EmployeeState>,
    Extension(actor): Extension<EmployeeIdentity>,
    mut form: Multipart,
) -> (StatusCode, Json<ApiResult<String>>) {
    if let Err(status) = admin_database(&service, actor).await {
        return denied(status);
    }
    let failure = |status, message| (status, Json(ApiResult::failure(message)));
    let Ok(Some(field)) = form.next_field().await else {
        return failure(StatusCode::BAD_REQUEST, "请选择图片");
    };
    if field.name() != Some("file") {
        return failure(StatusCode::BAD_REQUEST, "请使用 file 字段上传图片");
    }
    let bytes = match field.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => return failure(error.status(), "无法读取上传文件"),
    };
    if bytes.len() > MAX_IMAGE_BYTES {
        return failure(StatusCode::PAYLOAD_TOO_LARGE, "图片最多 2 MiB");
    }
    let extension = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.ends_with(b"IEND\xaeB`\x82")
    {
        "png"
    } else if bytes.starts_with(b"\xff\xd8\xff") && bytes.ends_with(b"\xff\xd9") {
        "jpg"
    } else {
        return failure(StatusCode::BAD_REQUEST, "仅支持 JPEG 或 PNG 图片");
    };
    let name = format!(
        "{:016x}{:016x}.{extension}",
        OsRng.next_u64(),
        OsRng.next_u64()
    );
    let folder = std::path::Path::new(".preview/uploads");
    if tokio::fs::create_dir_all(folder).await.is_err()
        || tokio::fs::write(folder.join(&name), bytes).await.is_err()
    {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "暂时无法保存图片");
    }
    (
        StatusCode::OK,
        Json(ApiResult::success(format!("/uploads/{name}"))),
    )
}

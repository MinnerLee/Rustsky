use axum::{
    extract::{Request, State},
    http::Method,
    middleware::Next,
    response::Response,
};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Serialize, de::DeserializeOwned};
use std::{future::Future, time::Duration};
use tokio::time::timeout;

const VERSION_KEY: &str = "rustsky:menu:version";
const TTL_SECONDS: u64 = 60;
const REDIS_TIMEOUT: Duration = Duration::from_millis(250);

pub(crate) async fn load<T: DeserializeOwned + Serialize>(
    redis: Option<ConnectionManager>,
    resource: &str,
    id: i64,
    query: impl Future<Output = Result<T, sqlx::Error>>,
) -> Result<T, sqlx::Error> {
    let Some(mut redis) = redis else {
        return query.await;
    };
    let Ok(Ok(version)) = timeout(REDIS_TIMEOUT, redis.get::<_, Option<u64>>(VERSION_KEY)).await
    else {
        return query.await;
    };
    let version = version.unwrap_or(0);
    let key = format!("rustsky:menu:{version}:{resource}:{id}");
    if let Ok(Ok(Some(json))) = timeout(REDIS_TIMEOUT, redis.get::<_, Option<String>>(&key)).await
        && let Ok(data) = serde_json::from_str(&json)
    {
        return Ok(data);
    }
    let data = query.await?;
    if let Ok(json) = serde_json::to_string(&data) {
        let _ = timeout(
            REDIS_TIMEOUT,
            redis.set_ex::<_, _, ()>(key, json, TTL_SECONDS),
        )
        .await;
    }
    Ok(data)
}

pub(crate) async fn invalidate_after_write(
    State(redis): State<Option<ConnectionManager>>,
    request: Request,
    next: Next,
) -> Response {
    let mutating = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::DELETE
    );
    let response = next.run(request).await;
    if mutating
        && response.status().is_success()
        && let Some(mut redis) = redis
        && !matches!(
            timeout(REDIS_TIMEOUT, redis.incr::<_, _, u64>(VERSION_KEY, 1)).await,
            Ok(Ok(_))
        )
    {
        eprintln!("Menu cache invalidation unavailable; existing values will expire by TTL");
    }
    response
}

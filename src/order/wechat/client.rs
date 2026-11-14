use super::{Error, MiniPayment, Transaction};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use axum::http::{HeaderMap, Method, StatusCode};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use rand_core::{OsRng, RngCore};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1::DecodeRsaPrivateKey,
    pkcs1v15::{Signature, SigningKey, VerifyingKey},
    pkcs8::{DecodePrivateKey, DecodePublicKey},
    sha2::Sha256,
    signature::{RandomizedSigner, SignatureEncoding, Verifier},
    traits::PublicKeyParts,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{env, fs, io, time::Duration};

const API: &str = "https://api.mch.weixin.qq.com";

pub(crate) struct Client {
    pub(super) appid: String,
    pub(super) mchid: String,
    merchant_serial: String,
    platform_id: String,
    notify_url: String,
    signing_key: SigningKey<Sha256>,
    verifying_key: VerifyingKey<Sha256>,
    cipher: Aes256Gcm,
    http: reqwest::Client,
    #[cfg(test)]
    pub(super) test_api: Option<String>,
}

impl Client {
    pub(crate) fn from_env() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let settings = [
            "RUSTSKY_WXPAY_MCH_ID",
            "RUSTSKY_WXPAY_MERCHANT_SERIAL",
            "RUSTSKY_WXPAY_PRIVATE_KEY",
            "RUSTSKY_WXPAY_PUBLIC_KEY_ID",
            "RUSTSKY_WXPAY_PUBLIC_KEY",
            "RUSTSKY_WXPAY_API_V3_KEY",
            "RUSTSKY_WXPAY_NOTIFY_URL",
        ];
        if settings.iter().all(|name| env::var_os(name).is_none()) {
            return Ok(None);
        }
        let required = |name: &str| {
            env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| io::Error::other(format!("Missing payment setting {name}")))
        };
        let [
            mchid,
            merchant_serial,
            private_path,
            platform_id,
            public_path,
            api_key,
            notify_url,
        ] = settings
            .map(required)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| io::Error::other("Invalid payment settings"))?;
        let appid = required("RUSTSKY_WX_APP_ID")?;
        let _ = required("RUSTSKY_WX_APP_SECRET")?;
        let url = reqwest::Url::parse(&notify_url)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || url.path() != "/notify/wechat/payment"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !mchid.bytes().all(|b| b.is_ascii_digit())
            || !merchant_serial.bytes().all(|b| b.is_ascii_hexdigit())
            || !platform_id
                .strip_prefix("PUB_KEY_ID_")
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(io::Error::other(
                "Invalid WeChat Pay merchant, key identifier or callback URL",
            )
            .into());
        }
        let pem = fs::read_to_string(private_path)?;
        let private =
            RsaPrivateKey::from_pkcs8_pem(&pem).or_else(|_| RsaPrivateKey::from_pkcs1_pem(&pem))?;
        private.validate()?;
        let public = RsaPublicKey::from_public_key_pem(&fs::read_to_string(public_path)?)?;
        if private.n().bits() != 2048 || public.n().bits() != 2048 {
            return Err(io::Error::other("WeChat Pay expects 2048-bit RSA keys").into());
        }
        let cipher = Aes256Gcm::new_from_slice(api_key.as_bytes())
            .map_err(|_| io::Error::other("APIv3 key must be 32 bytes"))?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("Rustsky/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Some(Self {
            appid,
            mchid,
            merchant_serial,
            platform_id,
            notify_url,
            signing_key: SigningKey::new(private),
            verifying_key: VerifyingKey::new(public),
            cipher,
            http,
            #[cfg(test)]
            test_api: None,
        }))
    }

    fn sign(&self, message: &[u8]) -> Result<String, Error> {
        self.signing_key
            .try_sign_with_rng(&mut OsRng, message)
            .map(|signature| STANDARD.encode(signature.to_bytes()))
            .map_err(|_| Error::Protocol)
    }

    pub(super) fn verify(&self, headers: &HeaderMap, body: &[u8]) -> Result<(), Error> {
        let header = |name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .ok_or(Error::Invalid)
        };
        let stamp = header("wechatpay-timestamp")?;
        let seconds = stamp.parse::<i64>().map_err(|_| Error::Invalid)?;
        if seconds.abs_diff(Utc::now().timestamp()) > 300
            || header("wechatpay-serial")? != self.platform_id
        {
            return Err(Error::Invalid);
        }
        let nonce = header("wechatpay-nonce")?;
        let signature = STANDARD
            .decode(header("wechatpay-signature")?)
            .map_err(|_| Error::Invalid)?;
        let signature = Signature::try_from(signature.as_slice()).map_err(|_| Error::Invalid)?;
        let mut message = format!("{stamp}\n{nonce}\n").into_bytes();
        message.extend_from_slice(body);
        message.push(b'\n');
        self.verifying_key
            .verify(&message, &signature)
            .map_err(|_| Error::Invalid)
    }

    pub(super) fn callback<T: DeserializeOwned>(
        &self,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<(String, T), Error> {
        #[derive(Deserialize)]
        struct Notice {
            event_type: String,
            resource: Resource,
        }
        #[derive(Deserialize)]
        struct Resource {
            algorithm: String,
            ciphertext: String,
            nonce: String,
            associated_data: Option<String>,
        }
        self.verify(headers, body)?;
        let notice: Notice = serde_json::from_slice(body).map_err(|_| Error::Invalid)?;
        let resource = notice.resource;
        if resource.algorithm != "AEAD_AES_256_GCM" || resource.nonce.len() != 12 {
            return Err(Error::Invalid);
        }
        let encrypted = STANDARD
            .decode(resource.ciphertext)
            .map_err(|_| Error::Invalid)?;
        let payload = Payload {
            msg: &encrypted,
            aad: resource.associated_data.as_deref().unwrap_or("").as_bytes(),
        };
        let nonce: [u8; 12] = resource
            .nonce
            .as_bytes()
            .try_into()
            .map_err(|_| Error::Invalid)?;
        let plain = self
            .cipher
            .decrypt(&Nonce::from(nonce), payload)
            .map_err(|_| Error::Invalid)?;
        serde_json::from_slice(&plain)
            .map(|data| (notice.event_type, data))
            .map_err(|_| Error::Invalid)
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Vec<u8>, Error> {
        let body = body.map(|value| value.to_string()).unwrap_or_default();
        let timestamp = Utc::now().timestamp();
        let nonce = nonce()?;
        let signature =
            self.sign(format!("{method}\n{path}\n{timestamp}\n{nonce}\n{body}\n").as_bytes())?;
        let auth = format!(
            "WECHATPAY2-SHA256-RSA2048 mchid=\"{}\",nonce_str=\"{nonce}\",timestamp=\"{timestamp}\",serial_no=\"{}\",signature=\"{signature}\"",
            self.mchid, self.merchant_serial
        );
        let api = API;
        #[cfg(test)]
        let api = self.test_api.as_deref().unwrap_or(api);
        let mut request = self
            .http
            .request(method, format!("{api}{path}"))
            .header("Authorization", auth)
            .header("Accept", "application/json")
            .header("Wechatpay-Serial", &self.platform_id);
        if !body.is_empty() {
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        }
        let response = request.send().await.map_err(|_| Error::Network)?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.bytes().await.map_err(|_| Error::Network)?;
        if status.is_success() || headers.contains_key("wechatpay-signature") {
            self.verify(&headers, &bytes).map_err(|_| Error::Protocol)?;
        }
        if !status.is_success() {
            let code = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|data| data["code"].as_str().map(str::to_owned));
            return Err(
                if matches!(
                    code.as_deref(),
                    Some("ORDER_NOT_EXIST" | "RESOURCE_NOT_EXISTS")
                ) && status == StatusCode::NOT_FOUND
                {
                    Error::NotFound
                } else {
                    Error::Provider(status)
                },
            );
        }
        Ok(bytes.to_vec())
    }

    async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, Error> {
        serde_json::from_slice(&self.request(method, path, body).await?)
            .map_err(|_| Error::Protocol)
    }

    pub(super) async fn prepay(
        &self,
        number: &str,
        openid: &str,
        cents: i32,
        expiry: &str,
    ) -> Result<String, Error> {
        #[derive(Deserialize)]
        struct Prepay {
            prepay_id: String,
        }
        let response: Prepay = self.json(Method::POST, "/v3/pay/transactions/jsapi", Some(json!({
            "appid": self.appid, "mchid": self.mchid, "description": "Rustsky 餐饮订单", "out_trade_no": number,
            "time_expire": expiry, "notify_url": self.notify_url, "amount": { "total": cents, "currency": "CNY" },
            "payer": { "openid": openid }
        }))).await?;
        if !valid_prepay_id(&response.prepay_id) {
            return Err(Error::Protocol);
        }
        Ok(response.prepay_id)
    }

    pub(super) fn mini_payment(&self, prepay_id: &str) -> Result<MiniPayment, Error> {
        if !valid_prepay_id(prepay_id) {
            return Err(Error::Protocol);
        }
        let time_stamp = Utc::now().timestamp().to_string();
        let nonce_str = nonce()?;
        let package_str = format!("prepay_id={prepay_id}");
        let pay_sign = self.sign(
            format!("{}\n{time_stamp}\n{nonce_str}\n{package_str}\n", self.appid).as_bytes(),
        )?;
        Ok(MiniPayment {
            time_stamp,
            nonce_str,
            package_str,
            pay_sign,
            sign_type: "RSA",
        })
    }

    pub(super) async fn query(&self, number: &str) -> Result<Transaction, Error> {
        self.json(
            Method::GET,
            &format!(
                "/v3/pay/transactions/out-trade-no/{number}?mchid={}",
                self.mchid
            ),
            None,
        )
        .await
    }

    pub(super) async fn close(&self, number: &str) -> Result<(), Error> {
        let body = self
            .request(
                Method::POST,
                &format!("/v3/pay/transactions/out-trade-no/{number}/close"),
                Some(json!({"mchid": self.mchid})),
            )
            .await?;
        if body.is_empty() {
            Ok(())
        } else {
            Err(Error::Protocol)
        }
    }

    pub(super) async fn refund(
        &self,
        number: &str,
        cents: i32,
        state: &str,
    ) -> Result<Refund, Error> {
        let refund_number = format!("rs_{number}");
        let path = format!("/v3/refund/domestic/refunds/{refund_number}");
        match self.json(Method::GET, &path, None).await {
            Ok(refund) => Ok(refund),
            Err(Error::NotFound) if state == "PENDING" => self.json(Method::POST, "/v3/refund/domestic/refunds", Some(json!({
                "out_trade_no": number, "out_refund_no": refund_number, "reason": "餐饮订单取消",
                "notify_url": self.notify_url,
                "amount": { "total": cents, "refund": cents, "currency": "CNY" }
            }))).await,
            Err(error) => Err(error),
        }
    }
}

fn valid_prepay_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 118 && value.bytes().all(|b| b.is_ascii_graphic())
}

#[derive(Deserialize)]
pub(super) struct Refund {
    pub(super) out_trade_no: String,
    pub(super) out_refund_no: String,
    pub(super) status: String,
    pub(super) amount: RefundAmount,
}

#[derive(Deserialize)]
pub(super) struct RefundAmount {
    pub(super) total: i64,
    pub(super) refund: i64,
}

fn nonce() -> Result<String, Error> {
    let mut bytes = [0_u8; 16];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| Error::Protocol)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in crate::order::wechat) fn test_client() -> Client {
        let key = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
        let public = key.to_public_key();
        Client {
            appid: "wx-test".into(),
            mchid: "1234567890".into(),
            merchant_serial: "ABCD".into(),
            platform_id: "PUB_KEY_ID_1".into(),
            notify_url: "https://example.invalid/notify/wechat/payment".into(),
            signing_key: SigningKey::new(key),
            verifying_key: VerifyingKey::new(public),
            cipher: Aes256Gcm::new_from_slice(&[11; 32]).unwrap(),
            http: reqwest::Client::new(),
            test_api: None,
        }
    }

    pub(in crate::order::wechat) fn signed(
        client: &Client,
        bytes: &[u8],
        timestamp: i64,
    ) -> HeaderMap {
        let nonce = "provider-response-nonce";
        let mut payload = format!("{timestamp}\n{nonce}\n").into_bytes();
        payload.extend_from_slice(bytes);
        payload.push(b'\n');
        let mut headers = HeaderMap::new();
        headers.insert(
            "wechatpay-timestamp",
            timestamp.to_string().parse().unwrap(),
        );
        headers.insert("wechatpay-nonce", nonce.parse().unwrap());
        headers.insert("wechatpay-serial", client.platform_id.parse().unwrap());
        headers.insert(
            "wechatpay-signature",
            client.sign(&payload).unwrap().parse().unwrap(),
        );
        headers
    }

    pub(in crate::order::wechat) fn notice(
        client: &Client,
        event: &str,
        payload: Value,
    ) -> (HeaderMap, Vec<u8>) {
        let nonce: [u8; 12] = OsRng
            .next_u64()
            .to_le_bytes()
            .into_iter()
            .chain(OsRng.next_u32().to_le_bytes())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        // ASCII keeps the nonce representable in a JSON string.
        let nonce = nonce.map(|byte| b'A' + byte % 26);
        let aad = "transaction";
        let data = client
            .cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: payload.to_string().as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .unwrap();
        let bytes = serde_json::to_vec(&json!({"event_type":event,"resource":{
            "algorithm":"AEAD_AES_256_GCM","ciphertext": STANDARD.encode(data),
            "nonce": std::str::from_utf8(&nonce).unwrap(),"associated_data":aad
        }}))
        .unwrap();
        (signed(client, &bytes, Utc::now().timestamp()), bytes)
    }

    #[test]
    fn notification_rejects_changed_body_wrong_key_and_stale_timestamp() {
        let client = test_client();
        let (mut headers, bytes) = notice(
            &client,
            "TRANSACTION.SUCCESS",
            json!({"out_trade_no":"example1"}),
        );
        let (event, data) = client.callback::<Value>(&headers, &bytes).unwrap();
        assert_eq!(event, "TRANSACTION.SUCCESS");
        assert_eq!(data["out_trade_no"], "example1");
        let mut modified = bytes.clone();
        modified.push(b' ');
        assert!(client.callback::<Value>(&headers, &modified).is_err());
        headers.insert("wechatpay-serial", "PUB_KEY_ID_2".parse().unwrap());
        assert!(client.callback::<Value>(&headers, &bytes).is_err());
        let stale = signed(&client, &bytes, Utc::now().timestamp() - 301);
        assert!(client.callback::<Value>(&stale, &bytes).is_err());
        let empty = signed(&client, b"", Utc::now().timestamp());
        assert!(client.verify(&empty, b"").is_ok());
        assert!(client.verify(&HeaderMap::new(), b"").is_err());
    }

    #[tokio::test]
    async fn prepay_signs_request_and_miniprogram_parameters() {
        use axum::{Router, body::Bytes, extract::State, routing::post};
        use std::sync::Arc;
        let mut client = test_client();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        client.test_api = Some(format!("http://{}", listener.local_addr().unwrap()));
        let client = Arc::new(client);
        let app = Router::new().route("/v3/pay/transactions/jsapi", post(
            |State(client): State<Arc<Client>>, headers: HeaderMap, bytes: Bytes| async move {
                let auth = headers["authorization"].to_str().unwrap().split_once(' ').unwrap().1;
                let auth = auth.split(',').map(|part| {
                    let (name, value) = part.split_once('=').unwrap();
                    (name, value.trim_matches('"'))
                }).collect::<std::collections::HashMap<_,_>>();
                let body = std::str::from_utf8(&bytes).unwrap();
                let message = format!("POST\n/v3/pay/transactions/jsapi\n{}\n{}\n{body}\n", auth["timestamp"],auth["nonce_str"]);
                let signature = STANDARD.decode(auth["signature"]).unwrap();
                client.verifying_key.verify(message.as_bytes(), &Signature::try_from(signature.as_slice()).unwrap()).unwrap();
                let data: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(data["amount"]["total"], 5950);
                assert_eq!(data["payer"]["openid"], "fake-payer");
                assert_eq!(data["out_trade_no"], "example1");
                let response = r#"{"prepay_id":"fake-session"}"#;
                (signed(&client, response.as_bytes(), Utc::now().timestamp()), response)
            })).with_state(client.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let prepay_id = client
            .prepay("example1", "fake-payer", 5950, "2030-01-01T08:00:00+08:00")
            .await
            .unwrap();
        let result = client.mini_payment(&prepay_id).unwrap();
        assert_eq!(result.package_str, "prepay_id=fake-session");
        let signature = STANDARD.decode(result.pay_sign).unwrap();
        let message = format!(
            "{}\n{}\n{}\n{}\n",
            client.appid, result.time_stamp, result.nonce_str, result.package_str
        );
        client
            .verifying_key
            .verify(
                message.as_bytes(),
                &Signature::try_from(signature.as_slice()).unwrap(),
            )
            .unwrap();
        server.abort();
    }
}

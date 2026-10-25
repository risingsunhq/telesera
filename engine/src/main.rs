mod fmp4;
mod live;
mod media;
mod network;
mod session;
mod stickers;
mod telegram;
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine;
use grammers_client::tl;
use grammers_client::{
    Client, SignInError,
    client::{LoginToken, PasswordToken},
    peer::Peer,
};
use grammers_session::Session;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tower_http::services::ServeDir;

#[derive(Deserialize)]
struct Config {
    api_id: i32,
    api_hash: String,
    token: String,
    data_dir: PathBuf,
    ui_dir: PathBuf,
    #[serde(default = "default_port")]
    port: u16,
    import_session: Option<Import>,
}
fn default_port() -> u16 {
    3190
}
#[derive(Deserialize)]
struct Import {
    dc_id: i32,
    auth_key_hex: String,
}
#[derive(Default)]
struct Auth {
    phone: Option<LoginToken>,
    password: Option<PasswordToken>,
}
struct App {
    client: Client,
    session: Arc<session::FileSession>,
    config: Config,
    auth: Mutex<Auth>,
    peers: RwLock<HashMap<String, Peer>>,
    downloads: Arc<Semaphore>,
    images: Arc<Semaphore>,
    image_cache: Mutex<media::ImageCache>,
    stickers: Mutex<stickers::Catalog>,
    live: Mutex<HashMap<String, live::Stream>>,
}
type Shared = Arc<App>;
struct ApiError(StatusCode, String);
type ApiResult<T> = Result<T, ApiError>;
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(StatusCode::BAD_GATEWAY, e.into().to_string())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
fn bad(message: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message.into())
}
fn missing(message: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, message.into())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 3 && args[1] == "--config",
        "Usage: telesera-engine --config PATH"
    );
    let config_path = PathBuf::from(&args[2]);
    std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600))?;
    let mut config: Config = serde_json::from_slice(&std::fs::read(&config_path)?)?;
    anyhow::ensure!(
        config.api_id > 0 && config.api_hash.len() == 32 && config.token.len() >= 32,
        "Invalid API credentials or local token"
    );
    std::fs::create_dir_all(&config.data_dir)?;
    std::fs::set_permissions(&config.data_dir, std::fs::Permissions::from_mode(0o700))?;
    let import = config
        .import_session
        .take()
        .map(|i| (i.dc_id, i.auth_key_hex));
    let session = Arc::new(session::FileSession::open(
        config.data_dir.join("session.json"),
        import,
    )?);
    let pool = grammers_client::SenderPool::new(session.clone(), config.api_id);
    let client = Client::new(pool.handle.clone());
    tokio::spawn(network::maintain(pool.handle.clone()));
    tokio::spawn(pool.runner.run());
    // The UI polls history; drain protocol updates so a quiet UI never blocks MTProto.
    let mut updates = pool.updates;
    tokio::spawn(async move { while updates.recv().await.is_some() {} });
    let port = config.port;
    let ui = config.ui_dir.clone();
    let state = Arc::new(App {
        client,
        session,
        config,
        auth: Mutex::new(Auth::default()),
        peers: RwLock::new(HashMap::new()),
        downloads: Arc::new(Semaphore::new(4)),
        images: Arc::new(Semaphore::new(4)),
        image_cache: Mutex::new(media::ImageCache::default()),
        stickers: Mutex::new(stickers::Catalog::default()),
        live: Mutex::new(HashMap::new()),
    });
    let api = Router::new()
        .route("/api/status", get(status))
        .route("/api/auth/phone", post(phone))
        .route("/api/auth/code", post(code))
        .route("/api/auth/password", post(password))
        .route("/api/auth/qr", post(qr))
        .route("/api/auth/qr/check", post(qr))
        .route("/api/stickers", get(stickers::catalog))
        .route("/api/stickers/sets/{id}", get(stickers::set))
        .route("/api/stickers/{id}/thumbnail", get(stickers::thumbnail))
        .route("/api/chats/{id}/stickers", post(stickers::send))
        .route("/api/chats", get(telegram::chats))
        .route("/api/chats/resolve", get(telegram::resolve))
        .route("/api/chats/{id}/messages", get(telegram::messages))
        .route("/api/chats/{id}/send", post(telegram::send))
        .route("/api/chats/{id}/live", get(live::info))
        .route("/api/chats/{id}/live/segment", get(live::segment))
        .route("/api/chats/{id}/photo", get(media::photo))
        .route(
            "/api/chats/{id}/messages/{message}/thumbnail",
            get(media::thumbnail),
        )
        .route(
            "/api/chats/{id}/messages/{message}/media",
            get(media::media),
        )
        .route(
            "/api/chats/{id}/messages/{message}/button",
            post(telegram::button),
        )
        .route("/api/miniapp", post(telegram::miniapp))
        .fallback_service(ServeDir::new(ui).append_index_html_on_directories(true))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), security))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!(
        "Telesera engine {} ready on loopback:{}",
        env!("CARGO_PKG_VERSION"),
        port
    );
    axum::serve(listener, api)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
async fn security(State(app): State<Shared>, req: Request, next: Next) -> Response {
    let host = format!("127.0.0.1:{}", app.config.port);
    let localhost = format!("localhost:{}", app.config.port);
    let valid_host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == host || h == localhost);
    let origin = req.headers().get(header::ORIGIN);
    let valid_origin = origin.is_none_or(|o| {
        o.to_str()
            .ok()
            .is_some_and(|o| o == format!("http://{host}") || o == format!("http://{localhost}"))
    });
    if !valid_host || !valid_origin {
        return ApiError(StatusCode::FORBIDDEN, "Origine non consentita".into()).into_response();
    }
    let is_api = req.uri().path().starts_with("/api/");
    if is_api {
        let bearer = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "));
        let resource = ["/media", "/photo", "/thumbnail"]
            .iter()
            .any(|suffix| req.uri().path().ends_with(suffix));
        let query = if resource {
            url::form_urlencoded::parse(req.uri().query().unwrap_or("").as_bytes())
                .find(|(k, _)| k == "token")
                .map(|(_, v)| v.into_owned())
        } else {
            None
        };
        if bearer.or(query.as_deref()) != Some(app.config.token.as_str()) {
            return ApiError(StatusCode::UNAUTHORIZED, "Accesso locale richiesto".into())
                .into_response();
        }
    }
    let mut response = next.run(req).await;
    if is_api
        && response.status().is_client_error()
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|h| h.as_bytes().starts_with(b"application/json"))
    {
        let range = response.headers().get(header::CONTENT_RANGE).cloned();
        let status = response.status();
        response = ApiError(
            status,
            status
                .canonical_reason()
                .unwrap_or("Richiesta non valida")
                .into(),
        )
        .into_response();
        if let Some(range) = range {
            response.headers_mut().insert(header::CONTENT_RANGE, range);
        }
    }
    for (key, value) in [
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("cache-control", "no-store"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; frame-src https:; object-src 'none'; base-uri 'none'; frame-ancestors 'self'",
        ),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(key),
            axum::http::HeaderValue::from_static(value),
        );
    }
    response
}
async fn status(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    let authorized = app.client.is_authorized().await?;
    let user = if authorized {
        let u = app.client.get_me().await?;
        app.session.cache_peer((&u).into()).await?;
        Some(
            json!({"id":u.id().bare_id_unchecked().to_string(),"name":u.first_name().unwrap_or("Telegram")}),
        )
    } else {
        None
    };
    Ok(Json(
        json!({"authorized":authorized,"user":user,"version":env!("CARGO_PKG_VERSION")}),
    ))
}
#[derive(Deserialize)]
struct Phone {
    phone: String,
}
async fn phone(State(app): State<Shared>, Json(body): Json<Phone>) -> ApiResult<Json<Value>> {
    if body.phone.len() < 5 || body.phone.len() > 25 {
        return Err(bad("Numero non valido"));
    }
    let mut auth = app.auth.lock().await;
    auth.phone = Some(
        app.client
            .request_login_code(&body.phone, &app.config.api_hash)
            .await?,
    );
    auth.password = None;
    Ok(Json(json!({"step":"code"})))
}
#[derive(Deserialize)]
struct Code {
    code: String,
}
async fn code(State(app): State<Shared>, Json(body): Json<Code>) -> ApiResult<Json<Value>> {
    let mut auth = app.auth.lock().await;
    let token = auth
        .phone
        .as_ref()
        .ok_or_else(|| bad("Richiedi prima il codice"))?;
    match app.client.sign_in(token, &body.code).await {
        Ok(_) => {
            auth.phone = None;
            Ok(Json(json!({"step":"ready"})))
        }
        Err(SignInError::PasswordRequired(t)) => {
            auth.password = Some(t);
            Ok(Json(json!({"step":"password"})))
        }
        Err(e) => Err(e.into()),
    }
}
#[derive(Deserialize)]
struct Password {
    password: String,
}
async fn password(State(app): State<Shared>, Json(body): Json<Password>) -> ApiResult<Json<Value>> {
    let mut auth = app.auth.lock().await;
    if auth.password.is_none() {
        return Err(bad("Password non richiesta"));
    }
    // Fetch fresh SRP parameters each attempt; a rejected password must remain retryable.
    let info = app
        .client
        .invoke(&tl::functions::account::GetPassword {})
        .await?;
    app.client
        .check_password(PasswordToken::new(info.into()), body.password.as_bytes())
        .await?;
    auth.password = None;
    auth.phone = None;
    Ok(Json(json!({"step":"ready"})))
}
async fn qr(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    let mut auth = app.auth.lock().await;
    let result = app
        .client
        .invoke(&tl::functions::auth::ExportLoginToken {
            api_id: app.config.api_id,
            api_hash: app.config.api_hash.clone(),
            except_ids: vec![],
        })
        .await;
    let result = match result {
        Ok(tl::enums::auth::LoginToken::MigrateTo(m)) => {
            app.session.set_home_dc_id(m.dc_id).await?;
            app.client
                .invoke(&tl::functions::auth::ImportLoginToken { token: m.token })
                .await
        }
        other => other,
    };
    let result = match result {
        Ok(r) => r,
        Err(grammers_client::InvocationError::Rpc(e)) if e.name == "SESSION_PASSWORD_NEEDED" => {
            let info = app
                .client
                .invoke(&tl::functions::account::GetPassword {})
                .await?;
            auth.password = Some(PasswordToken::new(info.into()));
            return Ok(Json(json!({"step":"password"})));
        }
        Err(e) => return Err(e.into()),
    };
    match result {
        tl::enums::auth::LoginToken::Token(t) => {
            let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(t.token);
            Ok(Json(
                json!({"step":"qr","url":format!("tg://login?token={token}"),"token":token,"expires":t.expires}),
            ))
        }
        tl::enums::auth::LoginToken::Success(_) => {
            let u = app.client.get_me().await?;
            app.session.cache_peer((&u).into()).await?;
            Ok(Json(json!({"step":"ready"})))
        }
        _ => Err(bad("Migrazione login incompleta, riprova")),
    }
}

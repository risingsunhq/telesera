use crate::*;
use axum::extract::{Path, Query};
use grammers_client::{media::Media, message::Message};
use grammers_session::types::PeerRef;

pub async fn chats(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    let mut iter = app.client.iter_dialogs();
    let mut result = Vec::new();
    let mut peers = HashMap::new();
    while let Some(dialog) = iter.next().await? {
        let (archived, unread) = match &dialog.raw {
            tl::enums::Dialog::Dialog(d) => (d.folder_id == Some(1), d.unread_count),
            _ => continue,
        };
        let p = &dialog.peer;
        let id = p.id().bot_api_dialog_id_unchecked().to_string();
        result.push(
            chat_json(
                p,
                archived,
                unread,
                dialog.last_message.as_ref().map(|m| m.text()),
            )
            .await?,
        );
        peers.insert(id, p.clone());
    }
    // Keep explicitly resolved peers usable after a dialog-list refresh.
    app.peers.write().await.extend(peers);
    Ok(Json(json!({"chats":result})))
}
async fn chat_json(
    p: &Peer,
    archived: bool,
    unread: i32,
    last_text: Option<&str>,
) -> ApiResult<Value> {
    let kind = match p {
        Peer::User(u) if u.is_self() => "saved",
        Peer::User(u) if u.is_bot() => "bot",
        Peer::User(_) => "user",
        Peer::Channel(_) => "channel",
        _ => "group",
    };
    Ok(
        json!({"id":p.id().bot_api_dialog_id_unchecked().to_string(),"title":p.name().unwrap_or("Telegram"),"kind":kind,"archived":archived,"unread":unread,"photo":p.photo(false).await?.is_some(),"last_text":last_text,"username":p.username(),"live":matches!(p, Peer::Channel(c) if c.raw.call_active)}),
    )
}

fn valid_username(value: &str) -> Option<&str> {
    let username = value.strip_prefix('@').unwrap_or(value);
    ((5..=32).contains(&username.len())
        && username.as_bytes()[0].is_ascii_alphabetic()
        && username
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_'))
    .then_some(username)
}

#[derive(Deserialize)]
pub struct Resolve {
    username: String,
}
pub async fn resolve(
    State(app): State<Shared>,
    Query(query): Query<Resolve>,
) -> ApiResult<Json<Value>> {
    let username =
        valid_username(&query.username).ok_or_else(|| bad("Username Telegram non valido"))?;
    let peer = app
        .client
        .resolve_username(username)
        .await?
        .ok_or_else(|| missing("Chat non trovata"))?;
    let chat = chat_json(&peer, false, 0, None).await?;
    app.peers
        .write()
        .await
        .insert(peer.id().bot_api_dialog_id_unchecked().to_string(), peer);
    Ok(Json(json!({"chat":chat})))
}

pub async fn peer(app: &Shared, id: &str) -> ApiResult<Peer> {
    if let Some(p) = app.peers.read().await.get(id).cloned() {
        return Ok(p);
    }
    let _ = chats(State(app.clone())).await?;
    app.peers
        .read()
        .await
        .get(id)
        .cloned()
        .ok_or_else(|| missing("Chat non trovata"))
}
pub async fn peer_ref(app: &Shared, id: &str) -> ApiResult<PeerRef> {
    peer(app, id)
        .await?
        .to_ref()
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?
        .ok_or_else(|| missing("Chat non disponibile"))
}
pub async fn message(app: &Shared, id: &str, mid: i32) -> ApiResult<Message> {
    if mid <= 0 {
        return Err(bad("Messaggio non valido"));
    }
    let p = peer_ref(app, id).await?;
    app.client
        .get_messages_by_id(p, &[mid])
        .await?
        .into_iter()
        .next()
        .flatten()
        .ok_or_else(|| missing("Messaggio non trovato"))
}
#[derive(Deserialize)]
pub struct Page {
    before: Option<i32>,
}
pub async fn messages(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Value>> {
    let p = peer_ref(&app, &id).await?;
    let mut iter = app.client.iter_messages(p).limit(51);
    if let Some(before) = page.before {
        if before <= 0 {
            return Err(bad("Pagina non valida"));
        }
        iter = iter.offset_id(before);
    }
    let mut messages = Vec::new();
    while let Some(m) = iter.next().await? {
        messages.push(message_json(&m, &id));
    }
    let has_more = messages.len() > 50;
    messages.truncate(50);
    Ok(Json(json!({"messages":messages,"has_more":has_more})))
}
fn message_json(m: &Message, id: &str) -> Value {
    let mut v = json!({"id":m.id(),"chat_id":id,"date":m.date().to_string(),"text":m.text(),"outgoing":m.outgoing()});
    if let Some(media) = m.media() {
        match media {
            Media::Photo(p) => {
                v["media"] = json!({"kind":"photo","name":"Foto.jpg","mime":"image/jpeg","size":p.size().unwrap_or(0)})
            }
            Media::Sticker(s) => {
                v["media"] = json!({"kind":"sticker","emoji":s.emoji(),"format":stickers::format(&s.document),"mime":s.document.mime_type(),"size":s.document.size().unwrap_or(0)});
            }
            Media::Document(d) => {
                let mime = d.mime_type().unwrap_or("application/octet-stream");
                v["media"] = json!({"kind":if mime.starts_with("video/"){"video"}else{"document"},"name":d.name().unwrap_or("File"),"mime":mime,"size":d.size().unwrap_or(0),"duration":d.duration()});
            }
            _ => {}
        }
    }
    let buttons = buttons(m);
    if !buttons.is_empty() {
        v["buttons"]=Value::Array(buttons.iter().enumerate().map(|(row,buttons)|Value::Array(buttons.iter().enumerate().map(|(col,b)|json!({"text":b.text,"kind":b.kind(),"url":b.url(),"row":row,"col":col})).collect())).collect());
    }
    v
}
#[derive(Deserialize)]
pub struct Send {
    text: String,
}
pub async fn send(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<Send>,
) -> ApiResult<Json<Value>> {
    if body.text.trim().is_empty() || body.text.chars().count() > 4096 {
        return Err(bad("Il messaggio deve contenere da 1 a 4096 caratteri"));
    }
    let m = app
        .client
        .send_message(peer_ref(&app, &id).await?, body.text)
        .await?;
    Ok(Json(json!({"id":m.id()})))
}
struct Button {
    text: String,
    action: Action,
}
enum Action {
    Callback(Vec<u8>),
    Text,
    Url(String),
    WebView(String),
    SimpleWebView(String),
    Unsupported,
}
impl Button {
    fn kind(&self) -> &str {
        match self.action {
            Action::Callback(_) => "callback",
            Action::Text => "text",
            Action::Url(_) => "url",
            Action::WebView(_) | Action::SimpleWebView(_) => "miniapp",
            Action::Unsupported => "unsupported",
        }
    }
    fn url(&self) -> Option<&str> {
        match &self.action {
            Action::Url(url) => Some(url),
            _ => None,
        }
    }
}
fn buttons(m: &Message) -> Vec<Vec<Button>> {
    match m.reply_markup() {
        Some(tl::enums::ReplyMarkup::ReplyInlineMarkup(markup)) => markup
            .rows
            .into_iter()
            .map(|row| {
                let tl::enums::KeyboardInlineButtonRow::Row(row) = row;
                row.buttons
                    .into_iter()
                    .map(|button| {
                        let tl::enums::KeyboardInlineButton::Button(b) = button;
                        let action = match b.r#type {
                            tl::enums::InlineButtonType::Callback(c) if !c.requires_password => {
                                Action::Callback(c.data)
                            }
                            tl::enums::InlineButtonType::Url(u) => Action::Url(u.url),
                            tl::enums::InlineButtonType::WebView(w) => Action::WebView(w.url),
                            _ => Action::Unsupported,
                        };
                        Button {
                            text: b.text,
                            action,
                        }
                    })
                    .collect()
            })
            .collect(),
        Some(tl::enums::ReplyMarkup::ReplyKeyboardMarkup(markup)) => markup
            .rows
            .into_iter()
            .map(|row| {
                let tl::enums::KeyboardButtonRow::Row(row) = row;
                row.buttons
                    .into_iter()
                    .map(|button| {
                        let tl::enums::KeyboardButton::Button(b) = button;
                        let action = match b.r#type {
                            tl::enums::ButtonType::Default => Action::Text,
                            tl::enums::ButtonType::SimpleWebView(w) => Action::SimpleWebView(w.url),
                            _ => Action::Unsupported,
                        };
                        Button {
                            text: b.text,
                            action,
                        }
                    })
                    .collect()
            })
            .collect(),
        _ => vec![],
    }
}
#[derive(Deserialize)]
pub struct Press {
    row: usize,
    col: usize,
}
pub async fn button(
    State(app): State<Shared>,
    Path((id, mid)): Path<(String, i32)>,
    Json(body): Json<Press>,
) -> ApiResult<Json<Value>> {
    let m = message(&app, &id, mid).await?;
    let rows = buttons(&m);
    let b = rows
        .get(body.row)
        .and_then(|r| r.get(body.col))
        .ok_or_else(|| bad("Pulsante non disponibile"))?;
    let p = peer_ref(&app, &id).await?;
    let value = match &b.action {
        Action::Callback(data) => {
            let tl::enums::messages::BotCallbackAnswer::Answer(answer) = app
                .client
                .invoke(&tl::functions::messages::GetBotCallbackAnswer {
                    game: false,
                    peer: p.into(),
                    msg_id: mid,
                    data: Some(data.clone()),
                    password: None,
                })
                .await?;
            json!({"kind":"callback","message":answer.message,"url":answer.url.filter(|u|safe_url(u).is_ok())})
        }
        Action::Text => {
            app.client.send_message(p, b.text.clone()).await?;
            json!({"kind":"text"})
        }
        Action::Url(url) => {
            safe_url(url)?;
            json!({"kind":"url","url":url})
        }
        Action::WebView(url) | Action::SimpleWebView(url) => {
            let bot = m
                .sender_ref()
                .await
                .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?
                .ok_or_else(|| bad("Bot non trovato"))?;
            let result = if matches!(b.action, Action::SimpleWebView(_)) {
                app.client
                    .invoke(&tl::functions::messages::RequestSimpleWebView {
                        from_switch_webview: false,
                        from_side_menu: false,
                        compact: false,
                        fullscreen: false,
                        bot: bot.into(),
                        url: Some(url.clone()),
                        start_param: None,
                        theme_params: None,
                        platform: "web".into(),
                    })
                    .await?
            } else {
                app.client
                    .invoke(&tl::functions::messages::RequestWebView {
                        from_bot_menu: false,
                        silent: true,
                        compact: false,
                        fullscreen: false,
                        peer: p.into(),
                        bot: bot.into(),
                        url: Some(url.clone()),
                        start_param: None,
                        theme_params: None,
                        platform: "web".into(),
                        reply_to: None,
                        send_as: None,
                    })
                    .await?
            };
            let url = webview_url(result)?;
            json!({"kind":"miniapp","url":url})
        }
        Action::Unsupported => {
            return Err(bad(
                "Questo pulsante richiede una funzione Telegram non disponibile sulla TV",
            ));
        }
    };
    Ok(Json(value))
}
fn safe_url(value: &str) -> ApiResult<url::Url> {
    let u = url::Url::parse(value).map_err(|_| bad("Link non valido"))?;
    if !["https", "http", "tg"].contains(&u.scheme())
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err(bad("Tipo di link non consentito"));
    }
    Ok(u)
}
fn webview_url(result: tl::enums::WebViewResult) -> ApiResult<String> {
    let tl::enums::WebViewResult::Url(result) = result;
    let url = safe_url(&result.url)?;
    if url.scheme() != "https" {
        return Err(bad("La Mini App deve usare HTTPS"));
    }
    Ok(result.url)
}
#[derive(Deserialize)]
pub struct MiniApp {
    url: String,
}
pub async fn miniapp(
    State(app): State<Shared>,
    Json(body): Json<MiniApp>,
) -> ApiResult<Json<Value>> {
    let url = safe_url(&body.url)?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("t.me" | "telegram.me"))
        || url.port().is_some()
    {
        return Err(bad("Usa un link ufficiale https://t.me/nomebot?startapp"));
    }
    let parts: Vec<_> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    if !(1..=2).contains(&parts.len())
        || parts
            .iter()
            .any(|s| s.is_empty() || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
    {
        return Err(bad("Link Mini App non valido"));
    }
    let bot = app
        .client
        .resolve_username(parts[0])
        .await?
        .ok_or_else(|| missing("Bot non trovato"))?;
    if !matches!(&bot,Peer::User(u) if u.is_bot()) {
        return Err(bad("Il link non identifica un bot"));
    }
    let p = bot
        .to_ref()
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?
        .ok_or_else(|| missing("Bot non disponibile"))?;
    let query: HashMap<_, _> = url.query_pairs().collect();
    let start = query
        .get("startapp")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let compact = query.get("mode").is_some_and(|s| s == "compact");
    let fullscreen = query.get("mode").is_some_and(|s| s == "fullscreen");
    let result = if parts.len() == 2 {
        app.client
            .invoke(&tl::functions::messages::RequestAppWebView {
                write_allowed: false,
                compact,
                fullscreen,
                peer: p.into(),
                app: tl::types::InputBotAppShortName {
                    bot_id: p.into(),
                    short_name: parts[1].into(),
                }
                .into(),
                start_param: start,
                theme_params: None,
                platform: "web".into(),
            })
            .await?
    } else {
        app.client
            .invoke(&tl::functions::messages::RequestMainWebView {
                compact,
                fullscreen,
                peer: p.into(),
                bot: p.into(),
                start_param: start,
                theme_params: None,
                platform: "web".into(),
            })
            .await?
    };
    Ok(Json(json!({"url":webview_url(result)?})))
}

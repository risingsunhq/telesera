# Telesera local API contract
Independent client by Ennio Cuteri. Third-party protocol libraries retain their own licenses.
Rust engine listens on 127.0.0.1:3190; serves ui/ at /. webOS wrapper opens http://127.0.0.1:3190/#token=LOCAL_SECRET. UI saves token in sessionStorage, removes fragment; all API fetches Authorization: Bearer token. Media/image URL queries ?token= same secret are allowed on loopback. Never log token or Telegram message text. Accept only loopback browser origin (and no Origin for native resource requests), reject other Origins; no wildcard CORS. No remote APIs or Spark dependency.
All JSON failures {error:string}; normal responses as below. IDs strings. Dates ISO8601. No public env credentials. Configuration read from file arg --config PATH: {api_id:integer,api_hash:string,token:string,data_dir:string,ui_dir:string,port:3190,import_session?:{dc_id:integer,auth_key_hex:string}}. Import only once, protect session/config at mode0600. Existing session supported. Engine restart persists account.
GET /api/status -> {authorized:bool,user?:{id,name},version:string}
POST /api/auth/phone {phone} -> {step:"code"}
POST /api/auth/code {code} -> {step:"ready"|"password"}
POST /api/auth/password {password} -> {step:"ready"}
POST /api/auth/qr and POST /api/auth/qr/check -> {step:"qr",url:string,token:string,expires:integer} | {step:"password"} | {step:"ready"}. QR tokens are short-lived credentials; never log or publish them.
GET /api/chats -> {chats:[{id,title,kind:"user"|"bot"|"group"|"channel"|"saved",archived:bool,unread:number,photo:bool,last_text?:string,username?:string,live:bool}]}
GET /api/chats/resolve?username=NAME -> {chat:<same chat object>}; read-only username resolution and peer caching. A leading @ is accepted; usernames contain 5–32 ASCII letters, digits or underscores and begin with a letter. Resolution never sends messages.
GET /api/chats/:id/messages?before=INTEGER -> {messages:[{id:integer,chat_id:string,date:string,text:string,outgoing:bool,media?:{kind:"video"|"photo"|"document"|"sticker",name?:string,mime:string,size:number,duration?:number,emoji?:string,format?:"static"|"animated"|"video"},buttons?:[[{text,kind:"callback"|"text"|"url"|"miniapp"|"unsupported",url?:string,row:number,col:number}]]}],has_more:bool}
GET /api/chats/:id/photo -> image
GET /api/chats/:id/messages/:messageId/thumbnail -> image
GET /api/chats/:id/messages/:messageId/media -> media with Range support; bounded-memory streaming, do not download entire video before playing.
POST /api/chats/:id/send {text} -> {id:integer}
GET /api/chats/:id/live -> {time_ms:integer,step_ms:integer,video:bool}; resolves the channel's active stream. 404 when no watchable stream exists, 425 while it has no parts yet (also during a broadcaster restart).
GET /api/chats/:id/live/segment?t=MS -> video/mp4 (fMP4 init + one fragment) with x-codecs header for Media Source Extensions. t is a multiple of step_ms from the latest /live response. 425 = part not ready yet, 410 = part expired (call /live again), 504 = Telegram stalled for 3 s (retry the same part). Parts are remuxed, never transcoded; quality 2 (best) falls back to 1 and 0.
POST /api/chats/:id/live/leave -> {}; leaves the call if the engine joined it. Some streams return GROUPCALL_JOIN_MISSING to non-members: the engine then joins once as a muted viewer (shared by parallel part requests) and retries.
POST /api/chats/:id/messages/:messageId/button {row,col} -> {kind:"callback"|"url"|"miniapp"|"text",message?:string,url?:string}; server must resolve button from actual Telegram message, no client supplied arbitrary callback payload or URL trust.
POST /api/miniapp {url:string} -> {url:string}; parse official Telegram t.me bot/startapp links, RequestMainWebView or proper rawAPI, no arbitrary outbound fetch. Return signed HTTPS URL to UI iframe. Restrict iframe permissions camera/mic/geolocation, limited explicit Telegram WebApp bridge handling. No arbitrary parent postMessage privileged ops.
Bot poll/new messages: UI can poll active messages every 5s, refresh after send/callback; preserve scroll and focus. Chat list refresh every30s. Engine cache safe for photos and peers. Sensitive data private.


Sticker picker:
- GET /api/stickers -> {recent:[Sticker],favorites:[Sticker],sets:[{id,title,count}],errors:{recent?:string,favorites?:string,sets?:string}}. Partial failures preserve successful collections.
- GET /api/stickers/sets/:id -> {stickers:[Sticker]} for a pack from the catalog.
- Sticker = {id:string,emoji:string,format:"static"|"animated"|"video",preview:bool}.
- GET /api/stickers/:id/thumbnail -> still image; the local token may be supplied in the query. Animated/video stickers use Telegram thumbnails when available.
- POST /api/chats/:id/stickers {sticker_id:string} -> {id:integer}. Sends the original sticker, not the preview. The ID must resolve to a server-held Telegram document; access hashes and file references never leave the engine. Do not automatically retry mutations after ambiguous network failures.

Sticker references expire after 15 minutes and the document cache is bounded to 2048 entries; reopen the picker to refresh expired references. Catalog reads have an 18-second timeout. Avatar downloads use separate image concurrency from video streams, explicit Telegram photo DCs, and an 8 MiB / 256-entry cache with a five-minute lifetime keyed by photo ID. Image bodies are capped at 4 MiB with a 20-second transfer timeout.

The webOS wrapper queries the public LG settings service for localeInfo.locales.UI, with an 800 ms fallback, and passes the selected locale to the HTTP interface through ?lang. The UI supports en/it/es/fr/de, with English fallback; message content and Mini Apps retain their own language. No user locale or credential is sent to an external translation service.

A read-only Telegram round trip runs every 30 seconds. If it receives no response within 12 seconds, the engine drops stale data-center transports without clearing account authorization. Requests then open fresh connections. The initial connection screen retries failed reads every five seconds and on network-online/foreground events; message sends are not replayed by the UI.

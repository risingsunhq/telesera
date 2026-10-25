use crate::*;
use axum::extract::{Path, Query};
use grammers_client::InvocationError;
use std::{
    hash::{BuildHasher, Hasher},
    time::{Duration, Instant},
};

// Channel live streams are read as one-second parts. The account joins as a muted
// viewer only when Telegram requires it, and leaves when the player closes.
#[derive(Clone)]
pub struct Stream {
    call: tl::enums::InputGroupCall,
    dc: i32,
    channel: Option<i32>,
    scale: i32,
    source: i32,
    joined: Option<Instant>,
}

fn rpc_error(e: InvocationError) -> ApiError {
    match &e {
        InvocationError::Rpc(r) if r.name == "TIME_TOO_BIG" => {
            ApiError(StatusCode::TOO_EARLY, "Live part not ready".into())
        }
        // A restarted source invalidates the channel; resynchronising picks the new one.
        InvocationError::Rpc(r)
            if r.name.starts_with("TIME_") || r.name == "VIDEO_CHANNEL_INVALID" =>
        {
            ApiError(StatusCode::GONE, "Live part expired".into())
        }
        // Some streams are only served to call members; segment() joins and retries.
        InvocationError::Rpc(r) if r.name == "GROUPCALL_JOIN_MISSING" => {
            ApiError(StatusCode::CONFLICT, "Join the live stream".into())
        }
        InvocationError::Rpc(r) if r.name.starts_with("GROUPCALL_") => {
            missing(&format!("The live stream has ended ({})", r.name))
        }
        _ => e.into(),
    }
}

pub async fn info(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if !matches!(telegram::peer(&app, &id).await?, Peer::Channel(_)) {
        return Err(missing("No live stream in this chat"));
    }
    let channel = telegram::peer_ref(&app, &id).await?.into();
    let tl::enums::messages::ChatFull::Full(full) = app
        .client
        .invoke(&tl::functions::channels::GetFullChannel { channel })
        .await?;
    let call = match full.full_chat {
        tl::enums::ChatFull::ChannelFull(f) => f.call,
        _ => None,
    }
    .ok_or_else(|| missing("No live stream in this chat"))?;
    let tl::enums::phone::GroupCall::Call(group) = app
        .client
        .invoke(&tl::functions::phone::GetGroupCall {
            call: call.clone(),
            limit: 1,
        })
        .await
        .map_err(rpc_error)?;
    let dc = match group.call {
        tl::enums::GroupCall::Call(c) => c.stream_dc_id,
        tl::enums::GroupCall::Discarded(_) => return Err(missing("The live stream has ended")),
    }
    .ok_or_else(|| missing("This live stream cannot be watched on the TV"))?;
    let request = tl::functions::phone::GetGroupCallStreamChannels { call: call.clone() };
    let mut result = app.client.invoke_in_dc(dc, &request).await;
    // Telegram rate-limits this call per account to about once every few seconds.
    for _ in 0..3 {
        match &result {
            Err(InvocationError::Rpc(r))
                if r.name == "FLOOD_WAIT" && r.value.is_some_and(|v| v <= 10) =>
            {
                tokio::time::sleep(Duration::from_secs(u64::from(r.value.unwrap_or(0)) + 1)).await;
                result = app.client.invoke_in_dc(dc, &request).await;
            }
            _ => break,
        }
    }
    let tl::enums::phone::GroupCallStreamChannels::Channels(channels) =
        result.map_err(rpc_error)?;
    let channels: Vec<_> = channels
        .channels
        .into_iter()
        .map(|tl::enums::GroupCallStreamChannel::Channel(c)| c)
        .filter(|c| c.scale == 0)
        .collect();
    // Channel 0 carries audio only; video channels also include the audio track.
    let best = channels
        .iter()
        .filter(|c| c.channel > 0)
        .min_by_key(|c| c.channel)
        .or(channels.first())
        // Also seen while the broadcaster restarts its RTMP feed.
        .ok_or_else(|| {
            ApiError(
                StatusCode::TOO_EARLY,
                "The live stream has not started yet".into(),
            )
        })?;
    let mut live = app.live.lock().await;
    // A resync keeps the membership so closing the player still leaves the call.
    let (source, joined) = live.get(&id).map_or((0, None), |s| (s.source, s.joined));
    live.insert(
        id,
        Stream {
            call,
            dc,
            channel: (best.channel > 0).then_some(best.channel),
            scale: best.scale,
            source,
            joined,
        },
    );
    Ok(Json(
        json!({"time_ms":best.last_timestamp_ms,"step_ms":1000,"video":best.channel > 0}),
    ))
}

#[derive(Deserialize)]
pub struct Part {
    t: i64,
}
pub async fn segment(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Query(part): Query<Part>,
) -> ApiResult<Response> {
    if part.t < 0 || part.t % 1000 != 0 {
        return Err(bad("Invalid live position"));
    }
    let _permit = app.downloads.clone().acquire_owned().await?;
    let segment = match fetch(&app, &id, part.t).await {
        Err(ApiError(StatusCode::CONFLICT, _)) => {
            join(&app, &id).await?;
            fetch(&app, &id, part.t).await?
        }
        other => other?,
    };
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "video/mp4")
        .header("x-codecs", segment.codecs)
        .body(Body::from(segment.bytes))
        .unwrap())
}
async fn fetch(app: &Shared, id: &str, t: i64) -> ApiResult<fmp4::Segment> {
    let stream = app
        .live
        .lock()
        .await
        .get(id)
        .cloned()
        .ok_or_else(|| missing("Open the live stream again"))?;
    // Not every source offers 720p; fall back to the next quality on other RPC errors.
    let qualities = if stream.channel.is_some() {
        &[2, 1, 0][..]
    } else {
        &[0]
    };
    let mut last = None;
    for &quality in qualities {
        let request = tl::functions::upload::GetFile {
            precise: false,
            cdn_supported: false,
            location: tl::types::InputGroupCallStream {
                call: stream.call.clone(),
                time_ms: t,
                scale: stream.scale,
                video_channel: stream.channel,
                video_quality: stream.channel.map(|_| quality),
            }
            .into(),
            offset: 0,
            limit: 1024 * 1024,
        };
        // Parts normally arrive within about a second; a stalled request is cut short and the UI asks again.
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            app.client.invoke_in_dc(stream.dc, &request),
        )
        .await
        .map_err(|_| {
            eprintln!("Live part {t}: timed out");
            ApiError(StatusCode::GATEWAY_TIMEOUT, "Live part timed out".into())
        })?;
        match result {
            Ok(tl::enums::upload::File::File(file)) => {
                return fmp4::convert(&file.bytes, t)
                    .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()));
            }
            Ok(_) => return Err(bad("Unexpected live stream response")),
            Err(InvocationError::Rpc(r))
                if !r.name.starts_with("TIME_")
                    && r.name != "VIDEO_CHANNEL_INVALID"
                    && !r.name.starts_with("GROUPCALL_")
                    && !r.name.starts_with("FLOOD_") =>
            {
                last = Some(InvocationError::Rpc(r))
            }
            Err(e) => {
                // Error names only: no tokens or message contents reach the log.
                if !matches!(&e, InvocationError::Rpc(r) if r.name.starts_with("TIME_")) {
                    eprintln!("Live part {t}: {e}");
                }
                return Err(rpc_error(e));
            }
        }
    }
    let e = last.expect("at least one quality");
    eprintln!("Live part {t}: {e}");
    Err(rpc_error(e))
}
async fn join(app: &Shared, id: &str) -> ApiResult<()> {
    // Holding the lock lets parallel part requests share a single join.
    let mut live = app.live.lock().await;
    let stream = live
        .get_mut(id)
        .ok_or_else(|| missing("Open the live stream again"))?;
    if stream
        .joined
        .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
    {
        return Ok(());
    }
    let source = (std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
        >> 33) as i32;
    let params = json!({"ufrag":"","pwd":"","fingerprints":[],"ssrc":source}).to_string();
    app.client
        .invoke(&tl::functions::phone::JoinGroupCall {
            muted: true,
            video_stopped: true,
            call: stream.call.clone(),
            join_as: tl::enums::InputPeer::PeerSelf,
            invite_hash: None,
            public_key: None,
            block: None,
            params: tl::types::DataJson { data: params }.into(),
        })
        .await
        .map_err(rpc_error)?;
    stream.source = source;
    stream.joined = Some(Instant::now());
    Ok(())
}
pub async fn leave(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if let Some(stream) = app.live.lock().await.remove(&id)
        && stream.joined.is_some()
    {
        app.client
            .invoke(&tl::functions::phone::LeaveGroupCall {
                call: stream.call,
                source: stream.source,
            })
            .await
            .map_err(rpc_error)?;
    }
    Ok(Json(json!({})))
}

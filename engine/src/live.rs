use crate::*;
use axum::extract::{Path, Query};
use grammers_client::InvocationError;

// Channel live streams are read as one-second parts without joining the call,
// so the account does not appear as a participant.
#[derive(Clone)]
pub struct Stream {
    call: tl::enums::InputGroupCall,
    dc: i32,
    channel: Option<i32>,
    scale: i32,
}

fn rpc_error(e: InvocationError) -> ApiError {
    match &e {
        InvocationError::Rpc(r) if r.name == "TIME_TOO_BIG" => {
            ApiError(StatusCode::TOO_EARLY, "Live part not ready".into())
        }
        InvocationError::Rpc(r) if r.name.starts_with("TIME_") => {
            ApiError(StatusCode::GONE, "Live part expired".into())
        }
        InvocationError::Rpc(r) if r.name.starts_with("GROUPCALL_") => {
            missing("The live stream has ended")
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
    let result = match app.client.invoke_in_dc(dc, &request).await {
        // Telegram rate-limits this call to about once every few seconds.
        Err(InvocationError::Rpc(r))
            if r.name == "FLOOD_WAIT" && r.value.is_some_and(|v| v <= 10) =>
        {
            tokio::time::sleep(std::time::Duration::from_secs(r.value.unwrap_or(0).into())).await;
            app.client.invoke_in_dc(dc, &request).await
        }
        other => other,
    };
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
        .ok_or_else(|| missing("The live stream has not started yet"))?;
    let stream = Stream {
        call,
        dc,
        channel: (best.channel > 0).then_some(best.channel),
        scale: best.scale,
    };
    app.live.lock().await.insert(id, stream);
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
    let stream = app
        .live
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| missing("Open the live stream again"))?;
    let _permit = app.downloads.clone().acquire_owned().await?;
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
                time_ms: part.t,
                scale: stream.scale,
                video_channel: stream.channel,
                video_quality: stream.channel.map(|_| quality),
            }
            .into(),
            offset: 0,
            limit: 1024 * 1024,
        };
        // Occasionally a part request stalls; the UI retries rather than freezing the stream.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            app.client.invoke_in_dc(stream.dc, &request),
        )
        .await
        .map_err(|_| ApiError(StatusCode::GATEWAY_TIMEOUT, "Live part timed out".into()))?;
        match result {
            Ok(tl::enums::upload::File::File(file)) => {
                let segment = fmp4::convert(&file.bytes, part.t)
                    .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
                return Ok(Response::builder()
                    .header(header::CONTENT_TYPE, "video/mp4")
                    .header("x-codecs", segment.codecs)
                    .body(Body::from(segment.bytes))
                    .unwrap());
            }
            Ok(_) => return Err(bad("Unexpected live stream response")),
            Err(InvocationError::Rpc(r))
                if !r.name.starts_with("TIME_")
                    && !r.name.starts_with("GROUPCALL_")
                    && !r.name.starts_with("FLOOD_") =>
            {
                last = Some(InvocationError::Rpc(r))
            }
            Err(e) => return Err(rpc_error(e)),
        }
    }
    Err(rpc_error(last.expect("at least one quality")))
}

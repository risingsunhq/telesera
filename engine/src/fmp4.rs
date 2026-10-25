// Telegram live parts are complete one-second MP4 files. Media Source Extensions only accept
// fragmented MP4, so each part becomes an init segment plus one fragment, without re-encoding.
use anyhow::{Context, Result, bail, ensure};

// (type, whole box, payload)
type Mp4Box<'a> = ([u8; 4], &'a [u8], &'a [u8]);
fn boxes(data: &[u8]) -> Result<Vec<Mp4Box<'_>>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into()?) as usize;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into()?;
        let (header, size) = match size {
            0 => (8, data.len() - at),
            1 => {
                ensure!(at + 16 <= data.len(), "Truncated MP4 box");
                (
                    16,
                    u64::from_be_bytes(data[at + 8..at + 16].try_into()?) as usize,
                )
            }
            n => (8, n),
        };
        ensure!(size >= header && at + size <= data.len(), "Invalid MP4 box");
        out.push((kind, &data[at..at + size], &data[at + header..at + size]));
        at += size;
    }
    Ok(out)
}
fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data)
        .ok()?
        .into_iter()
        .find(|b| &b.0 == kind)
        .map(|b| b.2)
}
fn path<'a>(data: &'a [u8], kinds: &[&[u8; 4]]) -> Result<&'a [u8]> {
    kinds.iter().try_fold(data, |d, k| {
        child(d, k).with_context(|| format!("Missing MP4 box {}", String::from_utf8_lossy(*k)))
    })
}
fn be32(d: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        d.get(at..at + 4)
            .context("Truncated MP4 table")?
            .try_into()?,
    ))
}
fn table(full_box: &[u8], width: usize) -> Result<Vec<&[u8]>> {
    let count = be32(full_box, 4)? as usize;
    let body = full_box.get(8..).context("Truncated MP4 table")?;
    ensure!(body.len() >= count * width, "Truncated MP4 table");
    Ok(body.chunks(width).take(count).collect())
}
fn mp4box(kind: &[u8; 4], parts: &[&[u8]]) -> Vec<u8> {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(len + 8);
    out.extend(((len + 8) as u32).to_be_bytes());
    out.extend(kind);
    parts.iter().for_each(|p| out.extend(*p));
    out
}

struct Sample {
    offset: usize,
    size: u32,
    duration: u32,
    cts: i32,
    sync: bool,
}
struct Track {
    id: u32,
    timescale: u32,
    trak: Vec<u8>,
    samples: Vec<Sample>,
}
fn track(trak: &[u8]) -> Result<Track> {
    let tkhd = path(trak, &[b"tkhd"])?;
    let id = be32(tkhd, if tkhd[0] == 1 { 20 } else { 12 })?;
    let mdhd = path(trak, &[b"mdia", b"mdhd"])?;
    let timescale = be32(mdhd, if mdhd[0] == 1 { 20 } else { 12 })?;
    let stbl = path(trak, &[b"mdia", b"minf", b"stbl"])?;
    let stsz = path(stbl, &[b"stsz"])?;
    let fixed = be32(stsz, 4)?;
    let count = be32(stsz, 8)? as usize;
    let sizes: Vec<u32> = if fixed != 0 {
        vec![fixed; count]
    } else {
        (0..count)
            .map(|i| be32(stsz, 12 + i * 4))
            .collect::<Result<_>>()?
    };
    let mut durations = Vec::with_capacity(count);
    for e in table(path(stbl, &[b"stts"])?, 8)? {
        durations.extend(std::iter::repeat_n(be32(e, 4)?, be32(e, 0)? as usize));
    }
    let mut cts = Vec::with_capacity(count);
    if let Some(ctts) = child(stbl, b"ctts") {
        for e in table(ctts, 8)? {
            cts.extend(std::iter::repeat_n(
                be32(e, 4)? as i32,
                be32(e, 0)? as usize,
            ));
        }
    }
    let sync: Option<Vec<u32>> = child(stbl, b"stss")
        .map(|s| table(s, 4)?.into_iter().map(|e| be32(e, 0)).collect())
        .transpose()?;
    let chunks: Vec<u64> = match child(stbl, b"stco") {
        Some(stco) => table(stco, 4)?
            .into_iter()
            .map(|e| be32(e, 0).map(u64::from))
            .collect::<Result<_>>()?,
        None => table(path(stbl, &[b"co64"])?, 8)?
            .into_iter()
            .map(|e| Ok(u64::from_be_bytes(e.try_into()?)))
            .collect::<Result<_>>()?,
    };
    let stsc = table(path(stbl, &[b"stsc"])?, 12)?;
    let mut samples = Vec::with_capacity(count);
    for (chunk, start) in chunks.iter().enumerate() {
        let chunk = chunk as u32 + 1;
        let per_chunk = stsc
            .iter()
            .rev()
            .find(|e| be32(e, 0).is_ok_and(|first| first <= chunk))
            .map(|e| be32(e, 4))
            .transpose()?
            .unwrap_or(0);
        let mut offset = *start as usize;
        for _ in 0..per_chunk {
            let i = samples.len();
            let Some(&size) = sizes.get(i) else { break };
            samples.push(Sample {
                offset,
                size,
                duration: durations.get(i).copied().unwrap_or(0),
                cts: cts.get(i).copied().unwrap_or(0),
                sync: sync.as_ref().is_none_or(|s| s.contains(&(i as u32 + 1))),
            });
            offset += size as usize;
        }
    }
    ensure!(samples.len() == count, "Inconsistent MP4 sample tables");
    Ok(Track {
        id,
        timescale,
        trak: init_box(trak, b"trak")?,
        samples,
    })
}
// Copy the track description and replace its sample tables with empty ones.
fn init_box(data: &[u8], kind: &[u8; 4]) -> Result<Vec<u8>> {
    let mut parts = Vec::new();
    for (k, whole, inner) in boxes(data)? {
        match &k {
            b"edts" => {}
            b"mdia" | b"minf" => parts.push(init_box(inner, &k)?),
            b"stbl" => {
                let empty = [0u8; 8];
                parts.push(mp4box(
                    b"stbl",
                    &[
                        &mp4box(b"stsd", &[path(inner, &[b"stsd"])?]),
                        &mp4box(b"stts", &[&empty]),
                        &mp4box(b"stsc", &[&empty]),
                        &mp4box(b"stsz", &[&[0u8; 12]]),
                        &mp4box(b"stco", &[&empty]),
                    ],
                ))
            }
            _ => parts.push(whole.to_vec()),
        }
    }
    Ok(mp4box(
        kind,
        &parts.iter().map(Vec::as_slice).collect::<Vec<_>>(),
    ))
}
fn codec(track: &Track) -> Option<String> {
    let stsd = path(track.trak.get(8..)?, &[b"mdia", b"minf", b"stbl", b"stsd"]).ok()?;
    let entry = boxes(stsd.get(8..)?).ok()?.into_iter().next()?;
    match &entry.0 {
        // avc1 sample entry: 78 bytes of visual fields precede its child boxes.
        b"avc1" => {
            let avcc = child(entry.2.get(78..)?, b"avcC")?;
            Some(format!("avc1.{}", hex::encode(avcc.get(1..4)?)))
        }
        // shortcut: assumes AAC-LC signalling, read the esds object type if HE-AAC streams fail.
        b"mp4a" => Some("mp4a.40.2".into()),
        _ => None,
    }
}

pub struct Segment {
    pub bytes: Vec<u8>,
    pub codecs: String,
}
/// `part` is a Telegram stream part; `time_ms` positions it on the live timeline.
pub fn convert(part: &[u8], time_ms: i64) -> Result<Segment> {
    // The Telegram part header precedes a regular MP4 file that starts with ftyp.
    let start = part
        .windows(4)
        .take(512)
        .position(|w| w == b"ftyp")
        .and_then(|p| p.checked_sub(4))
        .context("Unsupported live stream part")?;
    let mp4 = &part[start..];
    let moov = path(mp4, &[b"moov"])?;
    let tracks: Vec<Track> = boxes(moov)?
        .into_iter()
        .filter(|b| &b.0 == b"trak")
        .map(|b| track(b.2))
        .collect::<Result<_>>()?;
    if tracks.is_empty() {
        bail!("Live stream part has no tracks");
    }
    let codecs: Vec<String> = tracks.iter().filter_map(codec).collect();
    ensure!(
        codecs.len() == tracks.len(),
        "Unsupported live stream codec"
    );

    let ftyp = mp4box(b"ftyp", &[b"isom\0\0\x02\0isomiso6mp41"]);
    let mut moov_parts = vec![mp4box(b"mvhd", &[path(moov, &[b"mvhd"])?])];
    let mut trex = Vec::new();
    for t in &tracks {
        moov_parts.push(t.trak.clone());
        trex.extend(mp4box(
            b"trex",
            &[&[0; 4], &t.id.to_be_bytes(), &1u32.to_be_bytes(), &[0; 12]],
        ));
    }
    moov_parts.push(mp4box(b"mvex", &[&trex]));
    let init_moov = mp4box(
        b"moov",
        &moov_parts.iter().map(Vec::as_slice).collect::<Vec<_>>(),
    );

    // Build the fragment twice: the first pass measures moof so trun data offsets are exact.
    let fragment = |moof_len: usize| -> Result<(Vec<u8>, Vec<u8>)> {
        let mut trafs = Vec::new();
        let mut mdat = Vec::new();
        for t in &tracks {
            let mut trun = vec![1, 0, 0x0f, 0x01];
            trun.extend((t.samples.len() as u32).to_be_bytes());
            trun.extend(((moof_len + 8 + mdat.len()) as u32).to_be_bytes());
            for s in &t.samples {
                let flags: u32 = if s.sync { 0x0200_0000 } else { 0x0101_0000 };
                trun.extend(s.duration.to_be_bytes());
                trun.extend(s.size.to_be_bytes());
                trun.extend(flags.to_be_bytes());
                trun.extend(s.cts.to_be_bytes());
                mdat.extend(
                    mp4.get(s.offset..s.offset + s.size as usize)
                        .context("MP4 sample outside the file")?,
                );
            }
            let decode = time_ms.max(0) as u64 * u64::from(t.timescale) / 1000;
            trafs.push(mp4box(
                b"traf",
                &[
                    &mp4box(b"tfhd", &[&[0, 2, 0, 0], &t.id.to_be_bytes()]),
                    &mp4box(b"tfdt", &[&[1, 0, 0, 0], &decode.to_be_bytes()]),
                    &mp4box(b"trun", &[&trun]),
                ],
            ));
        }
        let sequence = ((time_ms / 1000) as u32).to_be_bytes();
        let mut moof = vec![mp4box(b"mfhd", &[&[0; 4], &sequence])];
        moof.extend(trafs);
        Ok((
            mp4box(b"moof", &moof.iter().map(Vec::as_slice).collect::<Vec<_>>()),
            mp4box(b"mdat", &[&mdat]),
        ))
    };
    let (moof, _) = fragment(0)?;
    let (moof, mdat) = fragment(moof.len())?;
    Ok(Segment {
        bytes: [ftyp, init_moov, moof, mdat].concat(),
        codecs: codecs.join(","),
    })
}

#[cfg(test)]
mod tests {
    // Fixture: 0.4 s synthetic H.264 + AAC (ffmpeg testsrc/sine) behind a 32-byte part header.
    const PART: &[u8] = include_bytes!("../tests/live-part.bin");
    #[test]
    fn converts_part_to_fragment() {
        let segment = super::convert(PART, 5000).unwrap();
        let top: Vec<_> = super::boxes(&segment.bytes)
            .unwrap()
            .into_iter()
            .map(|b| b.0)
            .collect();
        assert_eq!(top, [*b"ftyp", *b"moov", *b"moof", *b"mdat"]);
        assert!(segment.codecs.starts_with("avc1.") && segment.codecs.ends_with(",mp4a.40.2"));
        let moov = super::path(&segment.bytes, &[b"moov"]).unwrap();
        assert!(super::child(moov, b"mvex").is_some());
        let moof = super::path(&segment.bytes, &[b"moof"]).unwrap();
        let tfdt = super::path(moof, &[b"traf", b"tfdt"]).unwrap();
        assert_eq!(
            u64::from_be_bytes(tfdt[4..12].try_into().unwrap()),
            5 * 12800
        );
        assert!(super::convert(&PART[40..], 0).is_err());
    }
}

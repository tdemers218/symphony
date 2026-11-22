//! Movie export: H.264 video in an MP4 file, encoded with Cisco's OpenH264
//! binary. Like Harmony, Symphony never ships the codec. The user downloads
//! Cisco's own binary separately (see docs/CODECS.md), and this module loads
//! it from the path it's given. `openh264` refuses any file whose SHA-256
//! isn't a known Cisco build.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use mp4::{
    AvcConfig, FourCC, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig, TrackType,
};
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode, UsageType,
};
use openh264::formats::{RgbaSliceU8, YUVBuffer};
use openh264::OpenH264API;
use symphony_core::doc::Scene;

use crate::render_frame;

/// H.264 NAL unit types we care about.
const NAL_IDR: u8 = 5;
const NAL_SPS: u8 = 7;
const NAL_PPS: u8 = 8;

/// Split an Annex B byte stream into NAL units without their start codes.
pub fn split_annex_b(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut units = Vec::with_capacity(starts.len());
    for (k, &s) in starts.iter().enumerate() {
        let mut end = starts.get(k + 1).map_or(stream.len(), |next| next - 3);
        // A 4-byte start code leaves one zero at the end of the previous unit.
        while end > s && stream[end - 1] == 0 {
            end -= 1;
        }
        if end > s {
            units.push(&stream[s..end]);
        }
    }
    units
}

pub fn output_frame_count(source_frames: u32, source_fps: f32, output_fps: f32) -> u32 {
    (f64::from(source_frames) * f64::from(output_fps) / f64::from(source_fps.max(1.0)))
        .round()
        .max(1.0) as u32
}

/// Encode frames `first..=last` of `scene` at `w` x `h` into an MP4 at `out`.
#[allow(clippy::too_many_arguments)]
pub fn export_mp4(
    scene: &Scene,
    codec: &Path,
    out: &Path,
    first: u32,
    last: u32,
    w: u32,
    h: u32,
    movie_fps: Option<f32>,
    cancel: &AtomicBool,
    progress: &dyn Fn(u32, u32),
) -> Result<(), String> {
    // 4:2:0 video needs even sizes.
    let (w, h) = ((w & !1).max(2), (h & !1).max(2));
    let source_fps = scene.settings.fps.max(1.0);
    let fps = movie_fps.unwrap_or(source_fps);
    if !source_fps.is_finite() || !fps.is_finite() || !(1.0..=120.0).contains(&fps) {
        return Err("Choose a valid movie frame rate.".into());
    }
    let api = OpenH264API::from_blob_path(codec).map_err(|e| {
        format!("Couldn't load the OpenH264 codec ({e}). Download it again in Preferences > Video.")
    })?;
    // Animation is flat colour with sharp lines, so aim for a generous rate.
    // (OpenH264 only accepts its real-time usage modes; screen content suits
    // flat colour best.)
    let bps = ((w * h) as f32 * fps * 0.12) as u32;
    let config = EncoderConfig::new()
        .max_frame_rate(FrameRate::from_hz(fps))
        .bitrate(BitRate::from_bps(bps.max(500_000)))
        .rate_control_mode(RateControlMode::Quality)
        .usage_type(UsageType::ScreenContentRealTime)
        .adaptive_quantization(false)
        .background_detection(false)
        .skip_frames(false)
        .intra_frame_period(IntraFramePeriod::from_num_frames((fps * 2.0).round() as u32));
    let mut encoder = Encoder::with_api_config(api, config)
        .map_err(|e| format!("The codec wouldn't start ({e})."))?;

    let file =
        File::create(out).map_err(|e| format!("Couldn't create {} ({e}).", out.display()))?;
    let mut writer: Option<Mp4Writer<BufWriter<File>>> = None;
    let mut file = Some(BufWriter::new(file));
    // Timescale ticks per frame, so fractional rates (23.976) stay close.
    let per_frame = 100u32;
    let timescale = (fps * per_frame as f32).round() as u32;
    let total = output_frame_count(last - first + 1, source_fps, fps);

    for i in 0..total {
        let offset = (f64::from(i) * f64::from(source_fps) / f64::from(fps)).floor() as u32;
        let f = first.saturating_add(offset).min(last);
        if cancel.load(Ordering::SeqCst) {
            return Err("Export cancelled.".into());
        }
        let img = render_frame(scene, f, w, h).ok_or("Couldn't render a frame.")?;
        let yuv =
            YUVBuffer::from_rgb_source(RgbaSliceU8::new(img.as_raw(), (w as usize, h as usize)));
        let stream = encoder
            .encode(&yuv)
            .map_err(|e| format!("Couldn't encode frame {f} ({e})."))?
            .to_vec();

        let (mut sps, mut pps, mut sample, mut key) = (None, None, Vec::new(), false);
        for nal in split_annex_b(&stream) {
            match nal[0] & 0x1f {
                NAL_SPS => sps = Some(nal.to_vec()),
                NAL_PPS => pps = Some(nal.to_vec()),
                t => {
                    key |= t == NAL_IDR;
                    sample.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                    sample.extend_from_slice(nal);
                }
            }
        }
        if writer.is_none() {
            let (Some(sps), Some(pps)) = (sps, pps) else {
                return Err("The codec didn't start the video properly.".into());
            };
            let config = Mp4Config {
                major_brand: FourCC::from(*b"isom"),
                minor_version: 512,
                compatible_brands: [b"isom", b"iso2", b"avc1", b"mp41"]
                    .iter()
                    .map(|b| FourCC::from(**b))
                    .collect(),
                timescale: 1000,
            };
            let mut wr = Mp4Writer::write_start(file.take().expect("file is used once"), &config)
                .map_err(|e| format!("Couldn't start the movie file ({e})."))?;
            wr.add_track(&TrackConfig {
                track_type: TrackType::Video,
                timescale,
                language: "und".into(),
                media_conf: MediaConfig::AvcConfig(AvcConfig {
                    width: w as u16,
                    height: h as u16,
                    seq_param_set: sps,
                    pic_param_set: pps,
                }),
            })
            .map_err(|e| format!("Couldn't add the video track ({e})."))?;
            writer = Some(wr);
        }
        if !sample.is_empty() {
            let s = Mp4Sample {
                start_time: i as u64 * per_frame as u64,
                duration: per_frame,
                rendering_offset: 0,
                is_sync: key,
                bytes: sample.into(),
            };
            if let Some(wr) = writer.as_mut() {
                wr.write_sample(1, &s)
                    .map_err(|e| format!("Couldn't write frame {f} ({e})."))?;
            }
        }
        progress(i + 1, total);
    }
    if let Some(mut wr) = writer {
        wr.write_end()
            .map_err(|e| format!("Couldn't finish the movie file ({e})."))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::split_annex_b;

    #[test]
    fn splits_three_and_four_byte_start_codes() {
        let s = [
            0, 0, 0, 1, 0x67, 9, 9, 0, 0, 1, 0x68, 8, 0, 0, 0, 1, 0x65, 7, 7,
        ];
        let units = split_annex_b(&s);
        assert_eq!(
            units,
            vec![&[0x67, 9, 9][..], &[0x68, 8][..], &[0x65, 7, 7][..]]
        );
    }
}

//! MP3 length, from the frame headers (the public MPEG audio format): enough
//! to time voiced lines without decoding them.

/// The length in seconds of an MPEG-1/2/2.5 Layer III stream, or `None` if no
/// frames are found.
pub fn duration(bytes: &[u8]) -> Option<f32> {
    let mut i = skip_id3v2(bytes);
    let mut seconds = 0.0f64;
    let mut frames = 0;
    while i + 4 <= bytes.len() {
        let Some((length, samples, rate)) = frame(&bytes[i..i + 4]) else {
            i += 1;
            continue;
        };
        seconds += f64::from(samples) / f64::from(rate);
        frames += 1;
        i += length;
    }
    (frames > 0).then_some(seconds as f32)
}

fn skip_id3v2(bytes: &[u8]) -> usize {
    if bytes.len() >= 10 && &bytes[..3] == b"ID3" {
        let size = bytes[6..10]
            .iter()
            .fold(0usize, |n, &b| (n << 7) | usize::from(b & 0x7F));
        10 + size
    } else {
        0
    }
}

/// A Layer III frame header's frame length, samples and sample rate.
fn frame(h: &[u8]) -> Option<(usize, u32, u32)> {
    if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (h[1] >> 3) & 0x3; // 3: MPEG-1, 2: MPEG-2, 0: MPEG-2.5
    let layer = (h[1] >> 1) & 0x3; // 1: Layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let bitrate_index = usize::from(h[2] >> 4);
    let rate_index = usize::from((h[2] >> 2) & 0x3);
    if bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return None;
    }
    let padding = usize::from((h[2] >> 1) & 0x1);
    const V1: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const V2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let (bitrate, rate, samples, factor) = match version {
        3 => (
            V1[bitrate_index],
            [44100, 48000, 32000][rate_index],
            1152,
            144,
        ),
        2 => (
            V2[bitrate_index],
            [22050, 24000, 16000][rate_index],
            576,
            72,
        ),
        _ => (V2[bitrate_index], [11025, 12000, 8000][rate_index], 576, 72),
    };
    let length = (factor * bitrate * 1000 / rate) as usize + padding;
    (length >= 4).then_some((length, samples, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_frames() {
        // Two MPEG-1 Layer III frames at 128 kbit/s, 44.1 kHz: 417 bytes each.
        let mut bytes = Vec::new();
        for _ in 0..2 {
            let mut f = vec![0u8; 417];
            f[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            bytes.extend(f);
        }
        let d = duration(&bytes).unwrap();
        assert!((d - 2.0 * 1152.0 / 44100.0).abs() < 1e-4, "{d}");
        assert_eq!(duration(b"not audio"), None);
    }
}

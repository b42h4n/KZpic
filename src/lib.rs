use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
};

const MAGIC: &[u8; 5] = b"KZPIC";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 18;

const MODE_RAW: u8 = 0;
const MODE_PALETTE: u8 = 1;
const MODE_PREDICT: u8 = 2;

const FLAG_BLUR: u8 = 1;

#[derive(Clone, Copy)]
pub struct Opts {
    pub quant: u8,
    pub palette: bool,
    pub predict: bool,
    pub blur: bool,
    pub upscale: u8,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            quant: 8,
            palette: true,
            predict: true,
            blur: false,
            upscale: 3,
        }
    }
}

fn quantize(data: &mut [u8], q: u8) {
    if q <= 1 {
        return;
    }
    let q = q as u16;
    for v in data.iter_mut() {
        *v = (((*v as u16 + q / 2) / q) * q).min(255) as u8;
    }
}

fn med(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn pred_at(rec: &[u8], w: usize, x: usize, y: usize, ch: usize) -> i32 {
    let i = (y * w + x) * 3 + ch;
    let a = if x > 0 { rec[i - 3] as i32 } else { 0 };
    let b = if y > 0 { rec[i - w * 3] as i32 } else { 0 };
    let c = if x > 0 && y > 0 {
        rec[i - w * 3 - 3] as i32
    } else {
        0
    };
    med(a, b, c)
}

fn wrap(d: i32) -> i32 {
    (d + 128).rem_euclid(256) - 128
}

fn predict_encode(src: &[u8], w: usize, h: usize, q: i32) -> Vec<u8> {
    let mut rec = vec![0u8; src.len()];
    let mut out = Vec::with_capacity(src.len());
    for y in 0..h {
        for x in 0..w {
            for ch in 0..3 {
                let i = (y * w + x) * 3 + ch;
                let pred = pred_at(&rec, w, x, y, ch);
                let diff = wrap(src[i] as i32 - pred);
                let rq = (diff as f32 / q as f32).round() as i32;
                rec[i] = (pred + rq * q).rem_euclid(256) as u8;
                out.push(rq as i8 as u8);
            }
        }
    }
    out
}

fn predict_decode(res: &[u8], w: usize, h: usize, q: i32) -> Vec<u8> {
    let mut rec = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            for ch in 0..3 {
                let i = (y * w + x) * 3 + ch;
                let pred = pred_at(&rec, w, x, y, ch);
                let rq = res[i] as i8 as i32;
                rec[i] = (pred + rq * q).rem_euclid(256) as u8;
            }
        }
    }
    rec
}

fn palette_encode(px: &[u8]) -> Option<Vec<u8>> {
    let mut map: HashMap<[u8; 3], u32> = HashMap::new();
    let mut pal: Vec<[u8; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::with_capacity(px.len() / 3);
    for c in px.chunks_exact(3) {
        let key = [c[0], c[1], c[2]];
        let id = *map.entry(key).or_insert_with(|| {
            pal.push(key);
            (pal.len() - 1) as u32
        });
        idx.push(id);
    }
    if pal.len() > 65536 {
        return None;
    }
    let mut out = Vec::new();
    out.extend_from_slice(&(pal.len() as u32).to_le_bytes());
    for c in &pal {
        out.extend_from_slice(c);
    }
    let wide = pal.len() > 256;
    for i in idx {
        if wide {
            out.extend_from_slice(&(i as u16).to_le_bytes());
        } else {
            out.push(i as u8);
        }
    }
    Some(out)
}

fn palette_decode(buf: &[u8], npix: usize) -> Result<Vec<u8>, String> {
    if buf.len() < 4 {
        return Err("Invalid palette data".into());
    }
    let n = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let pal_end = 4 + n * 3;
    if buf.len() < pal_end {
        return Err("Invalid palette data".into());
    }
    let pal = &buf[4..pal_end];
    let body = &buf[pal_end..];
    let wide = n > 256;
    let step = if wide { 2 } else { 1 };
    if body.len() < npix * step {
        return Err("Not enough pixel data".into());
    }
    let mut out = Vec::with_capacity(npix * 3);
    for p in 0..npix {
        let id = if wide {
            u16::from_le_bytes([body[p * 2], body[p * 2 + 1]]) as usize
        } else {
            body[p] as usize
        };
        if id >= n {
            return Err("Palette index out of bounds".into());
        }
        out.extend_from_slice(&pal[id * 3..id * 3 + 3]);
    }
    Ok(out)
}

fn blur(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    const K: [[u32; 3]; 3] = [[1, 2, 1], [2, 4, 2], [1, 2, 1]];
    let mut out = src.to_vec();
    for y in 0..h {
        for x in 0..w {
            for ch in 0..3 {
                let mut sum = 0u32;
                let mut wsum = 0u32;
                for (dy, row) in K.iter().enumerate() {
                    for (dx, k) in row.iter().enumerate() {
                        let nx = x as i32 + dx as i32 - 1;
                        let ny = y as i32 + dy as i32 - 1;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        sum += src[(ny as usize * w + nx as usize) * 3 + ch] as u32 * k;
                        wsum += k;
                    }
                }
                let i = (y * w + x) * 3 + ch;
                let blurred = sum / wsum;
                out[i] = ((src[i] as u32 + blurred) / 2) as u8;
            }
        }
    }
    out
}

fn upscale(src: &[u8], w: usize, h: usize, n: usize) -> Vec<u8> {
    if n <= 1 {
        return src.to_vec();
    }
    let (ow, oh) = (w * n, h * n);
    let mut out = vec![0u8; ow * oh * 3];
    let get = |x: i32, y: i32, ch: usize| -> f32 {
        let x = x.clamp(0, w as i32 - 1) as usize;
        let y = y.clamp(0, h as i32 - 1) as usize;
        src[(y * w + x) * 3 + ch] as f32
    };
    for oy in 0..oh {
        let sy = (oy as f32 + 0.5) / n as f32 - 0.5;
        let y0 = sy.floor() as i32;
        let fy = sy - y0 as f32;
        for ox in 0..ow {
            let sx = (ox as f32 + 0.5) / n as f32 - 0.5;
            let x0 = sx.floor() as i32;
            let fx = sx - x0 as f32;
            for ch in 0..3 {
                let top = get(x0, y0, ch) * (1.0 - fx) + get(x0 + 1, y0, ch) * fx;
                let bot = get(x0, y0 + 1, ch) * (1.0 - fx) + get(x0 + 1, y0 + 1, ch) * fx;
                out[(oy * ow + ox) * 3 + ch] = (top * (1.0 - fy) + bot * fy).round() as u8;
            }
        }
    }
    out
}

pub fn encode(path_in: &str, o: Opts) -> Result<Vec<u8>, String> {
    let img = image::open(path_in)
        .map_err(|e| format!("Failed to open {path_in}: {e}"))?
        .to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut px = img.into_raw();
    quantize(&mut px, o.quant);

    let (mode, payload) = match (o.palette, o.predict) {
        (true, _) => match palette_encode(&px) {
            Some(p) => (MODE_PALETTE, p),
            None if o.predict => (
                MODE_PREDICT,
                predict_encode(&px, w, h, o.quant.max(1) as i32),
            ),
            None => (MODE_RAW, px),
        },
        (false, true) => (
            MODE_PREDICT,
            predict_encode(&px, w, h, o.quant.max(1) as i32),
        ),
        (false, false) => (MODE_RAW, px),
    };

    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&payload).map_err(|e| e.to_string())?;
    let packed = enc.finish().map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(HEADER_LEN + packed.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(mode);
    out.push(if o.blur { FLAG_BLUR } else { 0 });
    out.push(o.quant);
    out.push(o.upscale);
    out.extend_from_slice(&(w as u32).to_le_bytes());
    out.extend_from_slice(&(h as u32).to_le_bytes());
    out.extend_from_slice(&packed);
    Ok(out)
}

pub fn decode_to_rgb(
    path_in: &str,
    ov_blur: Option<bool>,
    ov_up: Option<u8>,
) -> Result<(usize, usize, Vec<u8>), String> {
    let buf = fs::read(path_in).map_err(|e| format!("Failed to read {path_in}: {e}"))?;
    if buf.len() < HEADER_LEN || &buf[..5] != MAGIC {
        return Err("This is not a .kzpic file".into());
    }
    if buf[5] != VERSION {
        return Err(format!("Unsupported version {}", buf[5]));
    }
    let (mode, flags, quant, up) = (buf[6], buf[7], buf[8].max(1), buf[9]);
    let w = u32::from_le_bytes([buf[10], buf[11], buf[12], buf[13]]) as usize;
    let h = u32::from_le_bytes([buf[14], buf[15], buf[16], buf[17]]) as usize;

    let mut payload = Vec::new();
    DeflateDecoder::new(&buf[HEADER_LEN..])
        .read_to_end(&mut payload)
        .map_err(|e| format!("Corrupted compressed stream: {e}"))?;

    let mut px = match mode {
        MODE_RAW => payload,
        MODE_PALETTE => palette_decode(&payload, w * h)?,
        MODE_PREDICT => {
            if payload.len() != w * h * 3 {
                return Err("Invalid image data size".into());
            }
            predict_decode(&payload, w, h, quant as i32)
        }
        _ => return Err("Unknown encoding mode".into()),
    };
    if px.len() != w * h * 3 {
        return Err("Invalid image data size".into());
    }

    if ov_blur.unwrap_or(flags & FLAG_BLUR != 0) {
        px = blur(&px, w, h);
    }
    let n = ov_up.unwrap_or(up).max(1) as usize;
    Ok((w * n, h * n, upscale(&px, w, h, n)))
}

pub fn decode(
    path_in: &str,
    path_out: &str,
    ov_blur: Option<bool>,
    ov_up: Option<u8>,
) -> Result<(), String> {
    let (w, h, px) = decode_to_rgb(path_in, ov_blur, ov_up)?;
    image::RgbImage::from_raw(w as u32, h as u32, px)
        .ok_or("Failed to construct image")?
        .save(path_out)
        .map_err(|e| format!("Failed to save {path_out}: {e}"))
}

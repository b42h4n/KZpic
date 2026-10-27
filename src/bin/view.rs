use minifb::{Key, ScaleMode, Window, WindowOptions};
use std::{env, process};

const BG: u32 = 0x0018_1818;

/// Fits the image (w x h) into a window-sized frame (ww x wh),
/// keeping the aspect ratio and centering it.
fn fit(src: &[u32], w: usize, h: usize, ww: usize, wh: usize, out: &mut Vec<u32>) {
    out.clear();
    out.resize(ww * wh, BG);
    if ww == 0 || wh == 0 {
        return;
    }
    let s = (ww as f32 / w as f32).min(wh as f32 / h as f32);
    let dw = ((w as f32 * s) as usize).clamp(1, ww);
    let dh = ((h as f32 * s) as usize).clamp(1, wh);
    let (ox, oy) = ((ww - dw) / 2, (wh - dh) / 2);
    for y in 0..dh {
        let sy = y * h / dh;
        for x in 0..dw {
            let sx = x * w / dw;
            out[(oy + y) * ww + ox + x] = src[sy * w + sx];
        }
    }
}

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("Usage: kzpic-view <file.kzpic>");
        process::exit(1);
    };

    let (w, h, rgb) = kzpic::decode_to_rgb(&path, None, None).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        process::exit(1);
    });

    let buf: Vec<u32> = rgb
        .chunks_exact(3)
        .map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
        .collect();

    // initial window size: fit into 1280x800, never enlarge
    let k = (1280.0 / w as f32).min(800.0 / h as f32).min(1.0);
    let (ww, wh) = (
        ((w as f32 * k) as usize).max(1),
        ((h as f32 * k) as usize).max(1),
    );

    let mut win = Window::new(
        &format!("kzpic - {path}"),
        ww,
        wh,
        WindowOptions {
            resize: true,
            // no scaling inside minifb: the frame always matches the window
            scale_mode: ScaleMode::UpperLeft,
            ..WindowOptions::default()
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("Window error: {e}");
        process::exit(1);
    });
    win.set_target_fps(30);

    let mut frame: Vec<u32> = Vec::new();
    let mut size = (0, 0);

    while win.is_open() && !win.is_key_down(Key::Escape) {
        let (cw, ch) = win.get_size();
        if cw == 0 || ch == 0 {
            win.update();
            continue;
        }
        if (cw, ch) != size {
            size = (cw, ch);
            fit(&buf, w, h, cw, ch, &mut frame);
        }
        if let Err(e) = win.update_with_buffer(&frame, cw, ch) {
            eprintln!("Window error: {e}");
            break;
        }
    }
}

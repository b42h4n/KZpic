use kzpic::{Opts, decode, encode};
use std::{env, fs, process};

const HELP: &str = "kzpic — .kzpic image format

kzpic encode <in.png|jpg|webp> <out.kzpic> [options]
--quant N      Quantization step 1..64 (1 = lossless, default: 8)
--no-palette   Disable color palette encoding
--no-predict   Disable color prediction
--blur         Store the edge-blurring option in the file
--upscale N    Store an NxN pixel grid (3 = 9 pixels per source pixel), default: 3

kzpic decode <in.kzpic> <out.png> [options]
--blur / --no-blur   Force blur on or off
--upscale N          Force the output scale factor";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 3 {
        println!("{HELP}");
        process::exit(1);
    }
    let cmd = args[0].as_str();
    let (inp, out) = (&args[1], &args[2]);
    let rest = &args[3..];

    let mut o = Opts::default();
    let (mut ov_blur, mut ov_up): (Option<bool>, Option<u8>) = (None, None);
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--quant" => {
                i += 1;
                o.quant = rest
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(8)
                    .clamp(1, 64);
            }
            "--upscale" => {
                i += 1;
                let n: u8 = rest
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1)
                    .clamp(1, 16);
                o.upscale = n;
                ov_up = Some(n);
            }
            "--no-palette" => o.palette = false,
            "--no-predict" => o.predict = false,
            "--blur" => {
                o.blur = true;
                ov_blur = Some(true);
            }
            "--no-blur" => ov_blur = Some(false),
            other => {
                eprintln!("Unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    let res = match cmd {
        "encode" => encode(inp, o).and_then(|data| {
            let len = data.len();
            fs::write(out, data).map_err(|e| e.to_string())?;
            println!("Done: {len} bytes");
            Ok(())
        }),
        "decode" => decode(inp, out, ov_blur, ov_up),
        _ => Err(format!("Unknown command: {cmd}")),
    };
    if let Err(e) = res {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

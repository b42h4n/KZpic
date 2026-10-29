<p align="center">
  <h1 align="center">KZpic</h1>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust-orange?logo=rust&style=for-the-badge" alt="Rust" />
  <img src="https://img.shields.io/badge/License-MIT-green?style=for-the-badge" alt="MIT" />
</p>
.kzpic image format, with a command-line tool and a minimal image viewer, written in Rust.

The format combines ideas from PNG, JPEG and WebP:

| Idea | Borrowed from | How it is done |
|------|---------------|----------------|
| Short references to identical and similar colors | PNG | Palette: each color is stored once, pixels store only an index |
| Imprecise color encoding | JPEG | Channel quantization with a configurable step |
| Predicting color from neighbors | WebP | MED predictor, only the prediction error is stored |
| Edge blurring | Own idea | 3×3 filter applied when the file is opened |
| NxN grid around every pixel | Own idea | Bilinear upscaling applied when the file is opened |

Every stage can be turned on or off, and the compression level is adjustable.

> The format may change between versions.

## Building

You need [Rust](https://rustup.rs) 1.85 or newer (the project uses the 2024 edition).

```bash
git clone https://github.com/b42h4n/kzpic
cd kzpic

cargo build --release                    # command-line tool only
cargo build --release --features viewer  # command-line tool + viewer
```

Binaries are placed in `target/release/`: `kzpic` and, with the `viewer` feature, `kzpic-view`.

To install them system-wide:

```bash
cargo install --path .                    # kzpic
cargo install --path . --features viewer  # kzpic and kzpic-view
```

The viewer is optional so the command-line tool builds without any GUI dependencies. On Linux the viewer needs the usual X11 or Wayland development libraries.

## Usage

### Encoding

```bash
kzpic encode <in.png|jpg|webp> <out.kzpic> [options]
```

| Option | Description |
|--------|-------------|
| `--quant N` | Quantization step from 1 to 64. `1` is lossless, default is `8` |
| `--no-palette` | Disable color references |
| `--no-predict` | Disable color prediction |
| `--blur` | Store a flag in the file to blur edges when it is opened |
| `--upscale N` | Store the NxN grid size per pixel in the file (1 to 16), default is `3` |

Input and output paths must come before the options.

### Decoding

```bash
kzpic decode <in.kzpic> <out.png> [options]
```

| Option | Description |
|--------|-------------|
| `--blur` / `--no-blur` | Force blurring on or off |
| `--upscale N` | Force the grid size (`1` means no upscaling) |

Without options, the settings stored in the file are used. By default that means the output is **3 times wider and 3 times taller** than the source image.

### Viewing

```bash
kzpic-view <file.kzpic>
```

Opens the file in a resizable window (the aspect ratio is kept). Press `Esc` to close it. The viewer uses the settings stored in the file, so a file encoded with the default `--upscale 3` is shown enlarged.

### Examples

```bash
# strong compression with edge blurring
kzpic encode photo.jpg photo.kzpic --quant 16 --blur

# lossless, prediction only, no upscaling
kzpic encode photo.png photo.kzpic --quant 1 --no-palette --upscale 1

# open without upscaling
kzpic decode photo.kzpic out.png --upscale 1

# look at the result
kzpic-view photo.kzpic
```

## Project layout

```
kzpic/
├── Cargo.toml
├── src/
│   ├── lib.rs        # the format: encode, decode, palette, prediction, blur, upscale
│   ├── main.rs       # kzpic command-line tool
│   └── bin/
│       └── view.rs   # kzpic-view window viewer (feature "viewer")
```

All format logic lives in the library, so the command-line tool and the viewer share exactly the same decoder.

## Using as a library

```toml
[dependencies]
kzpic = { git = "https://github.com/b42h4n/kzpic" }
```

```rust
// RGB8 pixels (w * h * 3 bytes) with blur and upscale already applied
let (w, h, rgb) = kzpic::decode_to_rgb("photo.kzpic", None, None)?;
```

Public API:

- `encode(path, Opts) -> Result<Vec<u8>, String>` — returns the bytes of a `.kzpic` file
- `decode(path_in, path_out, blur, upscale)` — decodes to an image file
- `decode_to_rgb(path, blur, upscale)` — decodes to a pixel buffer

The `blur` and `upscale` arguments are `Option` values: `None` means "use what is stored in the file".

## How it works

### Encoding

1. **Quantization.** Each channel (R, G, B) is rounded to the nearest multiple of the `--quant` step. A larger step means fewer distinct colors and better compression, but lower accuracy.
2. **Palette.** After quantization, identical colors are collected into a table, and each pixel stores a short index (1 byte for up to 256 colors, otherwise 2 bytes). If there are more than 65,536 colors, the palette is not used.
3. **Prediction.** If the palette is disabled or not applicable, each pixel's color is predicted from three neighbors (left, above, above-left). The quantized difference is written to the file. Prediction runs on already reconstructed pixels, so errors do not accumulate. A file uses either the palette or prediction, never both; the palette takes priority.
4. **Deflate.** The resulting stream is compressed once more.

### Decoding

5. **Blur.** A 3×3 filter is blended 50/50 with the original to smooth out quantization steps.
6. **NxN grid.** Each pixel becomes an NxN block whose colors blend smoothly toward the neighboring pixels. With `N = 3` the image has 9 times as many pixels (3 times wider and 3 times taller).

The grid is built when the file is opened and is not stored inside it, so the `.kzpic` size does not grow because of it.

## File format

The header is 18 bytes, all numbers are little-endian:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 5 | Signature `KZPIC` |
| 5 | 1 | Format version (currently `1`) |
| 6 | 1 | Mode: `0` raw data, `1` palette, `2` prediction |
| 7 | 1 | Flags: bit 0 is blur on open |
| 8 | 1 | Quantization step |
| 9 | 1 | Grid size on open |
| 10 | 4 | Width of the stored image (before upscaling), `u32` |
| 14 | 4 | Height of the stored image (before upscaling), `u32` |
| 18 | — | Deflate-compressed data |

The data layout depends on the mode:

- **Mode 0:** quantized RGB bytes in sequence.
- **Mode 1:** color count (`u32`), the palette (RGB), then a color index for every pixel.
- **Mode 2:** signed prediction-error bytes for every channel of every pixel.

## Limitations

- RGB only, no transparency (the alpha channel is dropped).
- A palette is only effective for photos with noticeable quantization (`--quant` of 8 or more).
- The grid upscaling does not add detail; it is a smoothed enlargement.

## Dependencies

- [`image`](https://crates.io/crates/image) — reading PNG, JPEG, WebP and writing PNG
- [`flate2`](https://crates.io/crates/flate2) — deflate
- [`minifb`](https://crates.io/crates/minifb) — window for the viewer (optional, feature `viewer`)

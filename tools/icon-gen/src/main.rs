//! 用法：`cargo run -p icon-gen`（在仓库根目录）
//!
//! 输入 `assets/icon/mdeasy.svg`，输出：
//! - `assets/icon/png/{16..1024}.png`
//! - `assets/icon/MDEasy.iconset/`（macOS 上再用 `iconutil -c icns` 生成 .icns）
//! - `assets/icon/mdeasy.ico`（Windows，内嵌 PNG）

use std::{fs, path::Path};

use anyhow::{Context, Result};
use resvg::{tiny_skia, usvg};

fn render(tree: &usvg::Tree, size: u32) -> Result<Vec<u8>> {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).context("创建画布失败")?;
    let scale = size as f32 / tree.size().width();
    resvg::render(tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

/// ICO 文件：头 + 目录项 + PNG 数据（Vista 起支持 PNG 内嵌）。
fn build_ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bpp
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}

fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon");
    let svg = fs::read(root.join("mdeasy.svg")).context("读取 mdeasy.svg")?;
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default())?;

    let png_dir = root.join("png");
    fs::create_dir_all(&png_dir)?;
    for size in [16, 24, 32, 48, 64, 128, 256, 512, 1024] {
        fs::write(png_dir.join(format!("{size}.png")), render(&tree, size)?)?;
    }

    let iconset = root.join("MDEasy.iconset");
    fs::create_dir_all(&iconset)?;
    for base in [16, 32, 128, 256, 512] {
        fs::write(iconset.join(format!("icon_{base}x{base}.png")), render(&tree, base)?)?;
        fs::write(
            iconset.join(format!("icon_{base}x{base}@2x.png")),
            render(&tree, base * 2)?,
        )?;
    }

    let ico: Vec<_> = [16, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|s| render(&tree, s).map(|png| (s, png)))
        .collect::<Result<_>>()?;
    fs::write(root.join("mdeasy.ico"), build_ico(&ico))?;

    println!("图标已生成到 {}", root.canonicalize()?.display());
    Ok(())
}

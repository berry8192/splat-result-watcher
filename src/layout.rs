//! 配信の出力の中で、ゲーム画面がどこにあるか。
//!
//! スプラの配信では画面の配置を変えないので、固定の矩形で持つ（ユーザー判断 2026-10-03）。
//! nicomment のスプラのテーマ（`stage.theme = splatoon3`: 上帯 10%・下帯 10%・16:9・左詰め）の
//! ゲーム穴で、2026-10-03 に撮った出力の黄色い枠（x 0〜5 / 1536〜1541、y 105〜106 / 969〜971）とも合う。
//! 枠はゲーム穴の縁に数 px 重なって描かれるので、照合の ROI は縁に寄せすぎない。
//! テーマの帯の厚みを変えたらここも直す。

use image::imageops;
use image::RgbImage;

/// 出力の基準の大きさ
const OUTPUT_W: u32 = 1920;

/// 1920×1080 の出力の中のゲーム穴（x, y, 幅, 高さ）
const GAME: (u32, u32, u32, u32) = (0, 108, 1536, 864);

/// 撮った出力からゲーム穴を切り出す。撮った大きさに合わせて矩形を縮める
/// （1280×720 で撮れば 1024×576）。
pub fn crop_game(output: &RgbImage) -> RgbImage {
    let s = |v: u32| (v as u64 * output.width() as u64 / OUTPUT_W as u64) as u32;
    let (x, y, w, h) = (s(GAME.0), s(GAME.1), s(GAME.2), s(GAME.3));
    let w = w.min(output.width().saturating_sub(x));
    let h = h.min(output.height().saturating_sub(y));
    imageops::crop_imm(output, x, y, w, h).to_image()
}

//! 配信の出力の中で、ゲーム画面がどこにあるか。
//!
//! スプラの配信では画面の配置を変えないので、固定の矩形で持つ（ユーザー判断 2026-10-03）。
//! 配置は配信者ごとに違うので、設定の `game_area` で決める（2026-10-06。既定は全画面）。
//! 切り出した後はゲーム画面の幅を基準に縮めて照合するので、16:9 であれば大きさは問わない。

use image::imageops;
use image::RgbImage;
use serde::{Deserialize, Serialize};

/// 出力の基準の大きさ
pub const OUTPUT_W: u32 = 1920;
pub const OUTPUT_H: u32 = 1080;

/// 1920×1080 の出力の中のゲーム画面
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameArea {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl GameArea {
    /// ゲーム画面だけを配信する
    pub const FULL: GameArea = GameArea { x: 0, y: 0, w: OUTPUT_W, h: OUTPUT_H };
    /// nicomment のスプラのテーマ（`stage.theme = splatoon3`: 上帯 10%・下帯 10%・16:9・左詰め）。
    /// 2026-10-03 に撮った出力の黄色い枠（x 0〜5 / 1536〜1541、y 105〜106 / 969〜971）とも合う。
    /// 枠はゲーム画面の縁に数 px 重なって描かれるので、照合の ROI は縁に寄せすぎない
    pub const NICOMMENT: GameArea = GameArea { x: 0, y: 108, w: 1536, h: 864 };

    pub fn check(&self) -> Result<(), String> {
        if self.w < 320 || self.x + self.w > OUTPUT_W || self.y + self.h > OUTPUT_H {
            return Err("ゲーム画面の位置は 1920×1080 の中に収め、幅は 320 以上にしてください".into());
        }
        // 照合はゲーム画面を 16:9 に縮めて行うので、比が崩れると読めない
        let expect = self.w as f64 * 9.0 / 16.0;
        if (self.h as f64 - expect).abs() > expect * 0.02 {
            return Err(format!("ゲーム画面は 16:9 にしてください（幅 {} なら高さ {}）", self.w, expect.round()));
        }
        Ok(())
    }
}

impl Default for GameArea {
    fn default() -> Self {
        GameArea::FULL
    }
}

/// 撮った出力からゲーム画面を切り出す。撮った大きさに合わせて矩形を縮める
/// （1280×720 で撮れば、全画面なら 1280×720、nicomment の配置なら 1024×576）。
pub fn crop_game(output: &RgbImage, area: GameArea) -> RgbImage {
    let s = |v: u32| (v as u64 * output.width() as u64 / OUTPUT_W as u64) as u32;
    let (x, y, w, h) = (s(area.x), s(area.y), s(area.w), s(area.h));
    let w = w.min(output.width().saturating_sub(x));
    let h = h.min(output.height().saturating_sub(y));
    imageops::crop_imm(output, x, y, w, h).to_image()
}

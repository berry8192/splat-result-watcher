//! GUI の exe。起動すると撮影・照合・WebSocket のサーバ（engine.rs）を回し、画面で
//! 段階・撮影プレビュー・見本の登録を見せる。画面は `ui/`（React + Vite）。

#![windows_subsystem = "windows"]

use std::io::Cursor;
use std::sync::Mutex;

use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, RgbImage};
use serde::Serialize;
use tauri::{Manager, State};

use splat_result_watcher::engine::{Engine, EngineConfig, Snapshot};
use splat_result_watcher::matching::{self, Patch};
use splat_result_watcher::nair;
use splat_result_watcher::recognize::{GlyphRead, Score};
use splat_result_watcher::templates::{self, glyph_label, place, Kind, Pool, TemplateInfo, PLACES, WORK_W};

struct App {
    engine: Mutex<Engine>,
    /// 見本を切り出す元の絵（照合する大きさ 1024×576 にそろえたゲーム穴）
    source: Mutex<Option<RgbImage>>,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn jpeg_url(img: &RgbImage, max_w: u32) -> String {
    let img = if img.width() > max_w {
        let h = img.height() * max_w / img.width();
        imageops::resize(img, max_w, h, FilterType::Triangle)
    } else {
        img.clone()
    };
    let mut buf = Vec::new();
    let _ = JpegEncoder::new_with_quality(&mut buf, 80).encode_image(&img);
    format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf))
}

fn png_url(img: &DynamicImage) -> String {
    let mut buf = Cursor::new(Vec::new());
    let _ = img.write_to(&mut buf, ImageFormat::Png);
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

/// 白黒の絵を、見やすいよう整数倍に拡大して PNG に
fn patch_url(p: &Patch, scale: u32) -> String {
    let img = image::GrayImage::from_fn(p.w * scale, p.h * scale, |x, y| {
        image::Luma([if p.px[((y / scale) * p.w + x / scale) as usize] != 0 { 255 } else { 0 }])
    });
    png_url(&DynamicImage::ImageLuma8(img))
}

fn pool_by_name(name: &str) -> Res<Pool> {
    Pool::ALL.into_iter().find(|p| p.dir_name() == name).ok_or_else(|| format!("見本の種類 {name} が無い"))
}

#[tauri::command]
fn status(app: State<App>) -> Snapshot {
    app.engine.lock().unwrap().snapshot()
}

/// 最新のゲーム穴（プレビュー）
#[tauri::command]
fn frame(app: State<App>, max_w: u32) -> Option<String> {
    let f = app.engine.lock().unwrap().frame()?;
    Some(jpeg_url(&f, max_w))
}

#[tauri::command]
fn set_record(app: State<App>, on: bool) {
    app.engine.lock().unwrap().set_record(on);
}

#[derive(Serialize)]
struct LabelInfo {
    id: &'static str,
    name: &'static str,
}

#[derive(Serialize)]
struct PlaceInfo {
    id: &'static str,
    name: &'static str,
    pool: &'static str,
    glyphs: bool,
    labels: Vec<LabelInfo>,
    /// 照合する大きさ（1024×576）での矩形
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

#[tauri::command]
fn places() -> Vec<PlaceInfo> {
    PLACES
        .iter()
        .map(|p| {
            let s = |v: u32| v * WORK_W / 1536;
            PlaceInfo {
                id: p.id,
                name: p.name,
                pool: p.pool.dir_name(),
                glyphs: p.kind == Kind::Glyphs,
                labels: match p.kind {
                    Kind::Labels(l) => l.iter().map(|(id, name)| LabelInfo { id, name }).collect(),
                    Kind::Glyphs => Vec::new(),
                },
                x: s(p.roi.x),
                y: s(p.roi.y),
                w: s(p.roi.w),
                h: s(p.roi.h),
            }
        })
        .collect()
}

/// 今の画面を見本の元にする
#[tauri::command]
fn source_from_live(app: State<App>) -> Res<String> {
    let f = app.engine.lock().unwrap().frame().ok_or("まだ撮れていない（N Air は起きている？）")?;
    let work = templates::to_work(&f);
    let url = jpeg_url(&work, WORK_W);
    *app.source.lock().unwrap() = Some(work);
    Ok(url)
}

/// 画像ファイル（data URL）を見本の元にする。ゲーム穴だけの絵（snap・record で残したもの）を渡す
#[tauri::command]
fn source_from_file(app: State<App>, data_url: String) -> Res<String> {
    let b64 = data_url.split_once(',').map(|(_, b)| b).unwrap_or(&data_url);
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(err)?;
    let img = image::load_from_memory(&bytes).map_err(err)?.to_rgb8();
    let ratio = img.width() as f64 / img.height() as f64;
    if (ratio - 16.0 / 9.0).abs() > 0.02 {
        return Err(format!("16:9 の絵を渡す（{}×{}）", img.width(), img.height()));
    }
    let work = templates::to_work(&img);
    let url = jpeg_url(&work, WORK_W);
    *app.source.lock().unwrap() = Some(work);
    Ok(url)
}

#[derive(Serialize)]
struct Inspect {
    /// 切り出した所（色つき・拡大）
    crop: String,
    /// 白黒にしたもの（拡大）
    binary: String,
    /// ラベルの見本との一致度（高い順）
    scores: Vec<Score>,
    /// 数字: 1 文字ずつの絵（小数点は null）
    glyphs: Vec<Option<String>>,
    reading: Option<GlyphRead>,
    /// 元の絵ぜんたいを読んだ結果
    seen: String,
    notes: Vec<String>,
}

#[tauri::command]
fn inspect(app: State<App>, place_id: String) -> Res<Inspect> {
    let p = place(&place_id).ok_or("場所が無い")?;
    let src = app.source.lock().unwrap().clone().ok_or("元の絵を選んでいない")?;
    let (x, y, w, h) = {
        let s = |v: u32| v * WORK_W / 1536;
        (s(p.roi.x), s(p.roi.y), s(p.roi.w), s(p.roi.h))
    };
    let crop = imageops::crop_imm(&src, x, y, w, h).to_image();
    let crop = imageops::resize(&crop, w * 2, h * 2, FilterType::Nearest);
    let bin = templates::cut(&src, p);
    let engine = app.engine.lock().unwrap();
    engine.read_recognizer(|r| {
        let reading = r.recognize(&src);
        let (scores, glyphs, read) = if p.kind == Kind::Glyphs {
            let g = r.glyphs(&src, p).iter().map(|g| g.as_ref().map(|g| patch_url(g, 2))).collect();
            (Vec::new(), g, Some(r.read_glyphs(&src, p)))
        } else {
            (r.scores(&src, p), Vec::new(), None)
        };
        Ok(Inspect {
            crop: png_url(&DynamicImage::ImageRgb8(crop)),
            binary: patch_url(&bin, 2),
            scores,
            glyphs,
            reading: read,
            seen: format!("{:?}", reading.seen),
            notes: reading.notes,
        })
    })
}

/// 決まった文字の見本を足す
#[tauri::command]
fn register_label(app: State<App>, place_id: String, label: String) -> Res<String> {
    let p = place(&place_id).ok_or("場所が無い")?;
    let Kind::Labels(labels) = p.kind else {
        return Err("数字の場所は register_glyphs で足す".into());
    };
    if !labels.iter().any(|(id, _)| *id == label) {
        return Err(format!("ラベル {label} はこの場所に無い"));
    }
    let src = app.source.lock().unwrap().clone().ok_or("元の絵を選んでいない")?;
    let patch = templates::cut(&src, p);
    if patch.px.iter().all(|&v| v == 0) {
        return Err("白い所が無い（場所や元の絵が合っているか確かめる）".into());
    }
    app.engine.lock().unwrap().with_recognizer(|r| r.templates_mut().add(p.pool, &label, patch)).map_err(err)
}

/// 数字の見本を 1 文字ずつ足す。`text` は見えている数字そのまま（例 "2194.6"、"+62.2"）
#[tauri::command]
fn register_glyphs(app: State<App>, place_id: String, text: String) -> Res<usize> {
    let p = place(&place_id).ok_or("場所が無い")?;
    if p.kind != Kind::Glyphs {
        return Err("数字の場所ではない".into());
    }
    let src = app.source.lock().unwrap().clone().ok_or("元の絵を選んでいない")?;
    let glyphs = matching::glyphs(&templates::cut(&src, p));
    let chars: Vec<char> = text.trim().chars().collect();
    if chars.len() != glyphs.len() {
        return Err(format!("{} 文字に切れている（入れたのは {} 文字）", glyphs.len(), chars.len()));
    }
    let mut todo = Vec::new();
    for (c, g) in chars.iter().zip(glyphs) {
        match (c, g) {
            ('.', None) => {}
            ('.', Some(_)) | (_, None) => return Err(format!("小数点の位置が合わない（{c}）")),
            (c, Some(g)) => todo.push((glyph_label(*c).ok_or(format!("{c} は数字の見本にできない"))?, g)),
        }
    }
    let n = todo.len();
    app.engine.lock().unwrap().with_recognizer(|r| {
        for (l, g) in todo {
            r.templates_mut().add(p.pool, &l, g)?;
        }
        anyhow::Ok(())
    })
    .map_err(err)?;
    Ok(n)
}

#[derive(Serialize)]
struct TemplateView {
    #[serde(flatten)]
    info: TemplateInfo,
    image: String,
}

#[derive(Serialize)]
struct PoolView {
    pool: &'static str,
    templates: Vec<TemplateView>,
}

#[tauri::command]
fn list_templates(app: State<App>) -> Vec<PoolView> {
    app.engine.lock().unwrap().read_recognizer(|r| {
        Pool::ALL
            .iter()
            .map(|&pool| PoolView {
                pool: pool.dir_name(),
                templates: r
                    .templates()
                    .list(pool)
                    .into_iter()
                    .map(|info| {
                        let image = r
                            .templates()
                            .image(pool, &info.id)
                            .map(|g| png_url(&DynamicImage::ImageLuma8(g)))
                            .unwrap_or_default();
                        TemplateView { info, image }
                    })
                    .collect(),
            })
            .collect()
    })
}

#[tauri::command]
fn delete_template(app: State<App>, pool: String, id: String) -> Res<()> {
    let pool = pool_by_name(&pool)?;
    app.engine.lock().unwrap().with_recognizer(|r| r.templates_mut().remove(pool, &id)).map_err(err)
}

fn main() {
    nair::init_dpi();
    tauri::Builder::default()
        .setup(|app| {
            let rt = tauri::async_runtime::handle();
            let engine = Engine::start(EngineConfig::default(), rt.inner())?;
            app.manage(App { engine: Mutex::new(engine), source: Mutex::new(None) });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // プロジェクターを閉じてから終わる
                if let Some(app) = window.try_state::<App>() {
                    app.engine.lock().unwrap().stop();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            status,
            frame,
            set_record,
            places,
            source_from_live,
            source_from_file,
            inspect,
            register_label,
            register_glyphs,
            list_templates,
            delete_template,
        ])
        .run(tauri::generate_context!())
        .expect("GUI を起動できない");
}

//! GUI の exe。起動すると撮影・照合・WebSocket のサーバ（engine.rs）を回す。窓は 2 つ:
//! - `main`: 勝敗などを見せる小さな窓。閉じるとアプリが終わる
//! - `settings`: 状態・撮影プレビュー・見本の登録。`main` のボタンで開き、閉じてもアプリは続く
//!
//! 画面は `ui/`（React + Vite）。`index.html#settings` で設定の窓の中身になる。

#![windows_subsystem = "windows"]

use std::io::Cursor;
use std::sync::Mutex;

use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, RgbImage};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use splat_result_watcher::engine::{Engine, RecentFrame, Snapshot};
use splat_result_watcher::settings::Settings;
use splat_result_watcher::matching::{Glyph, Patch};
use splat_result_watcher::nair;
use splat_result_watcher::recognize::{GlyphRead, Score};
use splat_result_watcher::templates::{self, glyph_label, place, Kind, Pool, TemplateInfo, PLACES, WORK_W};

struct App {
    engine: Mutex<Engine>,
    /// 見本を切り出す元の絵（照合する大きさ 1024×576 にそろえたゲーム穴）
    source: Mutex<Option<RgbImage>>,
    /// 取り込んだ直近の画面（見ている間に流れていかないよう、取り込んだ時点のものを持つ）
    held: Mutex<Vec<RecentFrame>>,
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

/// 設定・手動作業の窓を開く（開いていれば前に出す）
#[tauri::command]
fn open_settings(app: AppHandle) -> Res<()> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        w.show().map_err(err)?;
        return w.set_focus().map_err(err);
    }
    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("index.html#settings".into()))
        .title("splat-result-watcher - 設定・見本")
        .inner_size(1280.0, 860.0)
        .build()
        .map_err(err)?;
    Ok(())
}

/// 録画の入り切り。すぐ効き、設定にも残す
#[tauri::command]
fn set_record(app: State<App>, on: bool) -> Res<()> {
    app.engine.lock().unwrap().set_record(on);
    let (mut s, _) = Settings::load();
    s.record = on;
    s.save().map_err(err)
}

#[tauri::command]
fn get_settings() -> Settings {
    Settings::load().0
}

/// 設定を残す。番号と撮る幅は起動し直したときに効く（録画の入り切りはすぐ効く）
#[tauri::command]
fn save_settings(app: State<App>, settings: Settings) -> Res<()> {
    settings.save().map_err(err)?;
    app.engine.lock().unwrap().set_record(settings.record);
    Ok(())
}

/// 今の試合を捨てて待機に戻す
#[tauri::command]
fn reset_game(app: State<App>) {
    app.engine.lock().unwrap().reset_game();
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
struct HeldInfo {
    time: String,
    seen: String,
}

/// 直近の画面を取り込む（古い順の一覧を返す）
#[tauri::command]
fn hold_recent(app: State<App>) -> Vec<HeldInfo> {
    let recent = app.engine.lock().unwrap().recent();
    let list = recent.iter().map(|f| HeldInfo { time: f.at.format("%H:%M:%S%.1f").to_string(), seen: f.seen.clone() }).collect();
    *app.held.lock().unwrap() = recent;
    list
}

/// 取り込んだ直近の画面の `index` 枚目を見本の元にする
#[tauri::command]
fn source_from_held(app: State<App>, index: usize) -> Res<String> {
    let f = app.held.lock().unwrap().get(index).cloned().ok_or("その画面はもう無い（取り込み直す）")?;
    let img = image::load_from_memory_with_format(&f.jpeg, ImageFormat::Jpeg).map_err(err)?.to_rgb8();
    *app.source.lock().unwrap() = Some(templates::to_work(&img));
    Ok(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(f.jpeg.as_slice())))
}

#[derive(Serialize)]
struct GlyphView {
    image: Option<String>,
    mark: Option<char>,
}

#[derive(Serialize)]
struct Inspect {
    /// 切り出した所（色つき・拡大）
    crop: String,
    /// 白黒にしたもの（拡大）
    binary: String,
    /// ラベルの見本との一致度（高い順）
    scores: Vec<Score>,
    /// 数字: 1 文字ずつ。字は絵、小数点とマイナスは印（形で決まるので見本は要らない）
    glyphs: Vec<GlyphView>,
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
            let g = r
                .glyphs(&src, p)
                .iter()
                .map(|g| match g {
                    Glyph::Dot => GlyphView { image: None, mark: Some('.') },
                    Glyph::Minus => GlyphView { image: None, mark: Some('-') },
                    Glyph::Shape(g) => GlyphView { image: Some(patch_url(g, 2)), mark: None },
                })
                .collect();
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
    let glyphs = templates::cut_glyphs(&src, p);
    let chars: Vec<char> = text.trim().chars().collect();
    if chars.len() != glyphs.len() {
        return Err(format!("{} 文字に切れている（入れたのは {} 文字）", glyphs.len(), chars.len()));
    }
    let mut todo = Vec::new();
    for (c, g) in chars.iter().zip(glyphs) {
        match (c, g) {
            ('.', Glyph::Dot) | ('-', Glyph::Minus) => {}
            ('.' | '-', _) | (_, Glyph::Dot | Glyph::Minus) => {
                return Err(format!("小数点・マイナスの位置が合わない（{c}）"))
            }
            (c, Glyph::Shape(g)) => todo.push((glyph_label(*c).ok_or(format!("{c} は数字の見本にできない"))?, g)),
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

/// 起動は 1 つだけにする。すでに動いていれば、その見せる窓を前に出して `false`。
/// 前の GUI が後始末の途中（窓はもう無い）なら、終わるまで最大 30 秒待つ。
/// （2 つ動くと、後の方は 3140 番を取れず、撮影も取り合いになった。2026-10-04）
fn single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE};

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        // 持ったまま終わるまで離さない（プロセスが終われば Windows が片付ける）
        let Ok(h) = (unsafe { CreateMutexW(None, false, w!("Local\\splat-result-watcher-gui")) }) else {
            return true;
        };
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            return true;
        }
        let _ = unsafe { CloseHandle(h) };
        if let Ok(win) = unsafe { FindWindowW(w!("Tauri Window"), w!("splat-result-watcher")) } {
            unsafe {
                let _ = ShowWindow(win, SW_RESTORE);
                let _ = SetForegroundWindow(win);
            }
            return false;
        }
        if std::time::Instant::now() >= deadline {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn main() {
    if !single_instance() {
        return;
    }
    nair::init_dpi();
    tauri::Builder::default()
        .setup(|app| {
            let rt = tauri::async_runtime::handle();
            let (settings, warn) = Settings::load();
            let engine = Engine::start(settings.engine_config(), rt.inner())?;
            if let Some(w) = warn {
                eprintln!("{w}");
            }
            app.manage(App { engine: Mutex::new(engine), source: Mutex::new(None), held: Mutex::new(Vec::new()) });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 見せる窓を閉じたら、設定の窓もすぐ消して終わる。プロジェクターを閉じるなどの後始末は
            // 裏のスレッドで済ませる（ここで待つと窓が固まり、閉じられないように見えた。2026-10-04）
            if window.label() == "main" {
                if let tauri::WindowEvent::Destroyed = event {
                    let h = window.app_handle().clone();
                    if let Some(w) = h.get_webview_window("settings") {
                        let _ = w.destroy();
                    }
                    std::thread::spawn(move || {
                        if let Some(app) = h.try_state::<App>() {
                            app.engine.lock().unwrap().stop();
                        }
                        h.exit(0);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            status,
            frame,
            set_record,
            get_settings,
            save_settings,
            reset_game,
            open_settings,
            places,
            source_from_live,
            source_from_file,
            hold_recent,
            source_from_held,
            inspect,
            register_label,
            register_glyphs,
            list_templates,
            delete_template,
        ])
        .run(tauri::generate_context!())
        .expect("GUI を起動できない");
}

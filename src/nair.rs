//! N Air の出力を撮る。手順は docs/protocol.md の「N Air の出力の撮り方」。
//!
//! 1. 名前付きパイプ `\\.\pipe\n-air-app` に `createProjector` を送り、出力のプロジェクターを開く
//! 2. 開く前と後で「全画面表示」の窓を列挙し、増えた 1 枚を自分のものとして控える
//!    （nicomment も同じ方法で開くことがあるので、相手の窓は触らない）
//! 3. 窓を画面の外へ逃がし、子窓が 1920×1080 になる大きさにする
//! 4. 子窓 `SlobsChildWindowPreview` を `PrintWindow(PW_RENDERFULLCONTENT)` で撮る

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use image::RgbImage;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, GetClientRect, GetWindowRect, GetWindowTextW,
    IsWindow, PostMessageW, SetProcessDPIAware, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST,
    SWP_NOACTIVATE, WM_CLOSE,
};

const PIPE: &str = r"\\.\pipe\n-air-app";
const PROJECTOR_TITLE: &str = "全画面表示";
const PROJECTOR_CLASS: &str = "Chrome_WidgetWin_1";
const PREVIEW_TITLE: &str = "SlobsChildWindowPreview";
const PREVIEW_CLASS: &str = "Win32DisplayClass";

/// 窓を逃がす先。人の目に入らず、どのモニタにも掛からない位置
const PARK_X: i32 = -8000;
const PARK_Y: i32 = 0;
/// 大きさを決める間、主モニタの左上に出す角の大きさ
const PEEK: i32 = 8;
/// 子窓の大きさが変わらなくなってから、生まれ終わったと見なすまでの時間
const SETTLE: Duration = Duration::from_millis(500);

/// PrintWindow に D3D の中身まで描かせる指定（ヘッダに名前が無い）
const PW_RENDERFULLCONTENT: u32 = 2;

/// 撮影の前に 1 回呼ぶ。呼ばないと窓の大きさが表示倍率で縮められて扱われる
pub fn init_dpi() {
    unsafe {
        let _ = SetProcessDPIAware();
    }
}

/// N Air の操作口に要求を 1 つ送り、同じ id の応答の `result` を返す。
fn rpc(method: &str, resource: &str) -> Result<serde_json::Value> {
    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE)
        .context("N Air の操作口につなげない（N Air が起動していない？）")?;
    let id = 1;
    let req = serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method,
        "params": {"resource": resource, "args": []}});
    pipe.write_all(format!("{}\n", req).as_bytes())?;
    pipe.flush()?;

    // 応答が来ないとき読み取りで止まらないよう、別スレッドで読んで待つ時間を区切る
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if v.get("id").and_then(|i| i.as_i64()) == Some(id) {
                let _ = tx.send(v);
                break;
            }
        }
    });
    let resp = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| anyhow!("N Air が {} に 5 秒応答しない", method))?;
    if let Some(err) = resp.get("error") {
        bail!("N Air が {} を断った: {}", method, err);
    }
    Ok(resp
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn title_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    list.push(hwnd);
    true.into()
}

fn top_windows() -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

/// 子孫の窓をすべて（EnumChildWindows は孫以下もたどる）
fn descendants(parent: HWND) -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(
            Some(parent),
            Some(collect),
            LPARAM(&mut list as *mut _ as isize),
        );
    }
    list
}

fn is_projector(hwnd: HWND) -> bool {
    class_of(hwnd) == PROJECTOR_CLASS && title_of(hwnd) == PROJECTOR_TITLE
}

fn projectors() -> Vec<HWND> {
    top_windows()
        .into_iter()
        .filter(|&h| is_projector(h))
        .collect()
}

fn find_preview(projector: HWND) -> Option<HWND> {
    descendants(projector)
        .into_iter()
        .find(|&h| class_of(h) == PREVIEW_CLASS && title_of(h) == PREVIEW_TITLE)
}

fn alive(hwnd: HWND) -> bool {
    unsafe { IsWindow(Some(hwnd)).as_bool() }
}

/// 窓と子窓の大きさの差（窓枠の厚み）。表示倍率で変わるので、開いたときに測って覚える。
/// 最初の見当は表示倍率 150% での実測（上に 96px、幅は丸めで子窓のほうが 1px 大きい）
static FRAME: Mutex<(i32, i32)> = Mutex::new((-1, 96));

/// 開けなかったときに開き直す回数
const OPEN_TRIES: u32 = 2;

/// 自分で開いたプロジェクター。落としたら閉じる。
pub struct Projector {
    window: HWND,
    preview: HWND,
    /// 撮りたい子窓の大きさ
    size: (i32, i32),
    /// 窓ハンドルの控え。前回が落ちて閉じ損ねたとき、次に起きたときに閉じるため
    marker: Option<PathBuf>,
}

impl Projector {
    /// 新しくプロジェクターを開いて画面の外へ置く。`marker` に窓ハンドルを控える。
    /// `width` は撮る幅（高さは 16:9）。PrintWindow の時間は面積に比例する
    /// （1920×1080 で約 27ms、1280×720 で約 16ms）ので、要るだけの大きさにする。
    pub fn open(marker: Option<&Path>, width: u32) -> Result<Self> {
        let size = (width as i32, (width * 9 / 16) as i32);
        if let Some(m) = marker {
            close_leftover(m);
        }
        let mut last_err = None;
        for _ in 0..OPEN_TRIES {
            match Self::open_once(marker, size) {
                Ok(p) => return Ok(p),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap())
    }

    /// **隠れている窓（画面の外・ほかの窓の下）では、Chromium が中身の大きさを変えない**ので、
    /// 画面の外で窓の大きさを変えても子窓は追従しない（2026-10-03 に確認。開いた窓がほかの窓の
    /// 下に出ると、子窓は既定の 960×504 のまま生まれる）。Chromium は一部でも見えていれば
    /// 「見えている」とみなすので、大きさを決める間だけ最前面にして主モニタの左上に角を
    /// 少し出し、子窓が追いついてから画面の外へ逃がす。画面の外へ出したあとも描画は続く
    fn open_once(marker: Option<&Path>, size: (i32, i32)) -> Result<Self> {
        let before = projectors();
        rpc("createProjector", "ProjectorService")?;

        // 窓は応答より遅れて出る。増えた 1 枚が見つかるまで待つ。
        // 画面の真ん中に出るので、人の目に入る時間を短くしたいので細かく見る
        let deadline = Instant::now() + Duration::from_secs(5);
        let window = loop {
            let added: Vec<HWND> = projectors()
                .into_iter()
                .filter(|h| !before.contains(h))
                .collect();
            match added.len() {
                1 => break added[0],
                0 if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                0 => bail!(
                    "createProjector のあと「{}」の窓が増えない",
                    PROJECTOR_TITLE
                ),
                // 同時に誰か（nicomment）も開いた。どちらが自分か分からないので触らない
                n => bail!("「{}」の窓が同時に {} 枚増えた", PROJECTOR_TITLE, n),
            }
        };
        if let Some(m) = marker {
            let _ = std::fs::write(m, (window.0 as isize).to_string());
        }
        // ここから先で失敗したら、落とすときに閉じる
        let mut me = Projector {
            window,
            preview: HWND::default(),
            size,
            marker: marker.map(Path::to_path_buf),
        };
        let mut frame = *FRAME.lock().unwrap();
        peek(window, size.0 + frame.0, size.1 + frame.1)?;

        // 子窓が出て大きさが落ち着くのを待つ（出た直後は 0×0）。違えば窓枠の厚みを測り直して合わせる
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut seen = (0, 0);
        let mut since = Instant::now();
        loop {
            if let Some(p) = find_preview(window) {
                me.preview = p;
                let c = client_size(p)?;
                if c != seen {
                    seen = c;
                    since = Instant::now();
                } else if c == size && since.elapsed() >= SETTLE {
                    break;
                } else if c.0 > 0 && c.1 > 0 && since.elapsed() >= SETTLE {
                    let win = window_rect(window)?;
                    frame = ((win.right - win.left) - c.0, (win.bottom - win.top) - c.1);
                    peek(window, size.0 + frame.0, size.1 + frame.1)?;
                    since = Instant::now();
                }
            }
            if Instant::now() >= deadline {
                bail!(
                    "プロジェクターの子窓が {}×{} にならない（{}×{} のまま）",
                    size.0,
                    size.1,
                    seen.0,
                    seen.1
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        *FRAME.lock().unwrap() = frame;
        park(window, size.0 + frame.0, size.1 + frame.1)?;
        Ok(me)
    }

    /// 窓がまだあるか（N Air の再起動や人の操作で消える）
    pub fn alive(&self) -> bool {
        alive(self.window) && alive(self.preview)
    }

    /// 子窓を撮る。RGB で、大きさは開いたときの指定どおり（違えば失敗にする。開き直すこと）
    pub fn capture(&self) -> Result<RgbImage> {
        let img = capture_window(self.preview)?;
        if (img.width() as i32, img.height() as i32) != self.size {
            bail!(
                "子窓が {}×{} に変わった（{}×{} のはず）",
                img.width(),
                img.height(),
                self.size.0,
                self.size.1
            );
        }
        Ok(img)
    }
}

impl Drop for Projector {
    fn drop(&mut self) {
        if alive(self.window) && is_projector(self.window) {
            unsafe {
                let _ = PostMessageW(Some(self.window), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(m) = &self.marker {
            let _ = std::fs::remove_file(m);
        }
    }
}

/// 前回の自分が閉じ損ねたプロジェクターを閉じる。控えたハンドルがまだプロジェクターなら閉じる
/// （ハンドルは使い回されるので、窓の種類も確かめる）。
fn close_leftover(marker: &Path) {
    let Ok(text) = std::fs::read_to_string(marker) else {
        return;
    };
    let _ = std::fs::remove_file(marker);
    let Ok(raw) = text.trim().parse::<isize>() else {
        return;
    };
    let hwnd = HWND(raw as *mut _);
    if alive(hwnd) && is_projector(hwnd) {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// 画面の外へ逃がし、最前面を外す
fn park(window: HWND, w: i32, h: i32) -> Result<()> {
    unsafe {
        SetWindowPos(
            window,
            Some(HWND_NOTOPMOST),
            PARK_X,
            PARK_Y,
            w,
            h,
            SWP_NOACTIVATE,
        )
    }
    .context("プロジェクターを画面の外へ動かせない")
}

/// 最前面にして、主モニタの左上に右下の角を PEEK px だけ出す（Chromium に「見えている」と思わせる）
fn peek(window: HWND, w: i32, h: i32) -> Result<()> {
    unsafe {
        SetWindowPos(
            window,
            Some(HWND_TOPMOST),
            PEEK - w,
            PEEK - h,
            w,
            h,
            SWP_NOACTIVATE,
        )
    }
    .context("プロジェクターを動かせない")
}

fn window_rect(hwnd: HWND) -> Result<RECT> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.context("窓の位置が取れない")?;
    Ok(rect)
}

fn client_size(hwnd: HWND) -> Result<(i32, i32)> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.context("子窓の大きさが取れない")?;
    Ok((rect.right - rect.left, rect.bottom - rect.top))
}

fn capture_window(hwnd: HWND) -> Result<RgbImage> {
    let (w, h) = client_size(hwnd)?;
    if w <= 0 || h <= 0 {
        bail!("子窓の大きさが 0（{}×{}）", w, h);
    }

    let mut bgra = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bitmap.into());

        let printed = PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // 負にすると上から下の並びで返る
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // GetDIBits は選択中のビットマップを読めないので先に外す
        SelectObject(dc, old);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            h as u32,
            Some(bgra.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );

        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);

        if !printed {
            bail!("PrintWindow が失敗した");
        }
        if lines != h {
            bail!("GetDIBits が {} 行しか返さない（{} 行のはず）", lines, h);
        }
    }

    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for p in bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[p[2], p[1], p[0]]);
    }
    RgbImage::from_raw(w as u32, h as u32, rgb).ok_or_else(|| anyhow!("画素数が合わない"))
}

/// ほぼ黒の画素の割合（映像が来ていない判定に使う）。全画素は見ずに間引いて数える
pub fn dark_ratio(img: &RgbImage) -> f64 {
    let mut dark = 0usize;
    let mut total = 0usize;
    for (i, p) in img.pixels().enumerate() {
        if i % 97 != 0 {
            continue;
        }
        total += 1;
        if p.0.iter().all(|&c| c < 16) {
            dark += 1;
        }
    }
    if total == 0 {
        1.0
    } else {
        dark as f64 / total as f64
    }
}

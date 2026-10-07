# CLAUDE.md - splat-result-watcher

スプラトゥーン3 の配信映像（N Air か OBS Studio の出力）からリザルト画面を読み、勝敗・X パワー・ウデマエポイントを
WebSocket（既定 `ws://127.0.0.1:3140/events`）で流す単独アプリ。受け手の 1 つが `../nicomment`。

- 約束事: [docs/protocol.md](docs/protocol.md)（正本。受け手に影響する変更は nicomment に知らせる。「N Air の出力の撮り方」の節は nicomment も参照している）
- 方針・試合の管理・読むもの・見本集めの宿題: [docs/design.md](docs/design.md)
- **バトルに有利になる情報を出さない**。試合中に出すのは `status` と `battle_started` だけ
- 認識に LLM を使わない（見本との照合。見本が無ければ形と色の決まりごと `src/shapes.rs` と手がかりの数字 `src/starter.rs` で読み、確かめられたものを見本に足す `src/learn.rs`）。ゲームの音は使わない
- 受け手の動作確認は nicomment 側の模擬サーバ `cargo run --example splat_detect_mock`（../nicomment/app/src-tauri）を参考にする

## 動かし方
- `npm --prefix ui install`（初回だけ）→ `npm --prefix ui run build` → `cargo build --release`
  （GUI の画面 `ui/dist` を exe に埋め込むので、先に画面を組む。撮影・縮小は debug だと遅い。依存は debug でも最適化してある）
- `target/release/splat-result-watcher-gui.exe` … GUI。起動すると撮影・照合・WebSocket が回る。字だけの見せる窓（枠なし。配信ソフトのウィンドウキャプチャで載せる前提。左ドラッグで動かし、右クリックで横長・縦長・背景色（透明も）・設定・終了。字の大きさは設定の窓の「見せる窓」。窓の大きさは中身に合わせて変わる。同梱の M PLUS 1p（OFL、`ui/public/fonts`）で描く）と、設定の窓（状態・テンプレートの状況・テンプレートの登録・設定）。画面の文言は落ち着いた文体で、独自の言い回し（見本・撮る・ゲーム穴 など）は使わず一般的な語（テンプレート・キャプチャ・ゲーム画面）にそろえる（2026-10-06 の利用者の指摘）。見本の登録は直近の画面（メモリに JPEG で最大 200MB・30 分。バトル中は 5 秒に 1 枚、結果〜試合後は 0.2 秒ごと）からシークバーで選べる。設定は `%LOCALAPPDATA%\splat-result-watcher\settings.json`（serve も既定として読む）。起動は 1 つだけ。記録は `logs\日付.log`（段階ごとの一番高い一致度つき）
- デバッグ用の当たりの記録: `%LOCALAPPDATA%\splat-result-watcher\hits\YYYYMMDD\hits.jsonl`（1 行 1 件の JSON。何かに当たったフレームのうち中身が変わったものだけ。
  `seen`・`shapes`（形と色で見分けた見出し）・`numbers`（`text` は見本で読めた字、`guess` は推測）・`peaks`・`learned`・`events`・`image`）と、
  同じフォルダの `image` の JPEG（ゲーム穴）。上限 500MB で古い日から消す。設定の「当たりの記録を残す」で切れる（`src/hitlog.rs`）
- `target/release/splat-result-watcher.exe shot [out.png] [--width 1920]` … 1 枚撮って時間を測る（N Air か OBS が起きていること。どちらから撮るかは設定 `capture_from`、OBS は `obs_port` / `obs_password`。`src/source.rs`）
- `target/release/splat-result-watcher.exe snap <説明…> [--full]` … 1 枚撮ってゲーム穴を PNG で残す（`samples/snaps/`、説明は `index.tsv` にも）
- `target/release/splat-result-watcher.exe record [--dir D] [--width 1280] [--cap-gb 20] [--full]` … 見本の録画（ゲーム穴だけ）。Ctrl+C で止める
- `target/release/splat-result-watcher.exe serve [--record]` … 撮って読み、`ws://127.0.0.1:3140/events` で流す（照合には GUI で登録した見本を使う）。`--record` で見本の録画も一緒に回す。出来事の控えは `%LOCALAPPDATA%\splat-result-watcher\events.jsonl`
- `target/release/splat-result-watcher.exe probe` … 見本（`samples/snaps/`）で照合を試す
- `python tools/gen_icon.py` … アプリのアイコン（`icons/`）を描き直す。自作の図形だけで、ゲームの絵やロゴは使っていない
- `python tools/gen_starter.py` … 手がかりの数字（`src/starter_digits.rs`）を作り直す（手書きの `assets/hand_digits.png` を描き直したとき）
- `cargo test --release` … 状態の移り変わり・サーバ・見本の読み書きの試験。`-- --include-ignored` で手元の見本（samples/snaps）を使った照合の試験も
  （`samples/snaps/20261006-*` は精算・表彰・メニューなど。N Air の縁あり（10/03〜の見本は左 7px・下 4px に配信の黄色い縁が写る）と OBS の全画面の両方。
  `samples/snaps/web/` は攻略サイトのメニューの画像で、ランクの読み取りの試験に使う。`samples/keep/` は録画から残した一続きのフレームで、
  `SRW_REC=samples/keep/20261006-043707 SRW_KNOWN=284 cargo test --release -- --ignored replay_record --nocapture` で流し直せる。どれも git に入れない）
- exe の manifest は `app.manifest`（build.rs で埋め込む）。DPI はシステム全体で 1 つ。中に日本語を書くと exe が起動しなくなる
- ゲーム穴は固定（スプラの配信は配置を変えない）。`src/layout.rs`

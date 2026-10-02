# CLAUDE.md - splat-result-watcher

スプラトゥーン3 の配信映像（N Air の出力）からリザルト画面を読み、勝敗・X パワー・ウデマエポイントを
WebSocket（既定 `ws://127.0.0.1:3140/events`）で流す単独アプリ。受け手の 1 つが `../nicomment`。

- 約束事: [docs/protocol.md](docs/protocol.md)（原本は `../nicomment/docs/splatoon_detect_protocol.md`。変えるときは両方）
- 方針・状態遷移・読むもの・見本集めの宿題: [docs/design.md](docs/design.md)
- **バトルに有利になる情報を出さない**。試合中に出すのは `status` と `battle_started` だけ
- 認識に LLM を使わない（見本との照合）。ゲームの音は使わない
- 受け手の動作確認は nicomment 側の模擬サーバ `cargo run --example splat_detect_mock`（../nicomment/app/src-tauri）を参考にする

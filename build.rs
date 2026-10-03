fn main() {
    // Tauri の既定の manifest（共通コントロール 6 だけ）に、Windows 10 以降で動くことと DPI の扱いを足したものを埋め込む。
    // 既定のままだと表示倍率 150% で窓の大きさが 1.5 倍にずれ、プロジェクターを狙った大きさにできない（2026-10-03）。
    // DPI はシステム全体で 1 つ（SetProcessDPIAware と同じ）。モニタごと（PerMonitorV2）にすると、プロジェクターの
    // 大きさの合わせ込みが安定しなかった。
    // manifest の中には日本語を書かない（書くと「side-by-side 構成が正しくない」で exe が起動しなくなった）
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("tauri-build が失敗した");
}

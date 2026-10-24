//! 调用生产模块 `media::beat_model::analyze` 做端到端验证。
//!
//! `cargo run --release --example beat_model_check -- <音频>...`
fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("用法: beat_model_check <音频路径> [更多...]");
        std::process::exit(2);
    }
    println!("模型可用: {}", mercurial_player_lib::media::beat_model::is_available());
    for file in files {
        let started = std::time::Instant::now();
        match mercurial_player_lib::media::beat_model::analyze(&file) {
            Ok(track) => {
                let meter: String = track
                    .meter
                    .iter()
                    .take(3)
                    .map(|m| format!("{}拍×{}", m.beats_per_bar, m.count))
                    .collect::<Vec<_>>()
                    .join(", ");
                let name: String = std::path::Path::new(&file)
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                println!(
                    "{:<38} BPM {:>6}  {} 拍 / {} 小节线  [{meter}]  {:.1}ms",
                    name.chars().take(36).collect::<String>(),
                    track.bpm.map_or_else(|| "—".to_string(), |v| format!("{v:.1}")),
                    track.beats.len(),
                    track.downbeats.len(),
                    started.elapsed().as_millis()
                );
            }
            Err(e) => println!("{file}: 失败 {e:?}"),
        }
    }
}

//! 卡拉OK进度计算。
//!
//! 纯逻辑部分：按字素（grapheme）簇把歌词进度换算为颜色插值相位与
//! 剪裁终点 x 坐标，以及从逐字时间轴（`DesktopLyricWord`）推导整体
//! 卡拉OK进度。除 `hit_test_text_x` / `progress_clip_end_x` 需要
//! `IDWriteTextLayout` 做命中测试外，其余函数为可单测的纯计算。

use unicode_segmentation::UnicodeSegmentation;
use windows::Win32::Graphics::DirectWrite::{DWRITE_HIT_TEST_METRICS, IDWriteTextLayout};

use super::super::DesktopLyricWord;
use super::d2d_resources::trim_utf16_nul;

fn hit_test_text_x(layout: &IDWriteTextLayout, pos: u32, trailing: bool) -> Option<f32> {
    let mut x = 0.0f32;
    let mut y = 0.0f32;
    let mut metrics = DWRITE_HIT_TEST_METRICS::default();
    // SAFETY: layout 是有效 COM 对象；x/y/metrics 均为栈上局部变量，
    // &raw mut 传给 Win32 写入后在本函数内读取，不存在别名冲突
    unsafe {
        layout
            .HitTestTextPosition(pos, trailing, &raw mut x, &raw mut y, &raw mut metrics)
            .ok()?;
    }
    Some(x)
}

pub(super) fn progress_cluster_phase(text: &[u16], progress: f32) -> Option<f32> {
    let text = trim_utf16_nul(text);
    if text.is_empty() {
        return None;
    }
    let progress = progress.clamp(0.0, 1.0);
    let decoded = String::from_utf16_lossy(text);
    let graphemes: Vec<&str> = UnicodeSegmentation::graphemes(decoded.as_str(), true).collect();
    if graphemes.is_empty() {
        return None;
    }

    let exact = (graphemes.len() as f32) * progress;
    let highlighted = exact.floor() as usize;
    if highlighted >= graphemes.len() {
        return Some(1.0);
    }

    let phase = exact - highlighted as f32;
    Some(phase * phase * (3.0 - 2.0 * phase))
}

pub(super) fn progress_clip_end_x(
    layout: &IDWriteTextLayout,
    text: &[u16],
    progress: f32,
) -> Option<f32> {
    let text = trim_utf16_nul(text);
    if text.is_empty() {
        return None;
    }
    let progress = progress.clamp(0.0, 1.0);
    let decoded = String::from_utf16_lossy(text);
    let graphemes: Vec<&str> = UnicodeSegmentation::graphemes(decoded.as_str(), true).collect();
    if graphemes.is_empty() {
        return None;
    }

    let exact = (graphemes.len() as f32) * progress;
    let highlighted = exact.floor() as usize;
    let current_t = exact - highlighted as f32;
    if highlighted >= graphemes.len() {
        return None;
    }

    let mut utf16_idx = 0usize;
    for (cluster_idx, grapheme) in graphemes.iter().enumerate() {
        let len = grapheme.encode_utf16().count();
        if cluster_idx == highlighted {
            let start = utf16_idx as u32;
            let end = (utf16_idx + len) as u32;
            let start_x = hit_test_text_x(layout, start, false)?;
            let end_x = hit_test_text_x(layout, end, false)
                .or_else(|| hit_test_text_x(layout, end.saturating_sub(1), true))?;
            let eased = current_t * current_t * (3.0 - 2.0 * current_t);
            return Some(start_x + (end_x - start_x) * eased);
        }
        utf16_idx += len;
    }

    None
}

pub(super) fn karaoke_progress_from_words(
    words: &[DesktopLyricWord],
    visual_time: f32,
    fallback: f32,
) -> f32 {
    let valid: Vec<&DesktopLyricWord> = words
        .iter()
        .filter(|word| !word.text.is_empty() && word.end > word.start)
        .collect();
    if valid.is_empty() {
        return fallback.clamp(0.0, 1.0);
    }

    let first_start = valid[0].start;
    let last_end = valid[valid.len() - 1].end;
    if visual_time <= first_start {
        return 0.0;
    }
    if visual_time >= last_end {
        return 1.0;
    }

    let total_units: usize = valid
        .iter()
        .map(|word| UnicodeSegmentation::graphemes(word.text.as_str(), true).count())
        .sum();
    if total_units == 0 {
        return fallback.clamp(0.0, 1.0);
    }

    let mut passed_units = 0.0f32;
    for word in valid {
        let units = UnicodeSegmentation::graphemes(word.text.as_str(), true).count() as f32;
        if visual_time >= word.end {
            passed_units += units;
            continue;
        }
        if visual_time > word.start {
            let local = ((visual_time - word.start) / (word.end - word.start)).clamp(0.0, 1.0);
            passed_units += units * local;
        }
        break;
    }

    (passed_units / total_units as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::super::super::DesktopLyricWord;
    use super::karaoke_progress_from_words;
    use super::progress_cluster_phase;

    fn word(text: &str, start: f32, end: f32) -> DesktopLyricWord {
        DesktopLyricWord {
            text: text.to_string(),
            start,
            end,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn karaoke_empty_words_uses_fallback() {
        assert!(close(karaoke_progress_from_words(&[], 1.0, 0.7), 0.7));
        // 越界的 fallback 会被钳制到 [0, 1]
        assert!(close(karaoke_progress_from_words(&[], 1.0, 2.5), 1.0));
        assert!(close(karaoke_progress_from_words(&[], 1.0, -1.0), 0.0));
    }

    #[test]
    fn karaoke_before_first_start_is_zero() {
        let words = [word("你好", 1.0, 2.0)];
        assert!(close(karaoke_progress_from_words(&words, 0.5, 0.9), 0.0));
    }

    #[test]
    fn karaoke_after_last_end_is_one() {
        let words = [word("你好", 1.0, 2.0)];
        assert!(close(karaoke_progress_from_words(&words, 3.0, 0.1), 1.0));
    }

    #[test]
    fn karaoke_midpoint_is_half() {
        // 2 个字素、区间 [0, 2]，t=1 时走过 1 个字素 → 0.5
        let words = [word("ab", 0.0, 2.0)];
        let p = karaoke_progress_from_words(&words, 1.0, 0.0);
        assert!(close(p, 0.5));
    }

    #[test]
    fn karaoke_weighted_across_words() {
        // 第 1 词 2 字素 [0,1]，第 2 词 2 字素 [1,2]；t=1.5 时共走 3/4
        let words = [word("ab", 0.0, 1.0), word("cd", 1.0, 2.0)];
        let p = karaoke_progress_from_words(&words, 1.5, 0.0);
        assert!(close(p, 0.75));
    }

    #[test]
    fn cluster_phase_bounds() {
        let text: Vec<u16> = "ab".encode_utf16().collect();
        assert_eq!(
            progress_cluster_phase(&text, 0.0).map(|v| close(v, 0.0)),
            Some(true)
        );
        assert_eq!(
            progress_cluster_phase(&text, 1.0).map(|v| close(v, 1.0)),
            Some(true)
        );
        // 中点落在第 2 个字素内部：smoothstep(0.5) = 0.5
        let mid = progress_cluster_phase(&text, 0.75).unwrap();
        assert!(close(mid, 0.5));
        assert_eq!(progress_cluster_phase(&[], 0.5), None);
    }
}

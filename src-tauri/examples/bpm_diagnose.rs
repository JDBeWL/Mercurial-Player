//! BPM 标定的批量回归 runner：用真值 CSV 对比算法改动的命中率与八度错误。
//!
//! 用法：`cargo run --release --example bpm_diagnose -- --dir <音乐目录> --truth <truth.csv>`

use mercurial_player_lib::media::bpm::{
    ANALYSIS_SKIP_SECS, ANALYSIS_WINDOW_SECS, BPM_MAX, BPM_MIN, CandidateScore, DP_TIGHTNESS,
    analyze_consensus, analyze_production, diagnose_tuned, read_tagged_bpm,
};
use mercurial_player_lib::media::filesystem::AUDIO_EXTENSIONS;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use walkdir::WalkDir;

/// 递归扫描的默认深度：必须与应用配置 `directoryScan.maxDepth` 一致，
/// 否则会把应用根本不索引的深层文件（素材、备份、temp）也算进来
const DEFAULT_SCAN_DEPTH: usize = 3;

fn main() {
    let args = match Args::parse() {
        Ok(Some(args)) => args,
        Ok(None) => return,
        Err(message) => {
            eprintln!("参数错误：{message}\n\n运行 `--help` 查看用法");
            std::process::exit(2);
        }
    };

    if let Err(error) = run(&args) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

struct Args {
    inputs: Vec<PathBuf>,
    truth: Option<PathBuf>,
    offsets: Vec<f64>,
    window: f64,
    tightness: f64,
    /// "精确命中"的相对容差
    tolerance: f64,
    /// 跨起点一致性的相对容差
    consistency_tolerance: f64,
    runs_out: PathBuf,
    candidates_out: Option<PathBuf>,
    /// 导出胜出假设的拍点帧号，供离线复算拟合方式
    beats_out: Option<PathBuf>,
    /// 曲线模式：整曲滑动扫描并合并成速度段，而不是固定几个起点
    curve: bool,
    /// 对比模式：单窗结果 vs 三窗共识
    vote: bool,
    /// 可疑清单：找出可能报错层级的曲目，供人工核对
    suspects: bool,
    /// 拍号可行性探针：打印重音周期
    meter: bool,
    /// 只打印生产口径的结果（含首窗回退）
    production: bool,
    /// 递归扫描深度，默认与应用的 `directoryScan.maxDepth` 一致
    depth: usize,
    /// 排除清单：每行一个完整路径或文件名
    exclude: Option<PathBuf>,
    /// 直接按应用的 library-cache.json 取曲目，保证与被索引的集合完全一致
    from_cache: Option<PathBuf>,
    curve_window: f64,
    curve_hop: f64,
    jobs: usize,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            truth: None,
            // 生产只用到第一个偏移；另外两个用于量"换一个起点会不会换个答案"
            offsets: vec![ANALYSIS_SKIP_SECS, ANALYSIS_SKIP_SECS + 20.0, ANALYSIS_SKIP_SECS + 40.0],
            window: ANALYSIS_WINDOW_SECS,
            tightness: DP_TIGHTNESS,
            tolerance: 0.02,
            consistency_tolerance: 0.01,
            runs_out: PathBuf::from("bpm-runs.csv"),
            candidates_out: None,
            beats_out: None,
            curve: false,
            vote: false,
            suspects: false,
            meter: false,
            production: false,
            depth: DEFAULT_SCAN_DEPTH,
            exclude: None,
            from_cache: None,
            curve_window: 12.0,
            curve_hop: 4.0,
            jobs: 0,
        }
    }
}

impl Args {
    fn parse() -> Result<Option<Self>, String> {
        let mut args = Self::default();
        let mut raw = std::env::args().skip(1).peekable();

        while let Some(arg) = raw.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    print_help();
                    return Ok(None);
                }
                "--dir" | "-d" => args.inputs.push(PathBuf::from(need(&mut raw, "--dir")?)),
                "--truth" | "-t" => args.truth = Some(PathBuf::from(need(&mut raw, "--truth")?)),
                "--offsets" => args.offsets = parse_offsets(&need(&mut raw, "--offsets")?)?,
                "--window" => args.window = parse_f64(&need(&mut raw, "--window")?, "--window")?,
                "--tightness" => {
                    args.tightness = parse_f64(&need(&mut raw, "--tightness")?, "--tightness")?;
                }
                "--tolerance" => {
                    args.tolerance = parse_f64(&need(&mut raw, "--tolerance")?, "--tolerance")?;
                }
                "--consistency" => {
                    args.consistency_tolerance =
                        parse_f64(&need(&mut raw, "--consistency")?, "--consistency")?;
                }
                "--out" => args.runs_out = PathBuf::from(need(&mut raw, "--out")?),
                "--candidates" => {
                    args.candidates_out = Some(PathBuf::from(need(&mut raw, "--candidates")?));
                }
                "--beats" => {
                    args.beats_out = Some(PathBuf::from(need(&mut raw, "--beats")?));
                }
                "--curve" => args.curve = true,
                "--vote" => args.vote = true,
                "--suspects" => args.suspects = true,
                "--meter" => args.meter = true,
                "--production" => args.production = true,
                "--depth" => {
                    let value = need(&mut raw, "--depth")?;
                    args.depth = value
                        .parse::<usize>()
                        .map_err(|_| format!("--depth 需要正整数，收到 {value}"))?;
                }
                "--exclude" => {
                    args.exclude = Some(PathBuf::from(need(&mut raw, "--exclude")?));
                }
                "--from-cache" => {
                    args.from_cache = Some(PathBuf::from(need(&mut raw, "--from-cache")?));
                }
                "--curve-window" => {
                    args.curve_window =
                        parse_f64(&need(&mut raw, "--curve-window")?, "--curve-window")?;
                }
                "--curve-hop" => {
                    args.curve_hop = parse_f64(&need(&mut raw, "--curve-hop")?, "--curve-hop")?;
                }
                "--jobs" => {
                    let value = need(&mut raw, "--jobs")?;
                    args.jobs = value
                        .parse::<usize>()
                        .map_err(|_| format!("--jobs 需要正整数，收到 {value}"))?;
                }
                other if other.starts_with('-') => return Err(format!("未知参数 {other}")),
                other => args.inputs.push(PathBuf::from(other)),
            }
        }

        if args.inputs.is_empty() && args.from_cache.is_none() {
            return Err("至少给一个输入路径（--dir / 文件 / --from-cache）".to_string());
        }
        if args.window <= 0.0 || args.offsets.iter().any(|&o| o < 0.0) {
            return Err("窗口与偏移必须为非负数".to_string());
        }
        Ok(Some(args))
    }
}

fn need<I: Iterator<Item = String>>(raw: &mut I, flag: &str) -> Result<String, String> {
    raw.next().ok_or_else(|| format!("{flag} 缺少取值"))
}

fn parse_f64(text: &str, flag: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .map_err(|_| format!("{flag} 需要数字，收到 {text}"))
}

fn parse_offsets(text: &str) -> Result<Vec<f64>, String> {
    let mut offsets = Vec::new();
    for part in text.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        offsets.push(parse_f64(trimmed, "--offsets")?);
    }
    if offsets.is_empty() {
        return Err("--offsets 为空".to_string());
    }
    Ok(offsets)
}

fn print_help() {
    println!(
        "bpm_diagnose — BPM 标定的批量回归 runner\n\
         \n\
         用法:\n\
           bpm_diagnose --dir <目录> [--truth <真值.csv>] [选项]\n\
           bpm_diagnose <文件> [<文件> ...] [选项]\n\
         \n\
         选项:\n\
           -d, --dir <目录>        递归收集支持的音频文件（可重复）\n\
           -t, --truth <文件>      两列 CSV: path,bpm（文件名匹配亦可）\n\
               --offsets <秒,秒>   分析起点，默认 {:.0},{:.0},{:.0}\n\
               --window <秒>       分析窗长，默认 {:.0}\n\
               --tightness <值>    DP 刚度，默认 {:.0}\n\
               --tolerance <比例>  精确命中容差，默认 0.02\n\
               --consistency <比例> 跨起点一致容差，默认 0.01\n\
               --out <文件>        逐次运行明细 CSV，默认 bpm-runs.csv\n\
               --candidates <文件> 逐候选项明细 CSV，默认不写\n\
               --beats <文件>      导出胜出假设的拍点帧号（csv）\n\
               --curve             曲线模式：整曲滑动扫描并合并成速度段\n\
               --vote              对比模式：单窗结果 vs 三窗共识\n\
               --curve-window <秒> 曲线模式的窗长，默认 12\n\
               --curve-hop <秒>    曲线模式的步长，默认 4\n\
               --jobs <N>          并行线程数，0 = 自动\n\
           -h, --help              显示本帮助",
        ANALYSIS_SKIP_SECS,
        ANALYSIS_SKIP_SECS + 20.0,
        ANALYSIS_SKIP_SECS + 40.0,
        ANALYSIS_WINDOW_SECS,
        DP_TIGHTNESS
    );
}

fn run(args: &Args) -> Result<(), String> {
    let files = match &args.from_cache {
        Some(cache) => load_cache_paths(cache)?,
        None => collect_files(&args.inputs, args.depth, args.exclude.as_deref()),
    };
    if files.is_empty() {
        return Err("没有找到可分析的音频文件".to_string());
    }

    let truth = match &args.truth {
        Some(path) => load_truth(path)?,
        None => HashMap::new(),
    };

    if args.curve {
        return run_curve(args, &files);
    }
    if args.vote {
        return run_vote(&files);
    }
    if args.suspects {
        return run_suspects(args, &files);
    }
    if args.meter {
        return run_meter(args, &files);
    }
    if args.production {
        for file in &files {
            let Some(path) = file.to_str() else { continue };
            let value = analyze_production(path).map(|estimate| estimate.bpm);
            println!(
                "{:<60}{}",
                file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned()),
                value.map_or_else(|| "—".to_string(), |bpm| format!("{bpm:.1}"))
            );
        }
        return Ok(());
    }

    println!(
        "文件 {} 个 × 起点 {} 个 = {} 次分析；窗长 {:.0}s，刚度 {:.0}，真值 {} 条",
        files.len(),
        args.offsets.len(),
        files.len() * args.offsets.len(),
        args.window,
        args.tightness,
        truth.len()
    );
    if truth.is_empty() {
        println!("（没有真值：只统计耗时、八度分布与跨起点一致性）");
    }
    warn_out_of_range(&truth);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build()
        .map_err(|error| format!("线程池创建失败：{error}"))?;

    let done = AtomicUsize::new(0);
    let total = files.len();
    let started = Instant::now();

    let mut runs: Vec<Run> = pool.install(|| {
        files
            .par_iter()
            .flat_map_iter(|file| {
                let produced = analyze_file(file, args);
                let count = done.fetch_add(1, Ordering::Relaxed) + 1;
                if count % 10 == 0 || count == total {
                    eprintln!("  {count}/{total}");
                }
                produced.into_iter()
            })
            .collect()
    });

    runs.sort_by(|a, b| a.file.cmp(&b.file).then(a.offset.total_cmp(&b.offset)));

    write_runs_csv(&args.runs_out, &runs, &truth, args)?;
    if let Some(path) = &args.candidates_out {
        write_candidates_csv(path, &runs)?;
    }
    if let Some(path) = &args.beats_out {
        write_beats_csv(path, &runs)?;
    }

    print_summary(&runs, &truth, args, started.elapsed());
    println!("\n逐次运行明细 → {}", args.runs_out.display());
    if let Some(path) = &args.candidates_out {
        println!("逐候选项明细 → {}", path.display());
    }
    Ok(())
}

/// 拍号可行性探针：若每小节 N 拍，第 0、N、2N… 个拍点的起音应显著更强。
/// 连已知拍号的曲子都对不上，就不该做拍号分割。
fn run_meter(args: &Args, files: &[PathBuf]) -> Result<(), String> {
    for file in files {
        let Some(path) = file.to_str() else { continue };
        println!(
            "
=== {} ===",
            file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned())
        );
        for &offset in &args.offsets {
            let Some(diagnostics) = diagnose_tuned(path, offset, args.window, args.tightness) else {
                continue;
            };
            let (Some(bpm), strengths) = (diagnostics.bpm, &diagnostics.beat_strengths) else {
                continue;
            };
            let count = strengths.len();
            if count < 16 {
                println!("  起点 {offset:>4.0}s  拍数不足（{count}）");
                continue;
            }
            let mean = strengths.iter().sum::<f64>() / count as f64;
            let variance: f64 = strengths.iter().map(|v| (v - mean).powi(2)).sum();
            if variance <= f64::EPSILON {
                continue;
            }
            let mut best = (0_usize, f64::NEG_INFINITY);
            let mut profile = String::new();
            for lag in 2..=12 {
                if count <= lag + 4 {
                    break;
                }
                // 只用重叠区间，并按重叠长度补偿，否则长滞后会被系统性低估
                let numerator: f64 = (0..count - lag)
                    .map(|i| (strengths[i] - mean) * (strengths[i + lag] - mean))
                    .sum();
                let r = numerator / variance * (count as f64 / (count - lag) as f64);
                profile.push_str(&format!("{lag}:{r:+.2} "));
                if r > best.1 {
                    best = (lag, r);
                }
            }
            println!(
                "  起点 {:>4.0}s  BPM {:>6.1}  拍数 {:>4}  重音周期 {:>2} 拍   {profile}",
                offset, bpm, count, best.0
            );
        }
    }
    Ok(())
}

/// 可疑清单：按证据强度列出"可能报错层级"的曲目，交人工核对。
/// 门槛比生产的提升规则松，只捞边界样本，不自动改值。
fn run_suspects(args: &Args, files: &[PathBuf]) -> Result<(), String> {
    println!("分析 {} 首，共 {} 次解码…", files.len(), files.len() * 4);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build()
        .map_err(|error| format!("线程池创建失败：{error}"))?;
    let mut rows: Vec<SuspectRow> = pool
        .install(|| files.par_iter().filter_map(|file| analyze_suspect(file)).collect());

    rows.sort_by(|a, b| {
        hint_rank(&a.hint)
            .cmp(&hint_rank(&b.hint))
            .then(a.file.cmp(&b.file))
    });

    let mut out = String::from(
        "file,bpm,base_bpm,promoted,confidence,current_str,alt15_bpm,alt15_str,alt15_rec,\
         alt20_bpm,alt20_str,alt20_rec,alt067_bpm,alt067_str,alt067_rec,hint\n",
    );
    for row in &rows {
        let mut fields = vec![
            row.file.clone(),
            opt(row.bpm, 1),
            format!("{:.1}", row.base_bpm),
            u8::from(row.promoted).to_string(),
            format!("{:.4}", row.confidence),
            format!("{:.4}", row.current_strength),
        ];
        for index in 0..3 {
            match row.alts.get(index).and_then(Option::as_ref) {
                Some(alt) => {
                    fields.push(format!("{:.1}", alt.bpm));
                    fields.push(format!("{:.3}", alt.strength_ratio));
                    fields.push(format!("{:.3}", alt.recall_ratio));
                }
                None => {
                    fields.push(String::new());
                    fields.push(String::new());
                    fields.push(String::new());
                }
            }
        }
        fields.push(row.hint.clone());
        out.push_str(&csv_row(&fields));
        out.push('\n');
    }
    fs::write(&args.runs_out, out)
        .map_err(|error| format!("写 {} 失败：{error}", args.runs_out.display()))?;

    // 最该复核的一类：生产路径已经靠 3/2 提升改过值的。改对了是收益，改错了就是新引入的错误
    let promoted: Vec<&SuspectRow> = rows
        .iter()
        .filter(|row| row.promoted)
        .collect();
    println!(
        "\n生产路径靠 3/2 提升改过值的：{} 首（这些最该复核）\n",
        promoted.len()
    );
    for row in promoted.iter().take(40) {
        let name: String = row
            .file
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .chars()
            .take(40)
            .collect();
        println!(
            "{:<42}{:>7} → {:>7}",
            name,
            format!("{:.0}", row.base_bpm),
            row.bpm
                .map_or_else(|| "—".to_string(), |value| format!("{value:.0}"))
        );
    }

    let flagged: Vec<&SuspectRow> = rows.iter().filter(|row| !row.hint.is_empty()).collect();
    println!(
        "\n{} 首里 {} 首有其它可疑线索（前 30 条）：\n",
        rows.len(),
        flagged.len()
    );
    for row in flagged.iter().take(30) {
        let name: String = row.file.rsplit(['/', '\\']).next().unwrap_or("").chars().take(44).collect();
        println!(
            "{:<46}{:>7}  {}",
            name,
            row.bpm.map_or_else(|| "—".to_string(), |bpm| format!("{bpm:.0}")),
            row.hint
        );
    }
    println!("\n完整清单 → {}", args.runs_out.display());
    Ok(())
}

struct AltLevel {
    bpm: f64,
    strength_ratio: f64,
    recall_ratio: f64,
}

struct SuspectRow {
    file: String,
    /// 生产值（含多窗共识与 3/2 提升）
    bpm: Option<f64>,
    /// 提升前的最高分候选（即 1× 层级的原始结论），用来看 3/2 规则有没有介入
    base_bpm: f64,
    /// 生产值是否是被 3/2 提升得到的
    promoted: bool,
    confidence: f64,
    current_strength: f64,
    /// 依次是 ×1.5、×2、×2/3；对应层级没有候选时保留 `None` 占位，
    /// 不能跳过——否则后面的层级会顶上来，被标成错误的倍数
    alts: Vec<Option<AltLevel>>,
    hint: String,
}

fn hint_rank(hint: &str) -> u8 {
    if hint.contains("×1.5") {
        0
    } else if hint.contains("×2") {
        1
    } else if hint.contains("报高") {
        2
    } else if hint.contains("置信度") {
        3
    } else {
        4
    }
}

fn analyze_suspect(file: &Path) -> Option<SuspectRow> {
    let path = file.to_str()?;
    let diagnostics = diagnose_tuned(path, ANALYSIS_SKIP_SECS, ANALYSIS_WINDOW_SECS, DP_TIGHTNESS)?;
    let bpm = analyze_production(path).map(|estimate| estimate.bpm).or(diagnostics.bpm)?;

    // 与生产口径保持一致：强度/召回都相对"当前报的那个层级"，不是相对得分最高者
    let current = closest_level(&diagnostics.evaluated, bpm)?;
    let mut alts: Vec<Option<AltLevel>> = Vec::new();
    for ratio in [1.5_f64, 2.0, 2.0 / 3.0] {
        alts.push(closest_level(&diagnostics.evaluated, bpm * ratio).map(|candidate| AltLevel {
            bpm: candidate.bpm,
            strength_ratio: candidate.strength / current.strength.max(f64::EPSILON),
            recall_ratio: candidate.recall / current.recall.max(f64::EPSILON),
        }));
    }

    let top = diagnostics
        .evaluated
        .iter()
        .max_by(|a, b| a.score.total_cmp(&b.score))?;
    let base_bpm = top.bpm;
    // 任何层级提升都算（3/2 与 2 倍），不能只认 1.5
    let promoted = base_bpm > 0.0 && (bpm / base_bpm - 1.0).abs() > 0.06;
    let hint = suspect_hint(&alts, diagnostics.strength.unwrap_or(0.0));
    Some(SuspectRow {
        file: file.to_string_lossy().to_string(),
        bpm: Some(bpm),
        base_bpm,
        promoted,
        confidence: diagnostics.strength.unwrap_or(0.0),
        current_strength: current.strength,
        alts,
        hint,
    })
}

/// 取离 `target` 最近的候选（对数域 6% 以内），没有就返回 None
fn closest_level<'a>(
    evaluated: &'a [CandidateScore],
    target: f64,
) -> Option<&'a CandidateScore> {
    if target <= 0.0 {
        return None;
    }
    evaluated
        .iter()
        .map(|candidate| {
            let distance = if candidate.bpm > 0.0 {
                (candidate.bpm / target).log2().abs()
            } else {
                f64::INFINITY
            };
            (distance, candidate)
        })
        .filter(|(distance, _)| *distance < 0.06)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, candidate)| candidate)
}

fn suspect_hint(alts: &[Option<AltLevel>], confidence: f64) -> String {
    if let Some(alt) = alts.first().and_then(Option::as_ref) {
        if alt.strength_ratio >= 0.55 && alt.recall_ratio >= 1.10 {
            return format!(
                "可能应 ×1.5（{} 强度 {:.2}× 召回 {:.2}×）",
                alt.bpm.round(),
                alt.strength_ratio,
                alt.recall_ratio
            );
        }
    }
    if let Some(alt) = alts.get(2).and_then(Option::as_ref) {
        if alt.strength_ratio >= 0.75 && alt.recall_ratio >= 1.10 {
            return format!(
                "可能报高 ×2/3（{} 强度 {:.2}× 召回 {:.2}×）",
                alt.bpm.round(),
                alt.strength_ratio,
                alt.recall_ratio
            );
        }
    }
    if confidence > 0.0 && confidence < 0.10 {
        return format!("置信度低（{confidence:.3}）");
    }
    String::new()
}

/// 对比模式：同一文件跑单窗（生产口径）与三窗共识，看投票改了什么。
fn run_vote(files: &[PathBuf]) -> Result<(), String> {
    println!("{:<38}{:>9}{:>9}{:>9}", "曲目", "单窗", "共识", "差");
    let mut changed = 0;
    let mut total = 0;
    for file in files {
        let Some(path) = file.to_str() else { continue };
        let single = diagnose_tuned(path, ANALYSIS_SKIP_SECS, ANALYSIS_WINDOW_SECS, DP_TIGHTNESS)
            .and_then(|diagnostics| diagnostics.bpm);
        let voted = analyze_consensus(path).map(|estimate| estimate.bpm);
        let name = file
            .file_name()
            .map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned());

        let delta = match (single, voted) {
            (Some(a), Some(b)) => format!("{:+.1}%", (b - a) / a * 100.0),
            _ => "—".to_string(),
        };
        if let (Some(a), Some(b)) = (single, voted) {
            total += 1;
            if (b - a).abs() / a > 0.005 {
                changed += 1;
            }
        }
        println!(
            "{:<38}{:>9}{:>9}{:>9}",
            name.chars().take(36).collect::<String>(),
            opt(single, 1),
            opt(voted, 1),
            delta
        );
    }
    println!("\n{changed}/{total} 首的共识结果与单窗不同（>0.5%）");
    Ok(())
}

/// 曲线模式：滑动扫描整曲，把相邻同速窗口合并成速度段。`--offsets` 只能给单点，
/// 而变速曲的正确形态是一条折线。
///
/// 合并阈值 12%：4 秒步长、12 秒窗下渐变会让相邻窗口中位数差约 10%，再紧会把渐变切碎。
fn run_curve(args: &Args, files: &[PathBuf]) -> Result<(), String> {
    let mut curve_rows: Vec<[String; 5]> = Vec::new();
    for file in files {
        let Some(path) = file.to_str() else { continue };
        println!(
            "\n=== {} ===",
            file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned())
        );

        let mut samples: Vec<(f64, Option<f64>, f64)> = Vec::new();
        let mut offset = 0.0;
        let mut misses = 0_u32;
        // 两小时或者连续两个窗口解不出来就停：后者说明到文件尾了
        while offset < 7_200.0 && misses < 2 {
            match diagnose_tuned(path, offset, args.curve_window, args.tightness) {
                Some(diagnostics) => {
                    misses = 0;
                    samples.push((
                        offset,
                        diagnostics.bpm,
                        diagnostics.strength.unwrap_or(0.0),
                    ));
                }
                None => misses += 1,
            }
            offset += args.curve_hop;
        }

        if samples.is_empty() {
            println!("解不出任何窗口");
            continue;
        }
        let end = samples.last().map_or(0.0, |s| s.0) + args.curve_window;
        let decoded = samples.iter().filter(|s| s.1.is_some()).count();
        println!(
            "时长 {:.0}s，扫描 {} 个窗口（{:.0}s 窗 / {:.0}s 步），出值 {} 个",
            end,
            samples.len(),
            args.curve_window,
            args.curve_hop,
            decoded
        );

        let segments = merge_segments(&samples);
        println!("\n{:<9}{:<9}{:<7}{:>8}{:>10}", "起", "止", "时长", "BPM", "窗口数");
        for segment in &segments {
            println!(
                "{:<9}{:<9}{:<7}{:>8.1}{:>10}",
                clock(segment.start),
                clock(segment.end),
                format!("{:.0}s", segment.end - segment.start),
                segment.bpm,
                segment.windows
            );
        }

        // 稳定性：窗口间跳变次数比"给了哪些段"更能说明这个结论能不能信
        let values: Vec<f64> = samples.iter().filter_map(|s| s.1).collect();
        let jumps = values
            .windows(2)
            .filter(|pair| (pair[1] / pair[0]).log2().abs() > 0.10)
            .count();
        println!(
            "\n出值窗口 {} 个：中位 {:.1} BPM，范围 {:.1}–{:.1}，相邻窗口跳变 {jumps} 次",
            values.len(),
            median(&values),
            values.iter().copied().fold(f64::INFINITY, f64::min),
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        );

        // 原始采样攒起来最后一起写：多文件时共用一份 CSV，靠 file 列区分
        let label = file.to_string_lossy().to_string();
        for (offset, bpm, strength) in &samples {
            curve_rows.push([
                label.clone(),
                format!("{offset:.1}"),
                format!("{:.1}", offset + args.curve_window),
                opt(*bpm, 2),
                format!("{strength:.4}"),
            ]);
        }
    }

    if !curve_rows.is_empty() {
        let mut out = String::from("file,offset_sec,end_sec,bpm,strength\n");
        for row in &curve_rows {
            out.push_str(&csv_row(row));
            out.push('\n');
        }
        fs::write(&args.runs_out, out)
            .map_err(|error| format!("写 {} 失败：{error}", args.runs_out.display()))?;
        println!("\n逐窗采样 → {}", args.runs_out.display());
    }
    Ok(())
}

struct Segment {
    start: f64,
    end: f64,
    bpm: f64,
    windows: usize,
}

fn merge_segments(samples: &[(f64, Option<f64>, f64)]) -> Vec<Segment> {
    // 3 点中位数平滑：变速曲上单个窗口常在相邻次谐波间翻，不平滑会被孤立段撑爆
    let smoothed: Vec<(f64, Option<f64>)> = samples
        .iter()
        .enumerate()
        .map(|(index, &(offset, bpm, _))| {
            let mut window: Vec<f64> = (index.saturating_sub(1)..=(index + 1).min(samples.len() - 1))
                .filter_map(|i| samples[i].1)
                .collect();
            if bpm.is_none() || window.is_empty() {
                return (offset, bpm);
            }
            window.sort_by(f64::total_cmp);
            (offset, Some(window[window.len() / 2]))
        })
        .collect();

    let mut segments: Vec<Segment> = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();

    for (offset, bpm) in smoothed {
        let Some(bpm) = bpm else {
            flush(&mut segments, &mut current);
            continue;
        };
        let joinable =
            current.last().is_some_and(|&(_, previous)| (bpm / previous).log2().abs() < 0.10);
        if !joinable {
            flush(&mut segments, &mut current);
        }
        current.push((offset, bpm));
    }
    flush(&mut segments, &mut current);

    // 单窗口段并进前一段：单窗结论在变速曲上噪声太大，单独列只会显得更碎
    let mut merged: Vec<Segment> = Vec::new();
    for segment in segments {
        match merged.last_mut() {
            Some(previous) if segment.windows < 2 => {
                previous.end = segment.end;
                previous.windows += segment.windows;
            }
            _ => merged.push(segment),
        }
    }
    merged
}

fn flush(segments: &mut Vec<Segment>, current: &mut Vec<(f64, f64)>) {
    if current.is_empty() {
        return;
    }
    let bpms: Vec<f64> = current.iter().map(|&(_, bpm)| bpm).collect();
    segments.push(Segment {
        start: current[0].0,
        end: current[current.len() - 1].0,
        bpm: median(&bpms),
        windows: current.len(),
    });
    current.clear();
}

fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

struct Run {
    file: PathBuf,
    offset: f64,
    bpm: Option<f64>,
    /// 文件自带的 BPM 标签：没有真值表时，它是最廉价的一份对照
    tag_bpm: Option<f64>,
    confidence: Option<f64>,
    strength: Option<f64>,
    beats: usize,
    /// 胜出假设的拍点帧号（只在需要 `--beats` 时保留）
    beat_frames: Vec<usize>,
    candidates: Vec<CandidateScore>,
    elapsed_ms: f64,
    /// 解码失败（`diagnose_tuned` 返回 None）时为 true
    failed: bool,
}

fn analyze_file(file: &Path, args: &Args) -> Vec<Run> {
    let Some(path) = file.to_str() else {
        eprintln!("跳过非 UTF-8 路径：{}", file.display());
        return Vec::new();
    };
    // 每个文件只读一次标签，三个起点共用
    let tag_bpm = read_tagged_bpm(path);

    args.offsets
        .iter()
        .map(|&offset| {
            let started = Instant::now();
            let diagnostics = diagnose_tuned(path, offset, args.window, args.tightness);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match diagnostics {
                Some(diagnostics) => Run {
                    file: file.to_path_buf(),
                    offset,
                    bpm: diagnostics.bpm,
                    tag_bpm,
                    confidence: diagnostics.confidence,
                    strength: diagnostics.strength,
                    beats: diagnostics.beats.len(),
                    beat_frames: diagnostics.beats,
                    candidates: diagnostics.evaluated,
                    elapsed_ms,
                    failed: false,
                },
                None => Run {
                    file: file.to_path_buf(),
                    offset,
                    bpm: None,
                    tag_bpm,
                    confidence: None,
                    strength: None,
                    beats: 0,
                    beat_frames: Vec::new(),
                    candidates: Vec::new(),
                    elapsed_ms,
                    failed: true,
                },
            }
        })
        .collect()
}

fn collect_files(inputs: &[PathBuf], depth: usize, exclude: Option<&Path>) -> Vec<PathBuf> {
    let excluded = exclude.map(load_exclude).unwrap_or_default();
    let mut files = Vec::new();
    for input in inputs {
        if input.is_file() {
            files.push(input.clone());
        } else if input.is_dir() {
            for entry in WalkDir::new(input)
                .max_depth(depth)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
            {
                if entry.file_type().is_file()
                    && has_audio_extension(entry.path())
                    && !is_excluded(&excluded, entry.path())
                {
                    files.push(entry.into_path());
                }
            }
        } else {
            eprintln!("跳过不存在的路径：{}", input.display());
        }
    }
    files.sort();
    files.dedup();
    files
}

/// 排除清单：每行一个完整路径或文件名，`#` 开头忽略。返回归一化后的键集合
fn load_exclude(path: &Path) -> std::collections::HashSet<String> {
    let Ok(text) = fs::read_to_string(path) else {
        eprintln!("读不到排除清单 {}，按不排除处理", path.display());
        return std::collections::HashSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(normalize_path)
        .collect()
}

fn is_excluded(excluded: &std::collections::HashSet<String>, file: &Path) -> bool {
    if excluded.is_empty() {
        return false;
    }
    if excluded.contains(&normalize_path(&file.to_string_lossy())) {
        return true;
    }
    file.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| excluded.contains(&normalize_path(name)))
}

/// 从应用的 `library-cache.json` 读曲目清单。
///
/// 为什么不直接用目录扫描：应用有自己的扫描深度与扩展名白名单，用 `WalkDir` 复现容易差几个文件，
/// 而"哪些曲目被索引"这件事只有缓存说了算。诊断要对齐的是用户实际看到的集合。
fn load_cache_paths(cache: &Path) -> Result<Vec<PathBuf>, String> {
    let text = fs::read_to_string(cache)
        .map_err(|error| format!("读取曲库缓存失败 {}：{error}", cache.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("解析曲库缓存失败 {}：{error}", cache.display()))?;

    let mut files = Vec::new();
    let playlists = value
        .pointer("/libraryCache/playlists")
        .and_then(serde_json::Value::as_array)
        .ok_or("曲库缓存里没有 libraryCache.playlists")?;
    for playlist in playlists {
        if let Some(tracks) = playlist.get("files").and_then(serde_json::Value::as_array) {
            for track in tracks {
                if let Some(path) = track.get("path").and_then(serde_json::Value::as_str) {
                    let path = PathBuf::from(path);
                    // 缓存可能比磁盘旧：已经删掉的文件直接跳过，不报错
                    if path.is_file() {
                        files.push(path);
                    }
                }
            }
        }
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err("曲库缓存里没有可用曲目".to_string());
    }
    Ok(files)
}

fn has_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str()))
}

/// 真值表：键是归一化后的完整路径与文件名，两种都能查到
/// 一条真值。`level_free` 为真表示这首是变速曲目、或公开来源在多个计层级间分歧，
/// 只能按"层级命中"判定——把 230 和 77 当成同一个答案的两种计法，而不是一错一对。
#[derive(Clone, Copy)]
struct TruthEntry {
    bpm: f64,
    level_free: bool,
}

fn load_truth(path: &Path) -> Result<HashMap<String, TruthEntry>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("读取真值表失败 {}：{error}", path.display()))?;
    let mut truth = HashMap::new();

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = parse_csv_line(line);
        if fields.len() < 2 {
            continue;
        }
        let Ok(bpm) = fields[1].trim().parse::<f64>() else {
            // 表头或注释行：跳过而不是报错，手写真值表常见
            if index == 0 || fields[0].trim().eq_ignore_ascii_case("path") {
                continue;
            }
            eprintln!("真值表第 {} 行不是数字，已跳过：{}", index + 1, line);
            continue;
        };
        if bpm <= 0.0 {
            continue;
        }
        // 第三列可选：写 `level` 表示按层级命中判定（默认 strict）
        let level_free = fields
            .get(2)
            .is_some_and(|mode| mode.trim().eq_ignore_ascii_case("level"));
        let entry = TruthEntry { bpm, level_free };
        let key = normalize_path(&fields[0]);
        truth.insert(key, entry);
        if let Some(name) = Path::new(fields[0].trim()).file_name().and_then(|n| n.to_str()) {
            truth.entry(normalize_path(name)).or_insert(entry);
        }
    }

    if truth.is_empty() {
        return Err(format!("真值表 {} 里没有有效行", path.display()));
    }
    Ok(truth)
}

fn normalize_path(text: &str) -> String {
    text.trim().replace('\\', "/").to_lowercase()
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted => {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            }
            '"' => quoted = true,
            ',' if !quoted => fields.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    fields.push(current);
    fields
}

fn truth_for(truth: &HashMap<String, TruthEntry>, file: &Path) -> Option<f64> {
    let full = normalize_path(&file.to_string_lossy());
    if let Some(entry) = truth.get(&full) {
        return Some(entry.bpm);
    }
    let name = file.file_name()?.to_str()?;
    truth.get(&normalize_path(name)).map(|entry| entry.bpm)
}

/// 这首是否只能按"层级命中"判定
fn level_free_for(truth: &HashMap<String, TruthEntry>, file: &Path) -> bool {
    let full = normalize_path(&file.to_string_lossy());
    if let Some(entry) = truth.get(&full) {
        return entry.level_free;
    }
    file.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| truth.get(&normalize_path(name)))
        .is_some_and(|entry| entry.level_free)
}

/// 层级命中：允许 1、3/2、2、3 倍与 1/2、2/3、1/3 这些合法计层级。
/// 同一小节存在多个公开读数（四分/附点/二分）的曲目，判"错"没有意义。
fn is_level_hit(bpm: f64, truth: f64, tolerance: f64) -> bool {
    [1.0, 1.5, 2.0, 3.0, 0.5, 2.0 / 3.0, 1.0 / 3.0]
        .iter()
        .any(|&factor| relative_error(bpm, truth * factor) <= tolerance)
}

/// 真值超出 [`BPM_MIN`, `BPM_MAX`] 时先提醒：这类曲目在当前实现里结构上不可能报对
fn warn_out_of_range(truth: &HashMap<String, TruthEntry>) {
    let mut outside: Vec<(&String, &TruthEntry)> = truth
        .iter()
        .filter(|&(_, entry)| !(BPM_MIN..=BPM_MAX).contains(&entry.bpm))
        .collect();
    if outside.is_empty() {
        return;
    }
    outside.sort_by(|a, b| a.0.cmp(b.0));
    println!(
        "警告：{} 条真值落在 [{BPM_MIN:.0}, {BPM_MAX:.0}] 之外，当前候选搜索范围够不到它们：",
        outside.len()
    );
    for (key, entry) in outside.iter().take(10) {
        println!("  {key} = {:.1}", entry.bpm);
    }
    if outside.len() > 10 {
        println!("  ...（其余 {} 条省略）", outside.len() - 10);
    }
}

fn relative_error(bpm: f64, truth: f64) -> f64 {
    (bpm - truth).abs() / truth
}

fn is_exact(bpm: f64, truth: f64, tolerance: f64) -> bool {
    relative_error(bpm, truth) <= tolerance
}

fn is_octave(bpm: f64, truth: f64, tolerance: f64) -> bool {
    [0.25, 0.5, 1.0, 2.0, 4.0]
        .iter()
        .any(|&factor| relative_error(bpm, truth * factor) <= tolerance)
}

/// 把 bpm 折到离真值最近的八度，返回相对偏差（用于量速率偏置）
fn octave_residual(bpm: f64, truth: f64) -> f64 {
    let factor = (bpm / truth).log2().round();
    (bpm / (truth * 2f64.powf(factor))) - 1.0
}

fn opt(value: Option<f64>, precision: usize) -> String {
    value.map_or_else(String::new, |v| format!("{v:.precision$}"))
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_row(fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| csv_field(field))
        .collect::<Vec<_>>()
        .join(",")
}

fn write_runs_csv(
    path: &Path,
    runs: &[Run],
    truth: &HashMap<String, TruthEntry>,
    args: &Args,
) -> Result<(), String> {
    let mut out = String::from(
        "file,offset_sec,bpm,status,confidence,strength,beats,candidate_count,\
         truth_bpm,tag_bpm,exact_hit,octave_hit,residual_pct,elapsed_ms,base_bpm,level_ratio\n",
    );

    for run in runs {
        let reference = truth_for(truth, &run.file);
        // 层级判断的中间量：提升前的最高分候选，以及最终的层级倍数
        let base_bpm = run
            .candidates
            .iter()
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .map(|candidate| candidate.bpm);
        let level_ratio = match (run.bpm, base_bpm) {
            (Some(bpm), Some(base)) if base > 0.0 => Some(bpm / base),
            _ => None,
        };
        let status = match (run.failed, run.bpm) {
            (true, _) => "decode_failed",
            (false, Some(_)) => "accepted",
            (false, None) => "below_threshold",
        };
        let exact = match (run.bpm, reference) {
            (Some(bpm), Some(truth)) => u8::from(is_exact(bpm, truth, args.tolerance)).to_string(),
            _ => String::new(),
        };
        let octave = match (run.bpm, reference) {
            (Some(bpm), Some(truth)) => u8::from(is_octave(bpm, truth, args.tolerance)).to_string(),
            _ => String::new(),
        };
        let residual = match (run.bpm, reference) {
            (Some(bpm), Some(truth)) if is_octave(bpm, truth, args.tolerance) => {
                format!("{:.2}", octave_residual(bpm, truth) * 100.0)
            }
            _ => String::new(),
        };

        out.push_str(&csv_row(&[
            run.file.to_string_lossy().to_string(),
            format!("{:.1}", run.offset),
            opt(run.bpm, 2),
            status.to_string(),
            opt(run.confidence, 4),
            opt(run.strength, 4),
            run.beats.to_string(),
            run.candidates.len().to_string(),
            opt(reference, 2),
            opt(run.tag_bpm, 2),
            exact,
            octave,
            residual,
            format!("{:.1}", run.elapsed_ms),
            base_bpm.map_or_else(String::new, |value| format!("{value:.2}")),
            level_ratio.map_or_else(String::new, |value| format!("{value:.3}")),
        ]));
        out.push('\n');
    }

    fs::write(path, out).map_err(|error| format!("写 {} 失败：{error}", path.display()))
}

/// 导出胜出假设的拍点帧号。
///
/// 用来离线复算"换一种拟合方式会得到什么"——比如判断某个错值是拟合被少数拍点带偏，
/// 还是整条链本来就锁错了周期。这两者的修法完全不同，不能靠猜。
fn write_beats_csv(path: &Path, runs: &[Run]) -> Result<(), String> {
    let mut out = String::from("file,offset_sec,beat_index,frame\n");
    for run in runs {
        for (index, frame) in run.beat_frames.iter().enumerate() {
            out.push_str(&csv_row(&[
                run.file.to_string_lossy().to_string(),
                format!("{:.1}", run.offset),
                index.to_string(),
                frame.to_string(),
            ]));
            out.push('\n');
        }
    }
    fs::write(path, out).map_err(|error| format!("写 {} 失败：{error}", path.display()))
}

fn write_candidates_csv(path: &Path, runs: &[Run]) -> Result<(), String> {
    let mut out = String::from(
        "file,offset_sec,rank,chosen,bpm,score,strength,enrichment,salience,\
         skipped_ratio,periodicity,regularity,coverage,recall,contrast,beats\n",
    );

    for run in runs {
        let mut ranked = run.candidates.clone();
        ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
        for (rank, candidate) in ranked.iter().enumerate() {
            // 胜出者不一定排在得分第一：3/2 层级提升会把排在后面的候选选出来
            let chosen = u8::from(
                run.bpm.is_some_and(|reported| {
                    reported > 0.0 && (candidate.bpm - reported).abs() / reported < 0.03
                }),
            );
            out.push_str(&csv_row(&[
                run.file.to_string_lossy().to_string(),
                format!("{:.1}", run.offset),
                rank.to_string(),
                chosen.to_string(),
                format!("{:.2}", candidate.bpm),
                format!("{:.5}", candidate.score),
                format!("{:.5}", candidate.strength),
                format!("{:.5}", candidate.enrichment),
                format!("{:.5}", candidate.salience),
                format!("{:.5}", candidate.skipped_ratio),
                format!("{:.5}", candidate.periodicity),
                format!("{:.5}", candidate.regularity),
                format!("{:.5}", candidate.coverage),
                format!("{:.5}", candidate.recall),
                format!("{:.3}", candidate.contrast),
                candidate.beats.to_string(),
            ]));
            out.push('\n');
        }
    }

    fs::write(path, out).map_err(|error| format!("写 {} 失败：{error}", path.display()))
}

#[derive(Default)]
struct OffsetStats {
    with_truth: usize,
    exact: usize,
    octave: usize,
    accepted: usize,
    /// 变速 / 多计层级曲目：单独统计，不混进上面那个分母
    level_total: usize,
    level_hit: usize,
}

fn print_summary(
    runs: &[Run],
    truth: &HashMap<String, TruthEntry>,
    args: &Args,
    elapsed: std::time::Duration,
) {
    let mut by_offset: Vec<(f64, OffsetStats)> = args
        .offsets
        .iter()
        .map(|&offset| (offset, OffsetStats::default()))
        .collect();

    for run in runs {
        let Some(entry) = by_offset
            .iter_mut()
            .find(|(offset, _)| (*offset - run.offset).abs() < 0.05)
        else {
            continue;
        };
        let stats = &mut entry.1;
        if run.bpm.is_some() {
            stats.accepted += 1;
        }
        if let (Some(bpm), Some(reference)) = (run.bpm, truth_for(truth, &run.file)) {
            if level_free_for(truth, &run.file) {
                stats.level_total += 1;
                if is_level_hit(bpm, reference, args.tolerance) {
                    stats.level_hit += 1;
                }
            } else {
                stats.with_truth += 1;
                if is_exact(bpm, reference, args.tolerance) {
                    stats.exact += 1;
                }
                if is_octave(bpm, reference, args.tolerance) {
                    stats.octave += 1;
                }
            }
        }
    }

    let failed = runs.iter().filter(|run| run.failed).count();
    println!("\n=== 分起点 ===");
    for (offset, stats) in &by_offset {
        // 不用固定列宽：中文在格式化里按 1 字符算，宽度对不齐，反而更难读
        println!(
            "起点 {offset:>3.0}s   有真值 {:>3}   精确命中 {}/{}   含八度 {}/{}   出值 {}",
            stats.with_truth,
            stats.exact,
            stats.with_truth,
            stats.octave,
            stats.with_truth,
            stats.accepted
        );
    }

    let mut all = OffsetStats::default();
    for (_, stats) in &by_offset {
        all.with_truth += stats.with_truth;
        all.exact += stats.exact;
        all.octave += stats.octave;
        all.level_total += stats.level_total;
        all.level_hit += stats.level_hit;
    }
    if all.with_truth > 0 {
        println!(
            "\n合计    精确命中 {}/{}（{:.0}%）  含八度 {}/{}（{:.0}%）",
            all.exact,
            all.with_truth,
            percent(all.exact, all.with_truth),
            all.octave,
            all.with_truth,
            percent(all.octave, all.with_truth)
        );
    }
    if all.level_total > 0 {
        println!(
            "变速/多计层级  层级命中 {}/{}（{:.0}%）——单独统计，不计入上面的精确命中",
            all.level_hit,
            all.level_total,
            percent(all.level_hit, all.level_total)
        );
    }
    if failed > 0 {
        println!("解码失败 {failed} 次（不计入上表）");
    }

    print_consistency(runs, args);
    print_tag_agreement(runs, args);
    print_invariants(runs, truth, args);

    let mut timings: Vec<f64> = runs.iter().map(|run| run.elapsed_ms).collect();
    let p50 = percentile(&mut timings, 0.50);
    let mut timings: Vec<f64> = runs.iter().map(|run| run.elapsed_ms).collect();
    let p95 = percentile(&mut timings, 0.95);
    println!(
        "\n耗时 p50 {p50:.0}ms  p95 {p95:.0}ms  最慢 {:.0}ms  总耗时 {:.1}s",
        runs.iter().map(|run| run.elapsed_ms).fold(0.0, f64::max),
        elapsed.as_secs_f64()
    );
}

fn print_consistency(runs: &[Run], args: &Args) {
    // 去重：同一文件有多个起点，逐个 run 遍历会把每个文件统计 N 次
    let mut files: Vec<&Path> = runs.iter().map(|run| run.file.as_path()).collect();
    files.sort_unstable();
    files.dedup();
    let total = files.len();

    let mut by_file: HashMap<&Path, Vec<f64>> = HashMap::new();
    for run in runs {
        if let Some(bpm) = run.bpm {
            by_file.entry(run.file.as_path()).or_default().push(bpm);
        }
    }

    let mut strict = 0;
    let mut loose = 0;
    let mut missing = 0;
    let expected = args.offsets.len();

    for file in files {
        let readings = by_file.get(file).map_or(&[][..], Vec::as_slice);
        if readings.len() < 2 {
            missing += 1;
            continue;
        }
        let median = median(readings);
        let spread = readings
            .iter()
            .map(|&bpm| (bpm - median).abs() / median)
            .fold(0.0_f64, f64::max);
        if spread <= args.consistency_tolerance {
            loose += 1;
            if readings.len() == expected {
                strict += 1;
            }
        }
    }

    println!("\n=== 跨起点一致性 ===");
    println!(
        "严格（{} 个起点都出值且互相一致）  {strict}/{total}",
        args.offsets.len()
    );
    println!("宽松（至少 2 个起点一致）         {loose}/{total}");
    println!("出值不足 2 个起点                 {missing}/{total}");
}

/// 用文件自带的 BPM 标签当对照（标签可能出错，但独立于算法）。
/// 生产流程标签优先，所以它量的是"这库没标签时会错多少"。
fn print_tag_agreement(runs: &[Run], args: &Args) {
    let mut with_tag = 0;
    let mut exact = 0;
    let mut octave = 0;
    for run in runs {
        let (Some(bpm), Some(tag)) = (run.bpm, run.tag_bpm) else {
            continue;
        };
        with_tag += 1;
        if is_exact(bpm, tag, args.tolerance) {
            exact += 1;
        }
        if is_octave(bpm, tag, args.tolerance) {
            octave += 1;
        }
    }

    if with_tag == 0 {
        return;
    }
    println!("\n=== 与文件标签对照 ===");
    println!(
        "有标签且出值 {with_tag} 次：精确一致 {exact}（{:.0}%），含八度一致 {octave}（{:.0}%）",
        percent(exact, with_tag),
        percent(octave, with_tag)
    );
}

fn print_invariants(runs: &[Run], truth: &HashMap<String, TruthEntry>, args: &Args) {
    let accepted: Vec<&Run> = runs.iter().filter(|run| run.bpm.is_some()).collect();
    let slow = accepted
        .iter()
        .filter(|run| run.bpm.unwrap_or(0.0) < 80.0)
        .count();
    let fast = accepted
        .iter()
        .filter(|run| run.bpm.unwrap_or(0.0) >= 175.0)
        .count();

    let residuals: Vec<f64> = accepted
        .iter()
        .filter_map(|run| {
            let bpm = run.bpm?;
            let reference = truth_for(truth, &run.file)?;
            is_octave(bpm, reference, args.tolerance)
                .then(|| octave_residual(bpm, reference))
        })
        .collect();

    println!("\n=== 速率偏置检查 ===");
    println!(
        "出值 {} 次：<80 BPM {slow} 次，≥175 BPM {fast} 次",
        accepted.len()
    );
    if residuals.is_empty() {
        println!("八度命中样本为 0，无法估计速率偏置");
    } else {
        let mean = residuals.iter().sum::<f64>() / residuals.len() as f64;
        println!(
            "八度命中 {} 次，速率偏置 {:+.2}%（负数=整体偏慢）",
            residuals.len(),
            mean * 100.0
        );
    }
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        100.0 * part as f64 / whole as f64
    }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn percentile(values: &mut [f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * fraction).round() as usize;
    values[index.min(values.len() - 1)]
}

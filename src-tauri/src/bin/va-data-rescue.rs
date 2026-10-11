// 数据回退急救程序（存储计划 Task 5b）。只干一件事：把 1.9 搬家搬走的用户数据放回配置根。
//
// 为什么不做成设置页里的按钮：内容根指向的盘没接上时，主程序自己都起不来，
// 界面里的"回退"按钮根本点不到。独立小程序不看主程序脸色。
//
// 为什么不进安装包：用户定的范围——平时不发，真出事时我们 `cargo build --release --bin
// va-data-rescue` 编一个 exe 单独发给需要的人。
//
// 复用 `voiceassist_lib::storage` 里那套已经写完并测过的复制+校验，
// 不在脚本里重写第二份（设计文档 §五："同一套复制+校验代码"）。
//
// 用法（cmd 里跑；若中文显示成乱码，先执行 `chcp 65001` 再运行本程序）：
//   va-data-rescue.exe                     交互式：列清单 → 等回车确认 → 回退
//   va-data-rescue.exe --list              只看会回退什么，不动任何文件
//   va-data-rescue.exe --yes               跳过回车确认（远程指导时用）
//   va-data-rescue.exe --config-root <路径> 调试用：指定配置根

use std::path::PathBuf;
use std::process::ExitCode;

use voiceassist_lib::storage::atomic::atomic_write;
use voiceassist_lib::storage::migrate;
use voiceassist_lib::storage::paths;

const USAGE: &str = "\
用法：va-data-rescue.exe [--list] [--yes] [--config-root <路径>]

把迁移到内容根的用户数据（消息/音频/克隆样本/日志等）复制回配置根，
并把存储指针指回配置根，让 TTSassist 重新按老位置读数据。
迁移备份一律保留，回退不满意还能再试。

  --list          只列出将要回退的条目与体积，不动任何文件
  --yes           跳过回车确认
  --config-root   指定配置根（默认 %APPDATA%\\com.voiceassist.app），调试用
";

struct Options {
    list_only: bool,
    assume_yes: bool,
    help: bool,
    config_root: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        list_only: false,
        assume_yes: false,
        help: false,
        config_root: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--list" => o.list_only = true,
            "--yes" => o.assume_yes = true,
            "--help" | "-h" => o.help = true,
            "--config-root" => {
                i += 1;
                let v = args.get(i).ok_or("--config-root 后面要跟一个路径")?;
                o.config_root = Some(PathBuf::from(v));
            }
            other => return Err(format!("不认识参数「{other}」，用 --help 看用法")),
        }
        i += 1;
    }
    Ok(o)
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn run(o: &Options) -> Result<(), String> {
    let config_root = o
        .config_root
        .clone()
        .or_else(paths::config_root_from_env)
        .ok_or("找不到配置根（环境变量 APPDATA 缺失），可以用 --config-root 指定")?;

    println!("配置根（旧位置）：{}", config_root.display());
    let pointer_path = config_root.join(paths::POINTER_FILE_NAME);
    match paths::read_pointer(&pointer_path) {
        Some(cur) => println!("指针记录的内容根（现在的位置）：{}", cur.display()),
        None => println!("指针：无（软件按默认位置找数据）"),
    }

    let backups = migrate::find_backups(&config_root);
    let Some(newest) = backups.last().cloned() else {
        println!("没有找到迁移备份目录（.migrated-backup-*），说明这台机器从没执行过搬家，无需回退。");
        return Ok(());
    };
    println!("本次回退用的备份：{}", newest.display());
    if backups.len() > 1 {
        // 只取最新一份，但必须把更早的告诉用户，否则他们会以为我们弄丢了
        println!("⚠ 这台机器上有 {} 个备份，本程序只回退最新的一个，更早的原样保留：", backups.len());
        for b in &backups[..backups.len() - 1] {
            println!("    {}", b.display());
        }
    }

    let items = migrate::backup_items(&newest);
    if items.is_empty() {
        println!("最新备份里没有可回退的内容条目（消息/音频/样本/日志等），什么也不做。");
        return Ok(());
    }

    println!("将回退这些条目到配置根：");
    let mut total = 0u64;
    for it in &items {
        let size = migrate::path_size(it);
        total += size;
        let name = it.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        println!("    {:>9} KB  {name}", size / 1024);
    }
    println!("合计 {:.1} MB", mb(total));

    let conflicts = migrate::restore_conflicts(&newest, &config_root);
    if !conflicts.is_empty() {
        return Err(format!(
            "配置根已经存在同名条目，回退会覆盖用户这半年新写的数据，已整批停止。请先手工处置再重试：{}",
            conflicts.join("、")
        ));
    }
    if o.list_only {
        println!("--list 模式：没有动任何文件。");
        return Ok(());
    }

    if !o.assume_yes {
        println!();
        println!("请先完全退出 TTSassist（右下角托盘图标也要右键退出），然后按回车继续（Ctrl+C 取消）：");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map_err(|e| format!("读取输入失败: {e}"))?;
    }

    let report = migrate::restore_items(&newest, &config_root)?;

    // 指针必须指回配置根，而不是像设计 §五 那样"删掉指针"：
    // 那是翻默认之前的写法。翻默认之后删指针 = 内容根退回 <exe>\data（空目录），
    // 用户刚恢复回来的数据反而看不见，等于我们自造一场"消息全丢"。
    let pointer = paths::pointer_json(&config_root, Some(&newest.display().to_string()));
    atomic_write(&pointer_path, pointer.as_bytes())
        .map_err(|e| format!("数据已回退，但写指针失败（可手工把下面这句写进 {}）：{e}", pointer_path.display()))?;

    println!(
        "已回退 {} 项 / {:.1} MB，备份仍保留在 {}",
        report.restored,
        mb(report.bytes),
        newest.display()
    );
    println!("指针已指回配置根，现在重新打开 TTSassist，消息与音频应当都在。");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("错误：{e}");
            return ExitCode::from(1);
        }
    };
    if opts.help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("失败：{e}");
            ExitCode::from(1)
        }
    }
}

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod board;
mod event_matrix;
mod geometry;
mod integration;
mod session;
mod shortcuts;

use std::path::PathBuf;

#[derive(Clone, Default)]
pub struct Options {
    pub root: PathBuf,
    pub demo: bool,
    pub smoke: bool,
    pub screenshot: Option<PathBuf>,
}

fn options() -> Result<Options, String> {
    let mut options = Options::default();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--data-dir" => options.root = arguments.next().ok_or("--data-dir 需要路径")?.into(),
            "--demo" => options.demo = true,
            "--smoke-test" => options.smoke = true,
            "--screenshot" => {
                options.screenshot =
                    Some(arguments.next().ok_or("--screenshot 需要 PNG 路径")?.into())
            }
            "--help" | "-h" => {
                println!(
                    "星晴\n  --data-dir PATH  独立数据目录\n  --demo           在空目录创建演示项目\n  --smoke-test     启动检查后退出\n  --screenshot PNG 原生窗口截图"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("未知参数：{argument}")),
        }
    }
    if options.root.as_os_str().is_empty() {
        options.root = std::env::var_os("AICOMIC_APP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(default_data_dir);
    }
    Ok(options)
}

fn default_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(home_dir) = std::env::var_os("HOME") {
        return PathBuf::from(home_dir).join("Library/Application Support/com.ljc.aicomictool");
    }
    #[cfg(target_os = "windows")]
    if let Some(roaming_dir) = std::env::var_os("APPDATA") {
        return PathBuf::from(roaming_dir).join("com.ljc.aicomictool");
    }
    directories::ProjectDirs::from("com", "ljc", "aicomictool")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("aicomic-data"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = options().map_err(std::io::Error::other)?;
    let store = aicomic_core::Store::open(&options.root)?;
    iced::application(
        move || app::App::new(store.clone(), options.clone()),
        app::App::update,
        app::App::view,
    )
    .title("星晴 · AI 漫剧创作")
    .window_size((1440, 900))
    .theme(app::App::theme)
    .subscription(app::App::subscription)
    .exit_on_close_request(false)
    .default_font(iced::Font::with_name(if cfg!(target_os = "macos") {
        "PingFang SC"
    } else {
        "Microsoft YaHei"
    }))
    .run()?;
    Ok(())
}

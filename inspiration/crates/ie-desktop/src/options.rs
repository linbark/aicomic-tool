use std::path::{Path, PathBuf};

use ie::studio::StudioConfig;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Library,
    Brew,
    Pills,
    Preferences,
}
impl Screen {
    pub const ALL: [Self; 4] = [Self::Library, Self::Brew, Self::Pills, Self::Preferences];
    pub fn title(self) -> &'static str {
        match self {
            Self::Library => "素材库",
            Self::Brew => "炼丹台",
            Self::Pills => "丹药",
            Self::Preferences => "我的偏好",
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "library" => Ok(Self::Library),
            "brew" => Ok(Self::Brew),
            "pills" => Ok(Self::Pills),
            "preferences" => Ok(Self::Preferences),
            _ => Err("--screen 只接受 library / brew / pills / preferences".into()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub config: StudioConfig,
    pub demo: bool,
    pub root_explicit: bool,
    pub appearance: String,
    pub screenshot: Option<PathBuf>,
    pub exit_after_capture: bool,
    pub screen: Screen,
    pub size: (f32, f32),
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>, cwd: &Path) -> Result<Self, String> {
        let mut options = Self {
            config: StudioConfig {
                db: PathBuf::new(),
                root: cwd.to_path_buf(),
                recipes: cwd.join("recipes"),
            },
            demo: false,
            root_explicit: false,
            appearance: "system".into(),
            screenshot: None,
            exit_after_capture: false,
            screen: Screen::Library,
            size: (1280., 860.),
        };
        let mut args = args.into_iter();
        let mut recipes = None;
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("{arg} 需要值"));
            match arg.as_str() {
                "--db" => options.config.db = absolute(cwd, value()?.into()),
                "--root" => {
                    options.config.root = absolute(cwd, value()?.into());
                    options.root_explicit = true;
                }
                "--recipes" => recipes = Some(absolute(cwd, value()?.into())),
                "--demo" => options.demo = true,
                "--theme" => {
                    let appearance = value()?;
                    if !["light", "dark", "system"].contains(&appearance.as_str()) {
                        return Err("--theme 只接受 light / dark / system".into());
                    }
                    options.appearance = appearance;
                }
                "--screenshot" => options.screenshot = Some(absolute(cwd, value()?.into())),
                "--exit-after-capture" => options.exit_after_capture = true,
                "--screen" => options.screen = Screen::parse(&value()?)?,
                "--window-size" => {
                    let size = value()?;
                    let (width, height) = size
                        .split_once('x')
                        .ok_or("--window-size 格式为 1280x860")?;
                    let width: f32 = width.parse().map_err(|_| "窗口宽度无效")?;
                    let height: f32 = height.parse().map_err(|_| "窗口高度无效")?;
                    if !width.is_finite()
                        || !height.is_finite()
                        || !(1040. ..=4096.).contains(&width)
                        || !(720. ..=4096.).contains(&height)
                    {
                        return Err("窗口最小为 1040x720，最大为 4096x4096".into());
                    }
                    options.size = (width, height);
                }
                _ => return Err(format!("未知参数：{arg}；使用 --help 查看帮助")),
            }
        }
        if !options.root_explicit
            && !options.config.db.as_os_str().is_empty()
            && let Some(parent) = options.config.db.parent()
        {
            options.config.root = parent.to_path_buf();
        }
        options.config.recipes = recipes.unwrap_or_else(|| options.config.root.join("recipes"));
        if options.demo && options.config.db.as_os_str().is_empty() {
            return Err(
                "--demo 需同时提供 --db 指向隔离体验数据库；不会创建或读取默认用户库".into(),
            );
        }
        if options.exit_after_capture && options.screenshot.is_none() {
            return Err("--exit-after-capture 必须配合 --screenshot".into());
        }
        if options.screenshot.as_ref().is_some_and(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_none_or(|e| !e.eq_ignore_ascii_case("png"))
        }) {
            return Err("截图输出必须是 .png 文件".into());
        }
        Ok(options)
    }
}
fn absolute(cwd: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

pub const HELP: &str = "炼丹炉 · 原生本地素材工作台\n\n  --db PATH       只读打开已有数据库（不迁移原库）\n  --root DIR      素材根目录\n  --recipes DIR   本地配方目录\n  --demo          隔离体验数据，不记录真人；必须显式提供 --db\n  --screen library|brew|pills|preferences\n  --theme system|light|dark  默认跟随系统；后两项用于隔离预览\n  --window-size 1280x860  窗口逻辑尺寸，最小1040x720\n  --screenshot PATH.png   原生 compositor 截图；不覆盖已有文件\n  --exit-after-capture    保存截图后退出\n\n不传 --db 时不读取任何数据库。写入前需在窗口中建立工作副本或新库。";

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(|s| s.to_string()), Path::new("/isolated"))
    }
    #[test]
    fn no_implicit_database() {
        assert!(parse(&[]).unwrap().config.db.as_os_str().is_empty());
    }
    #[test]
    fn demo_needs_explicit_database() {
        assert!(parse(&["--demo"]).is_err());
    }
    #[test]
    fn resolves_paths_and_screen() {
        let o = parse(&[
            "--db",
            "test.db",
            "--root",
            "assets",
            "--screen",
            "brew",
            "--window-size",
            "1040x720",
        ])
        .unwrap();
        assert_eq!(o.config.db, Path::new("/isolated/test.db"));
        assert_eq!(o.config.recipes, Path::new("/isolated/assets/recipes"));
        assert_eq!(o.screen, Screen::Brew);
    }
    #[test]
    fn rejects_bad_capture_and_sizes() {
        assert!(parse(&["--exit-after-capture"]).is_err());
        assert!(parse(&["--window-size", "NaNx860"]).is_err());
        assert!(parse(&["--screenshot", "x.jpg"]).is_err());
    }
}

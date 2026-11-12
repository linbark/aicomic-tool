#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod data;
mod options;
mod runtime;
mod style;
mod view;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", options::HELP);
        return Ok(());
    }
    let options =
        options::Options::parse(args, &std::env::current_dir()?).map_err(std::io::Error::other)?;
    let size = options.size;
    iced::application(
        move || app::Studio::new(options.clone()),
        app::Studio::update,
        app::Studio::view,
    )
    .title(|app: &app::Studio| format!("{} · 炼丹炉", app.screen.title()))
    .window(iced::window::Settings {
        size: iced::Size::new(size.0, size.1),
        min_size: Some(iced::Size::new(1040., 720.)),
        ..Default::default()
    })
    .theme(app::Studio::theme)
    .subscription(app::Studio::subscription)
    .exit_on_close_request(false)
    .default_font(iced::Font::with_name(if cfg!(target_os = "macos") {
        "PingFang SC"
    } else {
        "Microsoft YaHei"
    }))
    .run()?;
    Ok(())
}

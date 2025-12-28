// macOS 不需要 windows_subsystem；保留空属性避免编译报错

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{Manager, Window};

fn pick_free_port() -> u16 {
  TcpListener::bind("127.0.0.1:0")
    .expect("bind ephemeral port failed")
    .local_addr()
    .expect("local_addr failed")
    .port()
}

fn wait_port_open(port: u16, timeout: Duration) -> bool {
  let start = Instant::now();
  while start.elapsed() < timeout {
    if TcpStream::connect(("127.0.0.1", port)).is_ok() {
      return true;
    }
    thread::sleep(Duration::from_millis(100));
  }
  false
}

fn find_backend_root(resource_dir: Option<PathBuf>) -> Option<PathBuf> {
  // 1) packaged: resources/ 里包含 backend/
  if let Some(rd) = resource_dir {
    let candidate = rd.join("backend").join("app").join("main.py");
    if candidate.exists() {
      return Some(rd);
    }
  }
  // 2) dev: 以编译期路径推导 project root（frontend/src-tauri -> frontend -> project）
  let dev_root = Path::new(env!("CARGO_MANIFEST_DIR"))
    .parent() // frontend/
    .and_then(|p| p.parent()) // project/
    .map(|p| p.to_path_buf());
  if let Some(dr) = dev_root {
    if dr.join("backend").join("app").join("main.py").exists() {
      return Some(dr);
    }
  }
  None
}

fn spawn_backend(app_data_dir: &Path, port: u16, backend_root: &Path) -> std::io::Result<Child> {
  let db_path = app_data_dir.join("database.db");
  let data_dir = app_data_dir.join("data");
  std::fs::create_dir_all(&data_dir)?;

  // 让 Python 能 import backend.*
  let pythonpath = backend_root.to_string_lossy().to_string();

  let args = vec![
    "-m".to_string(),
    "uvicorn".to_string(),
    "backend.app.main:app".to_string(),
    "--host".to_string(),
    "127.0.0.1".to_string(),
    "--port".to_string(),
    port.to_string(),
  ];

  let mut cmd = Command::new("python3");
  cmd.args(args.clone());
  cmd.current_dir(backend_root);
  cmd.env("PYTHONPATH", pythonpath.clone());
  cmd.env("AICOMIC_DB_PATH", db_path.to_string_lossy().to_string());
  cmd.env("AICOMIC_DATA_DIR", data_dir.to_string_lossy().to_string());
  cmd.stdout(Stdio::null());
  cmd.stderr(Stdio::null());

  match cmd.spawn() {
    Ok(child) => Ok(child),
    Err(_) => {
      // fallback to `python`
      let mut cmd2 = Command::new("python");
      cmd2.args(args);
      cmd2.current_dir(backend_root);
      cmd2.env("PYTHONPATH", pythonpath);
      cmd2.env("AICOMIC_DB_PATH", db_path.to_string_lossy().to_string());
      cmd2.env("AICOMIC_DATA_DIR", data_dir.to_string_lossy().to_string());
      cmd2.stdout(Stdio::null());
      cmd2.stderr(Stdio::null());
      cmd2.spawn()
    }
  }
}

fn inject_base_url(window: &Window, port: u16) {
  let js = format!(
    r#"
      (function() {{
        const url = "http://127.0.0.1:{port}";
        window.__AICOMIC_API_BASE_URL__ = url;
        window.dispatchEvent(new CustomEvent("aicomic-api-base-url", {{ detail: url }}));
      }})();
    "#
  );
  let _ = window.eval(&js);
}

fn main() {
  tauri::Builder::default()
    .setup(|app| {
      let app_data_dir = app
        .path_resolver()
        .app_data_dir()
        .expect("app_data_dir not available");
      std::fs::create_dir_all(&app_data_dir).expect("create app_data_dir failed");

      let resource_dir = app.path_resolver().resource_dir();
      let backend_root = find_backend_root(resource_dir).expect("backend root not found");

      let port = pick_free_port();
      let child = spawn_backend(&app_data_dir, port, &backend_root).expect("spawn backend failed");

      // stash child for shutdown
      app.manage(BackendChild(std::sync::Mutex::new(Some(child))));

      // wait backend ready then inject url
      if wait_port_open(port, Duration::from_secs(8)) {
        if let Some(win) = app.get_window("main") {
          inject_base_url(&win, port);
        }
      }

      Ok(())
    })
    .on_window_event(|event| {
      if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
        // allow close, but kill backend first
        let app = event.window().app_handle();
        if let Some(state) = app.try_state::<BackendChild>() {
          if let Ok(mut guard) = state.0.lock() {
            if let Some(mut child) = guard.take() {
              let _ = child.kill();
            }
          }
        }
        api.close_window();
      }
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

struct BackendChild(std::sync::Mutex<Option<Child>>);



// macOS 不需要 windows_subsystem；保留空属性避免编译报错

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{Manager, Window};

fn python_can_import(python: &str, backend_root: &Path, pythonpath: &str) -> bool {
  let mut cmd = Command::new(python);
  cmd.current_dir(backend_root);
  cmd.env("PYTHONPATH", pythonpath);
  cmd.arg("-c").arg("import sys; import uvicorn; import httpx; print(sys.executable)");
  match cmd.output() {
    Ok(out) => {
      if out.status.success() {
        let exe = String::from_utf8_lossy(&out.stdout).trim().to_string();
        eprintln!("[Tauri] Python OK: {} -> {}", python, exe);
        true
      } else {
        let err = String::from_utf8_lossy(&out.stderr);
        eprintln!("[Tauri] Python not usable: {} (need uvicorn+httpx). stderr: {}", python, err.trim());
        false
      }
    }
    Err(e) => {
      eprintln!("[Tauri] Python probe failed for {}: {:?}", python, e);
      false
    }
  }
}

fn pick_python_executable(backend_root: &Path, pythonpath: &str) -> String {
  // 1) 用户显式指定（推荐：绝对路径）
  if let Ok(p) = std::env::var("AICOMIC_PYTHON") {
    let p = p.trim().to_string();
    if !p.is_empty() && python_can_import(&p, backend_root, pythonpath) {
      eprintln!("[Tauri] Using python from AICOMIC_PYTHON: {}", p);
      return p;
    }
  }

  // 2) 常见候选：优先 python3.12
  let candidates = vec![
    "python3.12",
    "python3",
    "python",
    // Homebrew 常见位置（避免 PATH 不完整）
    "/opt/homebrew/bin/python3.12",
    "/usr/local/bin/python3.12",
    "/opt/homebrew/bin/python3",
    "/usr/local/bin/python3",
  ];

  for c in candidates {
    if python_can_import(c, backend_root, pythonpath) {
      eprintln!("[Tauri] Using python: {}", c);
      return c.to_string();
    }
  }

  // 3) 兜底：仍然返回 python3，让后续 spawn 输出更明确的 stderr
  eprintln!("[Tauri] WARNING: No suitable python found with uvicorn+httpx. Falling back to python3.");
  "python3".to_string()
}

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

fn spawn_backend(app_data_dir: &Path, port: u16, backend_root: &Path, python_exe: &str) -> std::io::Result<Child> {
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

  eprintln!("[Tauri] Attempting to spawn backend with {}...", python_exe);
  eprintln!("[Tauri] Command: {} -m uvicorn backend.app.main:app --host 127.0.0.1 --port {}", python_exe, port);
  eprintln!("[Tauri] Working directory: {:?}", backend_root);
  eprintln!("[Tauri] PYTHONPATH: {}", pythonpath);
  
  let mut cmd = Command::new(python_exe);
  cmd.args(args.clone());
  cmd.current_dir(backend_root);
  cmd.env("PYTHONPATH", pythonpath.clone());
  cmd.env("AICOMIC_DB_PATH", db_path.to_string_lossy().to_string());
  cmd.env("AICOMIC_DATA_DIR", data_dir.to_string_lossy().to_string());
  cmd.stdout(Stdio::piped()); // 保留 stdout 以便调试
  cmd.stderr(Stdio::piped()); // 保留 stderr 以便调试

  match cmd.spawn() {
    Ok(child) => {
      eprintln!("[Tauri] Backend process spawned successfully with {}", python_exe);
      Ok(child)
    }
    Err(e) => {
      eprintln!("[Tauri] Failed to spawn backend with {}: {:?}", python_exe, e);
      Err(e)
    }
  }
}

fn inject_base_url(window: &Window, port: u16) {
  let js = format!(
    r#"
      (function() {{
        const url = "http://127.0.0.1:{port}";
        window.__AICOMIC_API_BASE_URL__ = url;
        console.log("[Tauri] Injected API baseURL:", url);
        window.dispatchEvent(new CustomEvent("aicomic-api-base-url", {{ detail: url }}));
        // 确保立即触发一次，以防事件监听器已经注册
        if (window.setApiBaseUrl) {{
          window.setApiBaseUrl(url);
        }}
      }})();
    "#
  );
  if let Err(e) = window.eval(&js) {
    eprintln!("[Tauri] Failed to inject baseURL: {:?}", e);
  }
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
      eprintln!("[Tauri] Picked port: {}", port);
      eprintln!("[Tauri] Backend root: {:?}", backend_root);
      eprintln!("[Tauri] App data dir: {:?}", app_data_dir);
      let pythonpath = backend_root.to_string_lossy().to_string();
      let python_exe = pick_python_executable(&backend_root, &pythonpath);
      
      let mut child = match spawn_backend(&app_data_dir, port, &backend_root, &python_exe) {
        Ok(c) => {
          eprintln!("[Tauri] Backend spawned successfully");
          c
        }
        Err(e) => {
          eprintln!("[Tauri] Failed to spawn backend: {:?}", e);
          return Err(e.into());
        }
      };

      // 读取 stderr 以便调试
      if let Some(stderr) = child.stderr.take() {
        let _app_handle = app.handle().clone();
        thread::spawn(move || {
          use std::io::{BufRead, BufReader};
          let reader = BufReader::new(stderr);
          for line in reader.lines() {
            if let Ok(l) = line {
              eprintln!("[Backend stderr] {}", l);
            }
          }
        });
      }

      // stash child for shutdown
      app.manage(BackendChild(std::sync::Mutex::new(Some(child))));

      // wait backend ready then inject url
      let app_handle = app.handle().clone();
      let port_for_inject = port;
      thread::spawn(move || {
        eprintln!("[Tauri] Waiting for backend on port {}...", port_for_inject);
        // 等待后端启动
        if wait_port_open(port_for_inject, Duration::from_secs(10)) {
          eprintln!("[Tauri] Backend ready on port {}", port_for_inject);
          // 尝试多次注入，因为窗口可能还没准备好
          for i in 0..20 {
            if let Some(win) = app_handle.get_window("main") {
              inject_base_url(&win, port_for_inject);
              eprintln!("[Tauri] Injected baseURL on attempt {}", i + 1);
              // 再等待一下，确保注入完成
              thread::sleep(Duration::from_millis(100));
              break;
            }
            eprintln!("[Tauri] Window not ready yet, attempt {}", i + 1);
            thread::sleep(Duration::from_millis(200));
          }
        } else {
          eprintln!("[Tauri] ERROR: Backend failed to start within timeout on port {}", port_for_inject);
          eprintln!("[Tauri] Please check if Python and uvicorn are installed correctly");
        }
      });

      Ok(())
    })
    .on_window_event(|event| {
      if let tauri::WindowEvent::CloseRequested { .. } = event.event() {
        // 不拦截关闭：在关闭事件触发时先 kill 后端子进程，然后让窗口自然关闭即可
        let app = event.window().app_handle();
        if let Some(state) = app.try_state::<BackendChild>() {
          if let Ok(mut guard) = state.0.lock() {
            if let Some(mut child) = guard.take() {
              let _ = child.kill();
            }
          }
        }
      }
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

struct BackendChild(std::sync::Mutex<Option<Child>>);



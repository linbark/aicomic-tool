//! Offline, self-contained HTML capture. Hashes bind bytes, not authenticity.
//! `capture` is the controlled Chrome entry; injected `capture_with` renderers
//! are never trusted merely because their JSON claims to be Chrome.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const POLICY_VERSION: &str = "ie-static-capture-v1";

const EFFECTIVE_PREFIX: &str = concat!(
    "<!doctype html><meta charset=\"utf-8\">",
    "<meta http-equiv=\"Content-Security-Policy\" content=\"",
    "default-src 'none'; script-src 'none'; connect-src 'none'; ",
    "img-src data:; style-src 'unsafe-inline'; font-src 'none'; ",
    "object-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; ",
    "media-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'\">",
    "<meta name=\"color-scheme\" content=\"only light\">",
    "<style>:root{color-scheme:only light!important}",
    "*,*::before,*::after{animation:none!important;transition:none!important;caret-color:transparent!important}</style>"
);

#[derive(Debug, Clone, Copy)]
pub struct CaptureSpec {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct CapturedPage {
    pub page_sha256: String,
    pub screenshot_sha256: String,
    pub receipt: Value,
}

pub trait Renderer {
    fn render(&self, html: &Path, screenshot: &Path, spec: &CaptureSpec) -> Result<Value>;
}

#[derive(Debug, Clone)]
pub struct ChromeRenderer {
    executable: PathBuf,
}

impl ChromeRenderer {
    pub fn discover(explicit: Option<&Path>) -> Result<Self> {
        if let Some(path) = explicit {
            return Self::at_path(path)
                .context("invalid explicitly selected Chrome executable (no fallback)");
        }
        for path in chrome_candidates() {
            if let Ok(renderer) = Self::at_path(&path) {
                return Ok(renderer);
            }
        }
        bail!(
            "no existing Chrome/Chromium binary found; supply an explicit path (nothing is installed)"
        )
    }

    fn at_path(path: &Path) -> Result<Self> {
        let executable = fs::canonicalize(path).context("resolve Chrome executable")?;
        let metadata = fs::metadata(&executable)?;
        anyhow::ensure!(metadata.is_file(), "Chrome path is not a regular file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            anyhow::ensure!(
                metadata.permissions().mode() & 0o111 != 0,
                "Chrome file is not executable"
            );
        }
        Ok(Self { executable })
    }

    fn command(&self, work: &OwnedDir) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.executable);
        cmd.args(CHROME_FLAGS)
            .arg(format!(
                "--user-data-dir={}",
                work.path.join("profile").display()
            ))
            .arg(format!(
                "--disk-cache-dir={}",
                work.path.join("cache").display()
            ))
            .env("XDG_CONFIG_HOME", work.path.join("config"))
            .env("XDG_CACHE_HOME", work.path.join("cache"))
            .env("BREAKPAD_DUMP_LOCATION", work.path.join("crashes"))
            .stdin(std::process::Stdio::null());
        cmd
    }
}

const CHROME_FLAGS: &[&str] = &[
    "--headless=new",
    "--disable-gpu",
    "--force-device-scale-factor=1",
    "--force-color-profile=srgb",
    // Do not set scriptEnabled=false: Chrome's own headless screenshot
    // controller needs JavaScript. Untrusted page scripts remain forbidden
    // by source validation and the injected script-src 'none' CSP.
    "--blink-settings=preferredColorScheme=1",
    "--hide-scrollbars",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-domain-reliability",
    "--disable-sync",
    "--disable-extensions",
    "--disable-default-apps",
    "--disable-breakpad",
    "--disable-crash-reporter",
    "--no-pings",
    "--metrics-recording-only",
    "--safebrowsing-disable-auto-update",
    "--disable-client-side-phishing-detection",
    "--disable-features=Translate,MediaRouter,OptimizationHints,AutofillServerCommunication,CertificateTransparencyComponentUpdater",
    "--host-resolver-rules=MAP * ~NOTFOUND",
    "--proxy-server=http://127.0.0.1:9",
    "--proxy-bypass-list=<-loopback>",
    "--password-store=basic",
    "--use-mock-keychain",
    "--run-all-compositor-stages-before-draw",
    "--virtual-time-budget=1000",
    "--timeout=15000",
    "--default-background-color=ffffffff",
    "--force-prefers-reduced-motion",
    "--lang=en-US",
    "--enable-automation",
    "--disable-search-engine-choice-screen",
    "--disable-notifications",
];

fn capture_parameters(spec: &CaptureSpec) -> Value {
    json!({
        "chrome_flags": CHROME_FLAGS,
        "window_size": [spec.width, spec.height], "dpr": 1,
        "theme": "light", "color_profile": "srgb", "locale": "en-US",
        "virtual_time_budget_ms": 1000, "process_timeout_seconds": 30,
        "source": "file-url-with-injected-csp", "user_profile": "fresh-owned-temporary-directory",
        "network": "csp-denied-resolver-blackhole-and-loopback-discard-proxy",
        "javascript": "source-validation-and-csp-script-src-none", "fonts": "local-system-fonts-platform-dependent",
    })
}

fn chrome_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/opt/google/chrome/chrome",
        "/opt/chromium/chrome",
        "/usr/local/bin/chromium",
        "/opt/homebrew/bin/chromium",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        for app in ["Google Chrome", "Chromium", "Google Chrome for Testing"] {
            candidates.push(
                PathBuf::from(&home).join(format!("Applications/{app}.app/Contents/MacOS/{app}")),
            );
        }
    }
    for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(&root).join("Google/Chrome/Application/chrome.exe"));
            candidates.push(PathBuf::from(&root).join("Chromium/Application/chrome.exe"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in [
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "chrome",
                "chrome.exe",
                "headless_shell",
                "chrome-headless-shell",
            ] {
                candidates.push(dir.join(name));
            }
        }
    }
    candidates
}

impl Renderer for ChromeRenderer {
    fn render(&self, html: &Path, screenshot: &Path, spec: &CaptureSpec) -> Result<Value> {
        validate_spec(spec)?;
        let output = new_output_path(screenshot)?;
        let bytes = read_html(html)?;
        let source = std::str::from_utf8(&bytes).context("HTML must be UTF-8")?;
        // Capture's exact prefix is idempotent, but its remainder is always
        // validated. Direct Renderer calls cannot bypass validation or CSP.
        let original = source.strip_prefix(EFFECTIVE_PREFIX).unwrap_or(source);
        validate_self_contained(original)?;
        let effective_bytes = format!("{EFFECTIVE_PREFIX}{original}").into_bytes();
        let work = OwnedDir::new()?;
        let page = work.path.join("page.html");
        publish_new(&page, &effective_bytes)?;
        let executable_sha256 = sha256_file(&self.executable)?;
        let mut version_cmd = self.command(&work);
        version_cmd.arg("--version");
        let version = run_chrome(&mut version_cmd, &work, "version", 10)?
            .trim()
            .to_owned();
        anyhow::ensure!(
            version.len() <= 512
                && [
                    "Google Chrome",
                    "Chromium",
                    "HeadlessChrome",
                    "Chrome Headless Shell"
                ]
                .iter()
                .any(|name| version.starts_with(name))
                && version.chars().any(|c| c.is_ascii_digit()),
            "selected executable did not identify as Chrome/Chromium: {version}"
        );
        let rendered = work.path.join("chrome.png");
        let mut cmd = self.command(&work);
        cmd.arg(format!("--window-size={},{}", spec.width, spec.height))
            .arg(format!("--screenshot={}", rendered.display()))
            .arg(file_url(&page)?);
        run_chrome(&mut cmd, &work, "screenshot", 30)?;
        anyhow::ensure!(
            sha256_file(&self.executable)? == executable_sha256,
            "Chrome executable changed during capture"
        );
        anyhow::ensure!(
            sha256_file(html)? == sha256_bytes(&bytes),
            "renderer input changed during capture"
        );
        anyhow::ensure!(
            sha256_file(&page)? == sha256_bytes(&effective_bytes),
            "Chrome effective page changed during capture"
        );
        let png = read_chrome_png(&rendered, spec, &work.path.join("screenshot.stderr"))?;
        publish_new(&output, &png)?;
        Ok(json!({
            "kind": "chrome", "version": version,
            "executable_sha256": executable_sha256,
            "executable_path": self.executable,
            "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            "effective_page_sha256": sha256_bytes(&effective_bytes),
            "capture_parameters": capture_parameters(spec),
        }))
    }
}

fn read_chrome_png(path: &Path, spec: &CaptureSpec, stderr: &Path) -> Result<Vec<u8>> {
    read_png(path, spec).with_context(|| {
        let tail = read_log_tail(stderr).unwrap_or_else(|error| format!("<unavailable: {error}>"));
        format!("Chrome exited successfully but screenshot PNG was missing or invalid; stderr tail: {tail}")
    })
}

fn read_log_tail(path: &Path) -> Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::End(-(length.min(8192) as i64)))?;
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn run_chrome(
    cmd: &mut std::process::Command,
    work: &OwnedDir,
    label: &str,
    seconds: u64,
) -> Result<String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let stdout = work.path.join(format!("{label}.stdout"));
    let stderr = work.path.join(format!("{label}.stderr"));
    let create = |path: &Path| {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    };
    cmd.stdout(Stdio::from(create(&stdout)?))
        .stderr(Stdio::from(create(&stderr)?));
    let mut child = cmd
        .spawn()
        .context("start selected local Chrome executable")?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < Duration::from_secs(seconds) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                if let Err(error) = result {
                    return Err(error).context("wait for Chrome");
                }
                bail!("Chrome {label} timed out after {seconds}s");
            }
        }
    };
    anyhow::ensure!(
        status.success(),
        "Chrome {label} failed ({status}); stderr tail: {}",
        read_log_tail(&stderr)?
    );
    read_log_tail(&stdout)
}

fn file_url(path: &Path) -> Result<String> {
    let absolute = fs::canonicalize(path)?;
    let raw = absolute
        .to_str()
        .context("Chrome requires a UTF-8 file path")?;
    let raw = raw.strip_prefix("\\\\?\\").unwrap_or(raw);
    anyhow::ensure!(
        !raw.starts_with("UNC\\") && !raw.starts_with("\\\\") && !raw.starts_with("//"),
        "network-share file URLs are not supported"
    );
    let normalized = if cfg!(windows) {
        raw.replace('\\', "/")
    } else {
        raw.to_owned()
    };
    let mut encoded = String::new();
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(format!(
        "{}{}",
        if normalized.starts_with('/') {
            "file://"
        } else {
            "file:///"
        },
        encoded
    ))
}

/// Controlled engine entry. `verified` records this execution path, not a
/// cryptographic signature, installation attestation or OS network sandbox.
pub fn capture(
    html: &Path,
    screenshot: &Path,
    spec: &CaptureSpec,
    chrome: Option<&Path>,
) -> Result<CapturedPage> {
    validate_spec(spec)?;
    let original = read_html(html)?;
    validate_self_contained(std::str::from_utf8(&original).context("HTML must be UTF-8")?)?;
    capture_impl(
        html,
        screenshot,
        spec,
        &ChromeRenderer::discover(chrome)?,
        true,
    )
}

pub fn capture_with(
    html: &Path,
    screenshot: &Path,
    spec: &CaptureSpec,
    renderer: &dyn Renderer,
) -> Result<CapturedPage> {
    capture_impl(html, screenshot, spec, renderer, false)
}

fn capture_impl(
    html: &Path,
    screenshot: &Path,
    spec: &CaptureSpec,
    renderer: &dyn Renderer,
    controlled_chrome: bool,
) -> Result<CapturedPage> {
    validate_spec(spec)?;
    let screenshot = new_output_path(screenshot)?;
    let bytes = read_html(html)?;
    let source = std::str::from_utf8(&bytes).context("HTML must be UTF-8")?;
    validate_self_contained(source)?;
    let work = OwnedDir::new()?;
    let effective = work.path.join("effective.html");
    let effective_bytes = format!("{EFFECTIVE_PREFIX}{source}").into_bytes();
    publish_new(&effective, &effective_bytes)?;
    let effective_page_sha256 = sha256_bytes(&effective_bytes);
    let rendered = work.path.join("render.png");
    let metadata = renderer.render(&effective, &rendered, spec)?;
    let screenshot_bytes = read_png(&rendered, spec)?;
    let page_sha256 = sha256_bytes(&bytes);
    anyhow::ensure!(
        sha256_file(html)? == page_sha256,
        "original HTML changed during capture"
    );
    anyhow::ensure!(
        sha256_file(&effective)? == effective_page_sha256,
        "effective HTML changed during capture"
    );
    let screenshot_sha256 = sha256_bytes(&screenshot_bytes);
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let identity = json!({
        "kind": if controlled_chrome { "chrome" } else if metadata.get("kind").and_then(Value::as_str) == Some("mock") { "mock" } else { "custom" },
        "version": metadata.get("version").and_then(Value::as_str).unwrap_or("unknown"),
        "executable_sha256": metadata.get("executable_sha256").and_then(Value::as_str),
        "platform": platform,
    });
    let viewport = json!({"width":spec.width,"height":spec.height});
    let profile = json!({
        "policy_version": POLICY_VERSION, "renderer": identity,
        "viewport": viewport, "theme": "light", "dpr": 1,
        "effective_prefix_sha256": sha256_bytes(EFFECTIVE_PREFIX.as_bytes()),
        "capture_parameters": capture_parameters(spec),
    });
    let profile_sha256 = sha256_bytes(&serde_json::to_vec(&profile)?);
    let mut renderer_identity = identity;
    renderer_identity["metadata"] = metadata;
    let receipt = json!({
        "policy_version": POLICY_VERSION, "page_sha256": page_sha256,
        "effective_page_sha256": effective_page_sha256,
        "screenshot_sha256": screenshot_sha256,
        "profile_sha256": profile_sha256, "profile": profile,
        "viewport": viewport, "theme": "light", "dpr": 1,
        "renderer": renderer_identity, "verified": controlled_chrome,
        "trust": if controlled_chrome { "controlled-engine-path-not-a-signature" } else { "injected-renderer-not-authenticated" },
    });
    publish_new(&screenshot, &screenshot_bytes)?;
    anyhow::ensure!(
        sha256_file(&screenshot)? == screenshot_sha256,
        "published screenshot changed"
    );
    Ok(CapturedPage {
        receipt,
        page_sha256,
        screenshot_sha256,
    })
}

fn read_html(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let file = fs::File::open(path).context("read input HTML")?;
    anyhow::ensure!(file.metadata()?.is_file(), "HTML must be a regular file");
    let mut bytes = Vec::new();
    let limit = 16 * 1024 * 1024 + EFFECTIVE_PREFIX.len();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= limit,
        "HTML exceeds 16 MiB plus capture policy"
    );
    Ok(bytes)
}

fn new_output_path(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().context("screenshot needs a filename")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let output = fs::canonicalize(parent)
        .context("resolve screenshot parent")?
        .join(name);
    match fs::symlink_metadata(&output) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(output),
        Err(e) => Err(e).context("check screenshot destination"),
        Ok(_) => bail!("refusing to overwrite an existing screenshot destination"),
    }
}

fn publish_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context("create new screenshot, never overwrite")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn validate_spec(spec: &CaptureSpec) -> Result<()> {
    anyhow::ensure!(
        spec.width > 0
            && spec.height > 0
            && spec.width <= 16384
            && spec.height <= 16384
            && u64::from(spec.width) * u64::from(spec.height) <= 64 * 1024 * 1024,
        "invalid or excessive capture dimensions"
    );
    Ok(())
}

fn read_png(path: &Path, spec: &CaptureSpec) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).context("renderer did not produce a screenshot")?;
    anyhow::ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "screenshot must be a regular non-symlink file"
    );
    anyhow::ensure!(
        meta.len() <= 256 * 1024 * 1024,
        "screenshot exceeds 256 MiB"
    );
    let bytes = fs::read(path)?;
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(spec.width);
    limits.max_image_height = Some(spec.height);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .context("screenshot must fully decode as PNG within requested dimensions")?;
    anyhow::ensure!(
        decoded.width() == spec.width && decoded.height() == spec.height,
        "PNG dimensions do not match requested viewport"
    );
    Ok(bytes)
}

pub fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "hash input must be a regular file"
    );
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Only directories created by this instance, with a matching ownership token,
/// canonical absolute path and unchanged canonical parent, may be removed.
struct OwnedDir {
    path: PathBuf,
    parent: PathBuf,
    token: String,
}

impl OwnedDir {
    fn new() -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = fs::canonicalize(std::env::temp_dir()).context("resolve temporary root")?;
        for _ in 0..32 {
            let token = format!(
                "{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let path = parent.join(format!("ie-capture-{token}"));
            match fs::create_dir(&path) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                    }
                    fs::write(path.join(".ie-owned"), token.as_bytes())?;
                    return Ok(Self {
                        path,
                        parent,
                        token,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).context("create owned temporary directory"),
            }
        }
        bail!("could not allocate an owned temporary directory")
    }
}

impl Drop for OwnedDir {
    fn drop(&mut self) {
        let owned = self.path.is_absolute()
            && self.path.parent() == Some(self.parent.as_path())
            && fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            && fs::canonicalize(&self.path).is_ok_and(|p| p == self.path)
            && fs::read(self.path.join(".ie-owned")).is_ok_and(|b| b == self.token.as_bytes());
        if owned {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub fn validate_self_contained(html: &str) -> Result<()> {
    parse_static_html(html).map(|_| ())
}

/// Source-text projection, not computed accessibility/visibility or a DOM proof.
/// Whitespace is collapsed; head/style content is excluded. Unsupported HTML
/// is rejected, just as it is by capture.
pub fn text_projection(html: &str) -> Result<String> {
    parse_static_html(html)
}

fn parse_static_html(html: &str) -> Result<String> {
    anyhow::ensure!(html.len() <= 16 * 1024 * 1024, "HTML exceeds 16 MiB");
    anyhow::ensure!(
        !html
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
        "HTML contains control characters"
    );
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;
    let mut in_head = false;
    let mut text = String::new();
    while pos < html.len() {
        if html[pos..].starts_with("<!--") {
            let end = html[pos + 4..]
                .find("-->")
                .context("unterminated HTML comment")?
                + pos
                + 4;
            anyhow::ensure!(!html[pos + 4..end].contains("--"), "ambiguous HTML comment");
            pos = end + 3;
            continue;
        }
        if !html[pos..].starts_with('<') {
            let end = html[pos..].find('<').map_or(html.len(), |i| pos + i);
            if !in_head {
                text.push(' ');
                text.push_str(&decode_text(&html[pos..end])?);
            }
            pos = end;
            continue;
        }
        if lower[pos..].starts_with("<!doctype html>") {
            pos += "<!doctype html>".len();
            continue;
        }
        let start = pos + 1;
        let mut end = start;
        let mut quote = None;
        for (i, c) in html[start..].char_indices() {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if c == '\'' || c == '"' {
                quote = Some(c);
            } else if c == '>' {
                end = start + i;
                break;
            } else if c == '<' {
                bail!("nested tag delimiter");
            }
        }
        anyhow::ensure!(end > start && quote.is_none(), "malformed HTML tag");
        let raw = &html[start..end];
        let closing = raw.starts_with('/');
        let raw = if closing { &raw[1..] } else { raw };
        let name_end = raw
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(raw.len());
        let name = raw[..name_end].to_ascii_lowercase();
        anyhow::ensure!(
            matches!(
                name.as_str(),
                "html"
                    | "head"
                    | "body"
                    | "title"
                    | "meta"
                    | "style"
                    | "div"
                    | "span"
                    | "p"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "main"
                    | "section"
                    | "article"
                    | "header"
                    | "footer"
                    | "nav"
                    | "aside"
                    | "a"
                    | "ul"
                    | "ol"
                    | "li"
                    | "dl"
                    | "dt"
                    | "dd"
                    | "figure"
                    | "figcaption"
                    | "table"
                    | "thead"
                    | "tbody"
                    | "tfoot"
                    | "tr"
                    | "td"
                    | "th"
                    | "caption"
                    | "col"
                    | "colgroup"
                    | "br"
                    | "hr"
                    | "pre"
                    | "code"
                    | "em"
                    | "strong"
                    | "small"
                    | "b"
                    | "i"
                    | "u"
                    | "s"
                    | "sub"
                    | "sup"
                    | "blockquote"
                    | "img"
                    | "button"
                    | "label"
                    | "progress"
                    | "meter"
                    | "details"
                    | "summary"
                    | "time"
                    | "mark"
                    | "abbr"
                    | "address"
                    | "wbr"
            ),
            "unsupported or active HTML element: {name}"
        );
        let rest = &raw[name_end..];
        if closing {
            anyhow::ensure!(rest.trim().is_empty(), "malformed closing tag");
        } else {
            validate_attributes(&name, rest)?;
        }
        pos = end + 1;
        if name == "head" {
            in_head = !closing;
        }
        if name == "style" && !closing {
            let close = lower[pos..]
                .find("</style>")
                .context("style requires an unambiguous closing tag")?
                + pos;
            validate_css(&html[pos..close])?;
            pos = close + "</style>".len();
        }
    }
    Ok(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn validate_attributes(tag: &str, mut rest: &str) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    while !rest.trim().is_empty() && rest.trim() != "/" {
        anyhow::ensure!(
            rest.starts_with(char::is_whitespace),
            "attribute needs a separator"
        );
        rest = rest.trim_start();
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(rest.len());
        anyhow::ensure!(end > 0, "invalid attribute name");
        let key = rest[..end].to_ascii_lowercase();
        anyhow::ensure!(seen.insert(key.clone()), "duplicate attribute: {key}");
        rest = &rest[end..];
        let after_space = rest.trim_start();
        let mut value = "";
        if let Some(r) = after_space.strip_prefix('=') {
            let r = r.trim_start();
            if let Some(q) = r.chars().next().filter(|c| *c == '\'' || *c == '"') {
                let end = r[1..].find(q).context("unterminated attribute")? + 1;
                value = &r[1..end];
                rest = &r[end + 1..];
            } else {
                let end = r.find(char::is_whitespace).unwrap_or(r.len());
                value = &r[..end];
                anyhow::ensure!(
                    !value.is_empty() && !value.contains(['<', '>', '`', '=', '\'', '"']),
                    "invalid unquoted attribute"
                );
                rest = &r[end..];
            }
        }
        let global = matches!(
            key.as_str(),
            "id" | "class" | "title" | "lang" | "dir" | "role"
        ) || key.starts_with("aria-")
            || key.starts_with("data-");
        if global {
            continue;
        }
        match (tag, key.as_str()) {
            (_, "style") => validate_css(value)?,
            ("a", "href") => {
                anyhow::ensure!(safe_fragment(value), "only fragment links are allowed")
            }
            ("img", "src") => validate_data_image(value)?,
            ("img", "alt" | "width" | "height" | "loading" | "decoding") => {}
            ("meta", "charset") => anyhow::ensure!(
                value.eq_ignore_ascii_case("utf-8"),
                "only UTF-8 is supported"
            ),
            ("meta", "name") => anyhow::ensure!(
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "viewport" | "description" | "author"
                ),
                "unsupported meta name"
            ),
            ("meta", "content")
            | ("style", "type")
            | ("button", "disabled" | "type")
            | ("td" | "th", "colspan" | "rowspan" | "scope")
            | ("col" | "colgroup", "span")
            | ("ol", "start" | "reversed" | "type")
            | ("li", "value")
            | ("progress" | "meter", "value" | "min" | "max" | "low" | "high" | "optimum")
            | ("time", "datetime")
            | ("details", "open") => {}
            _ => bail!("unsupported or active attribute: {tag}.{key}"),
        }
    }
    Ok(())
}

fn safe_fragment(value: &str) -> bool {
    value.starts_with('#')
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '&' | '\\'))
}

fn validate_data_image(value: &str) -> Result<()> {
    use base64::Engine;
    let (header, data) = value
        .split_once(',')
        .context("image must be an inline base64 data image")?;
    anyhow::ensure!(
        matches!(
            header.to_ascii_lowercase().as_str(),
            "data:image/png;base64"
                | "data:image/jpeg;base64"
                | "data:image/webp;base64"
                | "data:image/gif;base64"
        ),
        "only inline raster data images are supported"
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .context("invalid base64 image")?;
    let format = image::guess_format(&bytes).context("invalid inline raster image")?;
    anyhow::ensure!(
        matches!(
            format,
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::WebP
                | image::ImageFormat::Gif
        ),
        "invalid raster image format"
    );
    Ok(())
}

fn validate_css(css: &str) -> Result<()> {
    anyhow::ensure!(
        !css.contains(['\\', '&', '<']),
        "CSS escapes/entities/markup are not supported"
    );
    let mut clean = String::new();
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        clean.push_str(&rest[..start]);
        let end = rest[start + 2..]
            .find("*/")
            .context("unterminated CSS comment")?
            + start
            + 2;
        rest = &rest[end + 2..];
    }
    clean.push_str(rest);
    let lower = clean.to_ascii_lowercase();
    anyhow::ensure!(
        !lower.contains("image-set")
            && !lower.contains("expression")
            && !lower.contains("-moz-binding")
            && !lower.contains("src("),
        "unsupported CSS dependency/function"
    );
    for at in lower.match_indices('@').map(|(i, _)| i + 1) {
        let name: String = lower[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphabetic() || *c == '-')
            .collect();
        anyhow::ensure!(
            matches!(name.as_str(), "media" | "supports" | "layer"),
            "unsupported CSS at-rule: {name}"
        );
    }
    let mut pos = 0;
    while let Some(i) = lower[pos..].find("url") {
        pos += i + 3;
        let rest = clean[pos..].trim_start();
        pos = clean.len() - rest.len();
        if let Some(r) = rest.strip_prefix('(') {
            let end = r.find(')').context("unterminated CSS url")?;
            let value = r[..end].trim().trim_matches(['\'', '"']);
            if !safe_fragment(value) {
                validate_data_image(value)?;
            }
            pos += end + 2;
        }
    }
    Ok(())
}

fn decode_text(text: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find(';') else {
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let entity = &tail[..end];
        let ch = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => ' ',
            "ndash" => '–',
            "mdash" => '—',
            "hellip" => '…',
            "copy" => '©',
            "reg" => '®',
            "bull" => '•',
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                char::from_u32(u32::from_str_radix(&entity[2..], 16)?)
                    .context("invalid character entity")?
            }
            _ if entity.starts_with('#') => {
                char::from_u32(entity[1..].parse()?).context("invalid character entity")?
            }
            _ => bail!("unsupported character entity: {entity}"),
        };
        out.push(ch);
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct MockRenderer {
        bytes: Option<Vec<u8>>,
        metadata: Value,
        mutate: Option<PathBuf>,
    }

    impl Renderer for MockRenderer {
        fn render(&self, html: &Path, screenshot: &Path, _spec: &CaptureSpec) -> Result<Value> {
            if let Some(path) = &self.mutate {
                fs::write(path, "<p>changed</p>")?;
            }
            if let Some(bytes) = &self.bytes {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(screenshot)?
                    .write_all(bytes)?;
            }
            let mut metadata = self.metadata.clone();
            metadata["observed_html_sha256"] = json!(sha256_file(html)?);
            metadata["policy_seen"] =
                json!(fs::read_to_string(html)?.contains("script-src 'none'"));
            Ok(metadata)
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(width, height)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn fixture() -> (OwnedDir, PathBuf, PathBuf, CaptureSpec) {
        let dir = OwnedDir::new().unwrap();
        let html = dir.path.join("input.html");
        let screenshot = dir.path.join("output.png");
        fs::write(&html, "<!doctype html><p>Hello</p>").unwrap();
        (
            dir,
            html,
            screenshot,
            CaptureSpec {
                width: 16,
                height: 12,
            },
        )
    }

    #[test]
    fn missing_chrome_png_reports_bounded_stderr_tail_even_after_success() {
        let (dir, _html, screenshot, spec) = fixture();
        let stderr = dir.path.join("screenshot.stderr");
        let mut log = b"SHOULD-NOT-REPORT-OLD-HEAD\n".to_vec();
        log.extend(vec![b'x'; 16 * 1024]);
        log.extend_from_slice(b"\nFINAL-CHROME-DIAGNOSTIC\n");
        publish_new(&stderr, &log).unwrap();
        // This is the exact PNG-validation boundary after a zero-exit Chrome;
        // no process is launched and no renderer receipt is trusted.
        let error = read_chrome_png(&screenshot, &spec, &stderr).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("Chrome exited successfully"), "{message}");
        assert!(message.contains("FINAL-CHROME-DIAGNOSTIC"), "{message}");
        assert!(!message.contains("SHOULD-NOT-REPORT-OLD-HEAD"));
        assert!(message.len() < 9000);
        assert!(!screenshot.exists());
    }

    #[test]
    fn chrome_command_keeps_internal_capture_scripts_enabled_but_content_csp_strict() {
        let work = OwnedDir::new().unwrap();
        let renderer = ChromeRenderer::discover(Some(&std::env::current_exe().unwrap())).unwrap();
        // Exercise the builder shared by both actual Chrome invocations; no
        // subprocess runs here. Chrome 157 silently skips --screenshot when
        // Blink globally disables scripts, including its internal controller.
        let cmd = renderer.command(&work);
        let args: Vec<_> = cmd.get_args().map(|arg| arg.to_string_lossy()).collect();
        assert!(
            !args.iter().any(|arg| arg.contains("scriptEnabled=false")),
            "global script disabling breaks headless screenshot: {args:?}"
        );
        assert!(
            args.iter()
                .any(|arg| arg == "--blink-settings=preferredColorScheme=1")
        );
        assert!(args.iter().any(|arg| arg == "--headless=new"));
        assert!(
            args.iter()
                .any(|arg| arg == "--disable-background-networking")
        );
        assert!(
            args.iter()
                .any(|arg| arg == "--host-resolver-rules=MAP * ~NOTFOUND")
        );
        assert!(
            args.iter()
                .any(|arg| arg == "--proxy-server=http://127.0.0.1:9")
        );
        assert!(EFFECTIVE_PREFIX.contains("script-src 'none'"));
        assert!(EFFECTIVE_PREFIX.contains("connect-src 'none'"));
        assert!(validate_self_contained("<script>alert(1)</script>").is_err());
        assert!(validate_self_contained("<p onclick='alert(1)'>x</p>").is_err());
    }

    #[test]
    fn accepts_static_css_child_combinators_and_media_ranges() {
        validate_self_contained("<style>.grid > div{color:red}@media (width > 600px){p{padding:2px}}</style><div class='grid'><div>x</div></div>").unwrap();
    }

    #[test]
    fn never_overwrites_existing_or_symlinked_outputs() {
        let (dir, html, screenshot, spec) = fixture();
        let original = fs::read(&html).unwrap();
        fs::write(&screenshot, b"unknown existing file").unwrap();
        let renderer = MockRenderer {
            bytes: Some(png(spec.width, spec.height)),
            metadata: json!({"kind":"mock"}),
            mutate: Some(html.clone()),
        };
        assert!(capture_with(&html, &screenshot, &spec, &renderer).is_err());
        assert_eq!(fs::read(&screenshot).unwrap(), b"unknown existing file");
        assert_eq!(fs::read(&html).unwrap(), original);
        #[cfg(unix)]
        {
            let link = dir.path.join("dangling.png");
            let target = dir.path.join("not-created.png");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert!(capture_with(&html, &link, &spec, &renderer).is_err());
            assert!(!target.exists());
            assert!(
                fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        #[cfg(not(unix))]
        let _ = dir;
    }

    #[test]
    fn effective_copy_cannot_be_rewritten_by_an_injected_renderer() {
        struct Rewriter;
        impl Renderer for Rewriter {
            fn render(&self, html: &Path, screenshot: &Path, spec: &CaptureSpec) -> Result<Value> {
                fs::write(html, "<p>rewritten</p>")?;
                publish_new(screenshot, &png(spec.width, spec.height))?;
                Ok(json!({"kind":"mock"}))
            }
        }
        let (_dir, html, screenshot, spec) = fixture();
        assert!(capture_with(&html, &screenshot, &spec, &Rewriter).is_err());
        assert!(!screenshot.exists());
    }

    #[test]
    fn text_projection_excludes_head_and_normalizes_entities() {
        let html = "<!doctype html><html><head><title>Excluded</title><style>p{color:red}</style></head><body><h1>Hello&nbsp;世界 &amp; friends</h1><p>&#x41;&#66;</p></body></html>";
        assert_eq!(text_projection(html).unwrap(), "Hello 世界 & friends AB");
    }

    #[test]
    fn accepts_inline_raster_data_images_without_case_corruption() {
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD.encode(png(2, 2));
        let html = format!(
            "<style>p{{background:URL(\"data:image/png;base64,{data}\")}}</style><p><img src=\"data:image/png;base64,{data}\" alt=\"inline\"></p>"
        );
        validate_self_contained(&html).unwrap();
    }

    #[test]
    fn default_capture_validates_input_before_browser_discovery() {
        let (dir, html, screenshot, spec) = fixture();
        fs::write(&html, "<script>active()</script>").unwrap();
        let error = capture(
            &html,
            &screenshot,
            &spec,
            Some(&dir.path.join("nonexistent-chrome")),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("active HTML element"),
            "{error:#}"
        );
        assert!(!screenshot.exists());
    }

    #[test]
    fn comparable_profile_includes_browser_policy_and_parameters() {
        let (_dir, html, screenshot, spec) = fixture();
        let renderer = MockRenderer {
            bytes: Some(png(spec.width, spec.height)),
            metadata: json!({"kind":"mock","version":"test-1"}),
            mutate: None,
        };
        let first = capture_with(&html, &screenshot, &spec, &renderer).unwrap();
        let flags = first.receipt["profile"]["capture_parameters"]["chrome_flags"]
            .as_array()
            .unwrap();
        for required in [
            "--headless=new",
            "--force-device-scale-factor=1",
            "--blink-settings=preferredColorScheme=1",
            "--disable-background-networking",
        ] {
            assert!(flags.contains(&json!(required)), "missing {required}");
        }
        let same = capture_with(
            &html,
            &screenshot.with_file_name("same.png"),
            &spec,
            &renderer,
        )
        .unwrap();
        assert_eq!(
            same.receipt["profile_sha256"],
            first.receipt["profile_sha256"]
        );
        let different = CaptureSpec {
            width: spec.width + 1,
            height: spec.height,
        };
        let other = MockRenderer {
            bytes: Some(png(different.width, different.height)),
            metadata: renderer.metadata.clone(),
            mutate: None,
        };
        let changed = capture_with(
            &html,
            &screenshot.with_file_name("changed.png"),
            &different,
            &other,
        )
        .unwrap();
        assert_ne!(
            changed.receipt["profile_sha256"],
            first.receipt["profile_sha256"]
        );
    }

    #[test]
    fn explicit_discovery_checks_files_without_executing_them() {
        let dir = OwnedDir::new().unwrap();
        assert!(ChromeRenderer::discover(Some(&dir.path.join("missing-chrome"))).is_err());
        assert!(ChromeRenderer::discover(Some(&dir.path)).is_err());
        // Any executable is discoverable; only render executes/version-checks it.
        // This is the test binary, never launched by this test.
        assert!(ChromeRenderer::discover(Some(&std::env::current_exe().unwrap())).is_ok());
    }

    #[test]
    fn injected_renderers_cannot_claim_controlled_chrome_identity() {
        for claimed in ["mock", "chrome"] {
            let (_dir, html, screenshot, spec) = fixture();
            let renderer = MockRenderer {
                bytes: Some(png(spec.width, spec.height)),
                metadata: json!({"kind":claimed,"verified":true,"page_sha256":"forged","screenshot_sha256":"forged"}),
                mutate: None,
            };
            let result = capture_with(&html, &screenshot, &spec, &renderer).unwrap();
            assert_eq!(
                result.receipt["renderer"]["kind"],
                if claimed == "mock" { "mock" } else { "custom" }
            );
            assert_eq!(result.receipt["verified"], false);
            assert_ne!(result.page_sha256, "forged");
            assert_ne!(result.screenshot_sha256, "forged");
        }
    }

    #[test]
    fn receipt_binds_original_effective_page_and_decoded_screenshot() {
        let (_dir, html, screenshot, spec) = fixture();
        let original = fs::read(&html).unwrap();
        let png_bytes = png(spec.width, spec.height);
        let renderer = MockRenderer {
            bytes: Some(png_bytes.clone()),
            metadata: json!({"kind":"mock","version":"test-1"}),
            mutate: None,
        };
        let result = capture_with(&html, &screenshot, &spec, &renderer).unwrap();
        assert_eq!(result.page_sha256, sha256_file(&html).unwrap());
        assert_eq!(result.screenshot_sha256, sha256_file(&screenshot).unwrap());
        assert_eq!(result.receipt["page_sha256"], result.page_sha256);
        assert_eq!(
            result.receipt["screenshot_sha256"],
            result.screenshot_sha256
        );
        assert_eq!(
            result.receipt["effective_page_sha256"],
            result.receipt["renderer"]["metadata"]["observed_html_sha256"]
        );
        assert_ne!(
            result.receipt["effective_page_sha256"],
            result.receipt["page_sha256"]
        );
        assert_eq!(result.receipt["renderer"]["metadata"]["policy_seen"], true);
        assert_eq!(result.receipt["policy_version"], POLICY_VERSION);
        assert_eq!(result.receipt["viewport"], json!({"width":16,"height":12}));
        assert_eq!(result.receipt["theme"], "light");
        assert_eq!(result.receipt["dpr"], 1);
        assert_eq!(result.receipt["profile_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(fs::read(&html).unwrap(), original);
        assert_eq!(fs::read(&screenshot).unwrap(), png_bytes);
    }

    #[test]
    fn rejects_renderer_that_modifies_original_html() {
        let (_dir, html, screenshot, spec) = fixture();
        let renderer = MockRenderer {
            bytes: Some(png(spec.width, spec.height)),
            metadata: json!({"kind":"mock"}),
            mutate: Some(html.clone()),
        };
        assert!(capture_with(&html, &screenshot, &spec, &renderer).is_err());
        assert!(!screenshot.exists());
    }

    #[test]
    fn rejects_wrong_png_dimensions() {
        let (_dir, html, screenshot, spec) = fixture();
        let renderer = MockRenderer {
            bytes: Some(png(spec.width + 1, spec.height)),
            metadata: json!({"kind":"mock"}),
            mutate: None,
        };
        assert!(capture_with(&html, &screenshot, &spec, &renderer).is_err());
        assert!(!screenshot.exists());
    }

    #[test]
    fn rejects_missing_and_non_png_screenshots() {
        for bytes in [
            None,
            Some(b"not a PNG".to_vec()),
            Some(b"\x89PNG\r\n\x1a\ntruncated".to_vec()),
        ] {
            let (_dir, html, screenshot, spec) = fixture();
            let renderer = MockRenderer {
                bytes,
                metadata: json!({"kind":"mock"}),
                mutate: None,
            };
            assert!(capture_with(&html, &screenshot, &spec, &renderer).is_err());
            assert!(!screenshot.exists(), "invalid output must not be published");
        }
    }

    #[test]
    fn refuses_external_dependencies_and_active_markup() {
        for html in [
            "<img src='https://example.invalid/p.png'>",
            "<div onclick='x()'>x</div>",
            "<iframe srcdoc='<p>x</p>'></iframe>",
            "<link rel='stylesheet' href='local.css'>",
            "<style>@import 'remote.css';</style>",
            "<style>p{background:url(//example.invalid/a)}</style>",
            "<style>p{background:u\\72l(remote.png)}</style>",
            "<style>p{background:image-set('remote.png' 1x)}</style>",
            "<img src='&#104;ttps://example.invalid/a'>",
            "<svg><image href='remote.png'/></svg>",
            "<meta http-equiv='refresh' content='0;url=https://example.invalid'>",
            "<img src='data:image/svg+xml;base64,PHN2Zy8+'>",
        ] {
            assert!(validate_self_contained(html).is_err(), "accepted {html}");
        }
        assert!(validate_self_contained("<!doctype html><html><head><style>p{color:red}</style></head><body><p id='x'>Hello</p><a href='#x'>go</a></body></html>").is_ok());
    }

    #[test]
    fn refuses_scripts_before_rendering() {
        assert!(validate_self_contained("<html><script>alert(1)</script></html>").is_err());
    }
}

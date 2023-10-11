/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;

use serde::Deserialize;
use serde::Serialize;

/// Attempt to open a webview application window and spawn ISL servers to handle it. By default, this function
/// returns without waiting for the webview application. If `browser` is
/// `builtin-webview` on macOS, this function will wait for the webview to
/// close.
///
/// By default, on macOS:
/// - Creates an app bundle with "Sapling" branding.
/// - The current server spawn arguments are written into this app bundle.
/// - The app bundle is launched as new process.
/// - An ISL Server process is spawned by this app process, using the saved server arguments.
/// - The new process uses `webview-sys` to create a browser window, and connect to the server url.
///
/// By default, on Windows and Linux:
/// - An ISL server process is spawned by the current process to get the url for the browser to open.
/// - Try to find a chrome/edge browser and use its `--app` with the url.
pub fn open_isl(opts: ISLSpawnOptions) -> io::Result<()> {
    if should_just_launch_server(&opts) {
        let mut child = opts.spawn_isl_server(false)?;
        child.wait()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    if opts.browser.is_some() {
        // if --browser=... is passed, use browser instead of macOS app
        setup_and_spawn_chrome_like(opts)?;
    } else {
        setup_and_spawn_app_bundle(opts)?;
    }

    #[cfg(not(target_os = "macos"))]
    setup_and_spawn_chrome_like(opts)?;

    Ok(())
}

/// Check if the isl spawn options prevent opening a webview/chromelike window.
fn should_just_launch_server(opts: &ISLSpawnOptions) -> bool {
    opts.no_open || opts.kill || opts.no_app
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ISLSpawnResult {
    port: i32,
    url: String,
    token: String,
    pid: i32,
    was_server_reused: bool,
    log_file_location: String,
    cwd: String,
    command: String,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ISLSpawnOptions {
    pub port: i32,
    pub platform: String,
    pub slcommand: String,
    pub slversion: String,
    /// Path to the repository to open in ISL
    pub repo_cwd: String,
    pub json: bool,
    pub no_open: bool,
    pub foreground: bool,
    pub force: bool,
    pub kill: bool,
    /// Path to the cwd to spawn the isl bundle, from which entrypoint is a valid path.
    pub server_cwd: String,
    pub nodepath: String,
    pub entrypoint: String,
    /// None -> use native app or default chromelike,
    /// "True" -> have node app open OS default browser tab,
    ///  other string path -> launch that path as the browser with --app
    pub browser: Option<String>,
    /// If true, don't spawn the app bundle, just run the server directly and have it open an OS browser tab.
    /// If false (default), spawn with the chromelike --app or in an OS webview application.
    pub no_app: bool,
}

impl ISLSpawnOptions {
    pub fn spawn_isl_server(&self, pipe_stdout: bool) -> io::Result<Child> {
        let mut cmd = Command::new(&self.nodepath);
        cmd.current_dir(&self.server_cwd);
        cmd.arg(&self.entrypoint);
        cmd.args(["--port", &self.port.to_string()]);
        cmd.args(["--command", &self.slcommand]);
        cmd.args(["--sl-version", &self.slversion]);
        cmd.args(["--cwd", &self.repo_cwd]);
        if self.platform != "browser" {
            cmd.args(["--platform", &self.platform]);
        }
        if self.json {
            cmd.arg("--json");
        }
        if self.no_open {
            cmd.arg("--no-open");
        }
        if self.foreground {
            cmd.arg("--foreground");
        }
        if self.kill {
            cmd.arg("--kill");
        }
        if self.force {
            cmd.arg("--force");
        }
        cmd.stdin(Stdio::null());
        if pipe_stdout {
            cmd.stdout(Stdio::piped());
        }
        cmd.spawn()
    }

    pub fn spawn_isl_server_json(&self) -> io::Result<ISLSpawnResult> {
        let child = self.spawn_isl_server(true)?;
        let output = child.wait_with_output()?;
        let stdout = String::from_utf8(output.stdout).expect("invalid utf-8");

        let json = serde_json::from_str::<ISLSpawnResult>(&stdout).expect("failed to parse JSON");
        Ok(json)
    }

    /// Override arguments that make the spawned server compatible with connecting to the webview.
    pub fn replace_args_for_webview_spawn(self) -> ISLSpawnOptions {
        let mut opts = self.clone();
        opts.json = true;
        // no_open is slightly overloaded: it's used to prevent the app from spawning at all, but also passed
        // into the node server to tell it to open the browser or not.
        // If we've made it to this function call, we assume we've passed the test for opening the app bundle,
        // but if we want to open the app bundle then we shouldn't also open the browser, so we want to forward
        // "true" to the node's no_open option.
        // TODO: it might be better if we move browser opening outside of node and just handle it here instead.
        opts.no_open = true;
        opts.foreground = false;
        opts.kill = false;
        opts.platform = "webview".to_owned();
        opts
    }

    /// Override arguments that make the spawned server compatible with connecting to a chromelike browser via --app
    pub fn replace_args_for_chromelike_spawn(self) -> ISLSpawnOptions {
        let mut opts = self.clone();
        opts.json = true;
        // See replace_args_for_webview_spawn above
        opts.no_open = true;
        opts.foreground = false;
        opts.kill = false;
        opts.platform = "browser".to_owned();
        opts
    }
}

/// Setup macOS app bundle, save the configured server settings, and spawn the application in a new process.
#[cfg(target_os = "macos")]
pub fn setup_and_spawn_app_bundle(opts: ISLSpawnOptions) -> Result<(), io::Error> {
    let opts = opts.replace_args_for_webview_spawn();

    let app = ISLAppBundle::get_or_create_app_bundle()?;
    app.write_server_args(opts)?;
    app.run_app_bundle()?;

    Ok(())
}

/// Entry point for the app bundle.
/// Read past (or current) server args, spawn ISL server, then open the webview to that url.
#[cfg(target_os = "macos")]
pub fn maybe_become_webview_app() -> Option<()> {
    // this function is called from hgmain itself on all invocations, we need to only become the app
    // if it's being spawned by macOS as an app.
    let mut args = std::env::args();
    let arg0 = args.next()?;
    if !arg0.ends_with(CF_BUNDLE_EXECUTABLE) {
        return None;
    }

    let app = ISLAppBundle::get_or_create_app_bundle().expect("could not create app bundle");

    let server_options = app.read_server_args().expect("could not read server args");
    println!("Found spawn options: {:?}", server_options);

    // TODO: It might be a better idea to save an array of servers in the app state instead of just one.
    // Then, we can handle opening multiple windows (repos) in the app at the same time.
    // This would of course mean we would also spawn multiple node servers.
    // We would probably want to also save window size and position, so they can be restored fully.

    let server_output = server_options
        .spawn_isl_server_json()
        .expect("could not start server");
    println!("Started ISL server: {:?}", server_output);

    // TODO: save & read these from saved server state.
    let width = 1280;
    let height = 720;

    app.run_webview_sys(&server_output.url, width, height);
    std::process::exit(0);
}

struct ISLAppBundle {
    app_dir: PathBuf,
}

#[cfg(target_os = "macos")]
const SERVER_ARGS_DIR: &str = "Contents/Resources/server_args.json";

#[cfg(target_os = "macos")]
impl ISLAppBundle {
    /// Create an app bundle for the application.
    fn get_or_create_app_bundle() -> Result<ISLAppBundle, io::Error> {
        let dir = match dirs::data_local_dir() {
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "no data local dir")),
            Some(dir) => dir,
        };
        let app_dir = dir.join("Sapling/Sapling.app");
        fs::create_dir_all(app_dir.join("Contents/MacOS"))?;
        fs::create_dir_all(app_dir.join("Contents/Resources"))?;
        fs::write(
            app_dir.join("Contents/Info.plist"),
            include_bytes!("Info.plist"),
        )?;
        fs::write(
            app_dir.join("Contents/Resources/Icon.icns"),
            include_bytes!("Icon.icns"),
        )?;

        let current_exe = std::env::current_exe()?;
        let app_exe_path = app_dir.join("Contents/MacOS/").join(CF_BUNDLE_EXECUTABLE);
        if current_exe != app_exe_path {
            let is_symlink_ok = match fs::read_link(&app_exe_path) {
                Ok(target) => target == current_exe || target == app_exe_path,
                Err(_) => false,
            };
            if !is_symlink_ok {
                // Recreate the symlink.
                let _ = fs::remove_file(&app_exe_path);
                std::os::unix::fs::symlink(&current_exe, app_exe_path)?;
            }
        }

        Ok(ISLAppBundle { app_dir })
    }

    fn read_server_args(&self) -> Result<ISLSpawnOptions, io::Error> {
        let server_args_json = fs::read_to_string(self.app_dir.join(SERVER_ARGS_DIR))?;
        let json = serde_json::from_str::<ISLSpawnOptions>(&server_args_json)?;
        // // TODO: read args from the app bundle.
        // let width = 640;
        // let height = 480;
        Ok(json)
    }

    fn write_server_args(&self, opts: ISLSpawnOptions) -> Result<(), io::Error> {
        fs::write(
            self.app_dir.join(SERVER_ARGS_DIR),
            serde_json::to_vec(&opts)?,
        )?;
        Ok(())
    }

    /// Launch the app bundle in a new process via 'open'.
    fn run_app_bundle(&self) -> io::Result<()> {
        // Use 'open' to run the app.
        let mut command = Command::new("/usr/bin/open");
        command.arg(&self.app_dir).spawn()?;

        Ok(())
    }

    /// Open a browser window using webview-sys.
    /// Block until the webview is closed.
    fn run_webview_sys(&self, url: &str, width: i32, height: i32) {
        // Use webview-sys directly in this process.
        let url_cstr = std::ffi::CString::new(url).unwrap_or_default();
        unsafe {
            let resizable = true;
            let debug = true;
            let frameless = false;
            let visible = true;
            let min_width = 320;
            let min_height = 240;
            let hide_instead_of_close = false;
            let inner = webview_sys::webview_new(
                b"Sapling Interactive Smartlog\0" as *const u8 as _,
                url_cstr.as_bytes_with_nul().as_ptr() as _,
                width,
                height,
                resizable as _,
                debug as _,
                frameless as _,
                visible as _,
                min_width,
                min_height,
                hide_instead_of_close as _,
                None,
                std::ptr::null_mut(),
            );
            loop {
                let should_exit = webview_sys::webview_loop(inner, 1);
                if should_exit != 0 {
                    break;
                }
            }
        }
    }
}

pub fn setup_and_spawn_chrome_like(opts: ISLSpawnOptions) -> Result<(), io::Error> {
    // TODO: save & read saved server state, to remember windows size and position?
    let opts = opts.replace_args_for_chromelike_spawn();

    let server_output = opts
        .spawn_isl_server_json()
        .expect("could not start server");
    println!("Started ISL server: {:?}", server_output);

    let width = 1280;
    let height = 720;
    let chrome_opts = ISLChromelikeOptions {
        url: &server_output.url,
        width,
        height,
    };

    chrome_opts.run_chrome_like(opts.browser)?;

    Ok(())
}

struct ISLChromelikeOptions<'a> {
    url: &'a str,
    width: i32,
    height: i32,
}

impl ISLChromelikeOptions<'_> {
    /// Spawn a chrome-like browser to fulfil the webview request.
    fn run_chrome_like(&self, browser_path: Option<String>) -> io::Result<()> {
        let browser_path = match browser_path {
            None => find_chrome_like()?,
            Some(path) => path,
        };

        let mut command = Command::new(browser_path);
        if self.width > 0 && self.height > 0 {
            command.arg(format!("--window-size={},{}", self.width, self.height));
        }
        command.arg(format!("--app={}", self.url));
        if let Some(dir) = dirs::data_local_dir() {
            let dir = dir.join("Sapling").join("Webview");
            fs::create_dir_all(&dir)?;
            command.arg(format!("--user-data-dir={}", dir.display()));
        }
        command.spawn()?;
        Ok(())
    }
}

fn find_chrome_like() -> io::Result<String> {
    if cfg!(target_os = "windows") {
        let program_files = [
            std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()),
            std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()),
        ];
        let relative_paths = [
            r"\Microsoft\Edge\Application\msedge.exe",
            r"\Google\Chrome\Application\chrome.exe",
        ];
        for dir in program_files {
            for path in relative_paths {
                let full_path_str = format!("{dir}{path}");
                let full_path = Path::new(&full_path_str);
                if full_path.exists() {
                    return Ok(full_path_str);
                }
            }
        }
    } else {
        let candiates = [
            "/usr/bin/chromium",
            "/usr/bin/google-chrome",
            "/usr/bin/microsoft-edge",
            #[cfg(target_os = "macos")]
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ];
        for path in candiates {
            if Path::new(path).exists() {
                return Ok(path.to_string());
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Cannot find a chrome browser for webview",
    ))
}

// Match Info.plist CFBundleExecutable.
#[cfg(target_os = "macos")]
const CF_BUNDLE_EXECUTABLE: &str = "Interactive Smartlog";

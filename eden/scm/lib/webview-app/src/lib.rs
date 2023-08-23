/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

/// Attempt to open a webview application window. By default, this function
/// returns without waiting for the webview application. If `browser` is
/// `builtin-webview` on macOS, this function will wait for the webview to
/// close.
///
/// By default, on macOS:
/// - Creates an app bundle with "Sapling" branding.
/// - The app bundle launches a new process as `CFBundleExecutable URL SIZE`.
/// - The new process uses `webview-sys` to create a browser window.
///
/// By default, on Windows and Linux:
/// - Try to find a chrome/edge browser and use its `--app`.
///
/// `browser` can be a path to a Chrome-like browser to override
/// the default behavior. It can be `builtin-webview` to launch
/// the webview without app bundle.
pub fn open(url: &str, width: i32, height: i32, browser: Option<String>) -> io::Result<()> {
    let opts = WebviewOptions { url, width, height };

    #[cfg(target_os = "macos")]
    if browser.is_none() {
        opts.run_app_bundle()?;
    } else if browser.as_deref() == Some("builtin-webview") {
        opts.run_webview_sys();
    } else {
        opts.run_chrome_like(browser)?;
    }

    #[cfg(not(target_os = "macos"))]
    opts.run_chrome_like(browser)?;

    Ok(())
}

/// Entry point for the app bundle.
/// Run webview-sys and does not return if started as the app.
/// Return if this function does nothing.
#[cfg(target_os = "macos")]
pub fn maybe_become_webview_app() -> Option<()> {
    let mut args = std::env::args();
    let arg0 = args.next()?;
    if !arg0.ends_with(CF_BUNDLE_EXECUTABLE) {
        return None;
    }
    let url = args.next()?;
    let width: i32 = args.next()?.parse().ok().unwrap_or(800);
    let height: i32 = args.next()?.parse().ok().unwrap_or(600);
    let opts = WebviewOptions {
        url: &url,
        width,
        height,
    };
    opts.run_webview_sys();
    std::process::exit(0);
}

struct WebviewOptions<'a> {
    url: &'a str,
    width: i32,
    height: i32,
}

#[cfg(target_os = "macos")]
impl WebviewOptions<'_> {
    /// Create an app bundle and launch it via 'open'.
    /// Requires the main() logic to handle "webview" arg0 by calling
    /// `maybe_start_webview()`.
    fn run_app_bundle(&self) -> io::Result<()> {
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
        let is_symlink_ok = match fs::read_link(&app_exe_path) {
            Ok(target) => target == current_exe,
            Err(_) => false,
        };
        if !is_symlink_ok {
            // Recreate the symlink.
            let _ = fs::remove_file(&app_exe_path);
            std::os::unix::fs::symlink(&current_exe, app_exe_path)?;
        }

        // Use 'open' to run the app.
        let mut command = Command::new("/usr/bin/open");
        command
            .arg(app_dir)
            .args(["--args", self.url])
            .args([self.width.to_string(), self.height.to_string()])
            .spawn()?;

        Ok(())
    }

    /// Open a browser window using webview-sys.
    /// Block until the webview is closed.
    fn run_webview_sys(&self) {
        // Use webview-sys directly in this process.
        let url_cstr = std::ffi::CString::new(self.url).unwrap_or_default();
        unsafe {
            let resizable = true;
            let debug = true;
            let frameless = false;
            let visible = true;
            let min_width = 0;
            let min_height = 0;
            let hide_instead_of_close = false;
            let inner = webview_sys::webview_new(
                b"Sapling\0" as *const u8 as _,
                url_cstr.as_bytes_with_nul().as_ptr() as _,
                self.width,
                self.height,
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

impl WebviewOptions<'_> {
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
            std::env::var("ProgramFiles(x86)")
                .unwrap_or_else(|_| r#"C:\Program Files (x86)"#.into()),
            std::env::var("ProgramFiles").unwrap_or_else(|_| r#"C:\Program Files"#.into()),
        ];
        let relative_paths = [
            r#"\Microsoft\Edge\Application\msedge.exe"#,
            r#"\Google\Chrome\Application\chrome.exe"#,
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

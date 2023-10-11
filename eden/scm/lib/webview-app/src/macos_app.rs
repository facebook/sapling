/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use serde::Deserialize;
use serde::Serialize;

use crate::ISLSpawnOptions;

/// Setup macOS app bundle, save the configured server settings, and spawn the application in a new process.
#[cfg(target_os = "macos")]
pub fn setup_and_spawn_app_bundle(opts: ISLSpawnOptions) -> anyhow::Result<()> {
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

    // start the webview, and print any error it encounters
    start_webview_app().unwrap_or_else(|e| eprintln!("error starting webview app: {}", e));
    Some(())
}

#[cfg(target_os = "macos")]
fn start_webview_app() -> anyhow::Result<()> {
    let app =
        ISLAppBundle::get_or_create_app_bundle().context("could not create ISL app bundle")?;

    let server_options = app
        .read_server_args()
        .context("could not read saved server args")?;
    println!("Found spawn options: {:?}", server_options);

    // TODO: It might be a better idea to save an array of servers in the app state instead of just one.
    // Then, we can handle opening multiple windows (repos) in the app at the same time.
    // This would of course mean we would also spawn multiple node servers.
    // We would probably want to also save window size and position, so they can be restored fully.

    let server_output = server_options
        .spawn_isl_server_json()
        .context("could not start ISL server")?;

    // TODO: save & read these from saved server state.
    let width = 1280;
    let height = 720;

    app.run_webview_sys(&server_output.url, width, height);
    std::process::exit(0);
}

#[cfg(target_os = "macos")]
struct ISLAppBundle {
    app_dir: PathBuf,
}

const SERVER_ARGS_DIR: &str = "Contents/Resources/server_args.json";

#[cfg(target_os = "macos")]
impl ISLAppBundle {
    /// Create an app bundle for the application.
    pub(crate) fn get_or_create_app_bundle() -> anyhow::Result<ISLAppBundle> {
        let dir = match dirs::data_local_dir() {
            None => {
                return Err(anyhow::anyhow!("no data local dir"));
            }
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

    /// Read the server args from the app bundle, which should have been previously written
    pub(crate) fn read_server_args(&self) -> anyhow::Result<ISLSpawnOptions> {
        let server_args_json = fs::read_to_string(self.app_dir.join(SERVER_ARGS_DIR))?;
        let json = serde_json::from_str::<ISLSpawnOptions>(&server_args_json)?;
        // // TODO: read args from the app bundle.
        // let width = 640;
        // let height = 480;
        Ok(json)
    }

    /// Write the server args to the app bundle, to be used the next time the app is launched.
    pub(crate) fn write_server_args(&self, opts: ISLSpawnOptions) -> anyhow::Result<()> {
        fs::write(
            self.app_dir.join(SERVER_ARGS_DIR),
            serde_json::to_vec(&opts)?,
        )?;
        Ok(())
    }

    /// Launch the app bundle in a new process via 'open'.
    pub(crate) fn run_app_bundle(&self) -> anyhow::Result<()> {
        // Use 'open' to run the app.
        let mut command = Command::new("/usr/bin/open");
        command.arg(&self.app_dir).spawn()?;

        Ok(())
    }

    /// Open a browser window using webview-sys.
    /// Block until the webview is closed.
    pub(crate) fn run_webview_sys(&self, url: &str, width: i32, height: i32) {
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
                Some(handle_webview_invoke),
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

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd")]
enum WebviewInvokeMessage {
    #[serde(rename = "openExternal")]
    OpenExternal { url: String },
    #[serde(rename = "confirm")]
    Confirm {
        id: i32,
        message: String,
        details: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "cmd")]
enum WebviewInvokeResponse {
    #[serde(rename = "confirm")]
    Confirm { id: i32, ok: bool },
}

#[cfg(target_os = "macos")]
extern "C" fn handle_webview_invoke(webview: *mut webview_sys::CWebView, arg: *const i8) {
    let arg = unsafe { std::ffi::CStr::from_ptr(arg).to_string_lossy().to_string() };

    tracing::debug!("Webview invoked: {}", arg);

    let message: WebviewInvokeMessage = match serde_json::from_str(&arg) {
        Err(e) => {
            tracing::warn!("Failed to parse JSON message from webview: {}", e);
            return;
        }
        Ok(m) => m,
    };

    fn respond(
        webview: *mut webview_sys::CWebView,
        message: WebviewInvokeResponse,
    ) -> anyhow::Result<()> {
        let response: String = serde_json::to_string(&message)?;
        // This evals JS code, which could be a security concern.
        // however, we're only sending back serialized JSON so it should be ok.
        let js = format!("window.islWebviewHandleResponse({});", response);
        let js_cstr = std::ffi::CString::new(js).unwrap();
        let ret = unsafe { webview_sys::webview_eval(webview, js_cstr.as_ptr()) };
        if ret != 0 {
            Err(anyhow::Error::msg(
                "failed to execute javascript in webview to respond",
            ))
        } else {
            Ok(())
        }
    }

    let _ = match message {
        WebviewInvokeMessage::OpenExternal { url } => {
            open::that(url).context("could not open external url")
        }
        WebviewInvokeMessage::Confirm {
            id,
            message,
            details,
        } => {
            let result = tinyfiledialogs::message_box_ok_cancel(
                "", // message is usually too long for the title
                &vec![message, details.unwrap_or_default()].join("\n\n"),
                tinyfiledialogs::MessageBoxIcon::Warning,
                tinyfiledialogs::OkCancel::Ok,
            );
            let ok = result == tinyfiledialogs::OkCancel::Ok;
            respond(webview, WebviewInvokeResponse::Confirm { id, ok })
        }
    };
}

// Match Info.plist CFBundleExecutable.
#[cfg(target_os = "macos")]
const CF_BUNDLE_EXECUTABLE: &str = "Interactive Smartlog";

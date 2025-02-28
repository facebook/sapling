/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Context;

use crate::ISLSpawnOptions;

pub fn setup_and_spawn_chrome_like(opts: ISLSpawnOptions) -> anyhow::Result<()> {
    // TODO: save & read saved server state, to remember windows size and position?
    let opts = opts.replace_args_for_chromelike_spawn();

    let server_output = opts
        .spawn_isl_server_json()
        .context("could not start ISL server")?;

    let width = 1280;
    let height = 720;
    let chrome_opts = ISLChromelikeOptions {
        url: &server_output.url,
        width,
        height,
        user_data_dir: opts.chromelike_user_data_dir.as_deref(),
    };

    chrome_opts.run_chrome_like(opts.browser)?;

    Ok(())
}

struct ISLChromelikeOptions<'a> {
    url: &'a str,
    width: i32,
    height: i32,
    user_data_dir: Option<&'a Path>,
}

impl ISLChromelikeOptions<'_> {
    /// Spawn a chrome-like browser to fulfil the webview request.
    fn run_chrome_like(&self, browser_path: Option<String>) -> anyhow::Result<()> {
        let browser_path = match browser_path {
            None => find_chrome_like()?,
            Some(path) => path,
        };

        let mut command = Command::new(browser_path);
        if self.width > 0 && self.height > 0 {
            command.arg(format!("--window-size={},{}", self.width, self.height));
        }
        command.arg(format!("--app={}", self.url));
        if let (Some(chromelike_user_data_dir), Some(dir)) =
            (self.user_data_dir, dirs::data_local_dir())
        {
            let dir = dir.join(chromelike_user_data_dir);
            fs::create_dir_all(&dir)?;
            command.arg(format!("--user-data-dir={}", dir.display()));
        }
        command.spawn()?;
        Ok(())
    }
}

fn find_chrome_like() -> anyhow::Result<String> {
    if cfg!(target_os = "windows") {
        let program_files = [
            std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()),
            std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()),
        ];
        let relative_paths = [
            r"\Google\Chrome\Application\chrome.exe",
            r"\Microsoft\Edge\Application\msedge.exe",
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
        let candidates = [
            "/usr/bin/chromium",
            "/usr/bin/google-chrome",
            "/usr/bin/microsoft-edge",
            #[cfg(target_os = "macos")]
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ];
        for path in candidates {
            if Path::new(path).exists() {
                return Ok(path.to_string());
            }
        }
    }

    Err(anyhow::anyhow!("Cannot find a chrome browser for webview"))
}

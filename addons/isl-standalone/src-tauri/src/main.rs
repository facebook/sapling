/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::process::Command;

use serde::Deserialize;
use tauri::api::dialog::MessageDialogBuilder;
use tauri::api::dialog::MessageDialogKind;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ISLOutput {
    url: String,
    port: u32,
    pid: u32,
}

fn main() {
    open_isl_window();
}

fn get_spawn_isl_args() -> Vec<String> {
    let mut cmds: Vec<String> = std::env::args().skip(1).collect();

    // no args provided -> default to "sl web"
    if cmds.len() == 0 {
        cmds.insert(0, "sl".to_string());
        cmds.insert(1, "web".to_string());
    }

    cmds
}

fn spawn_isl_server() -> Result<ISLOutput, String> {
    let args = get_spawn_isl_args();

    println!("Spawning ISL server with: `{}`", args.join(" "));

    let result = Command::new(args.first().clone().unwrap())
        .args(args.iter().skip(1).collect::<Vec<&String>>())
        .arg("--json")
        .arg("--no-open")
        .arg("--platform")
        .arg("standalone")
        .output()
        .map_err(|err| err.to_string())?;

    let isl_output = String::from_utf8_lossy(&result.stdout);
    let json: ISLOutput = serde_json::from_str(&isl_output).map_err(|err| {
        format!(
            "Could not parse JSON: {}\nstdout: {}\nstderr: {}",
            err.to_string(),
            isl_output,
            String::from_utf8_lossy(&result.stderr)
        )
    })?;

    println!("output from ISL: {:?}", json);
    Ok(json)
}

fn open_isl_window() {
    tauri::Builder::default()
        .setup(move |app| {
            let json = spawn_isl_server();

            match json {
                Ok(json) => {
                    tauri::WindowBuilder::new(
                        app,
                        "external",
                        tauri::WindowUrl::External(json.url.parse().unwrap()),
                    )
                    .title("Interactive Smartlog")
                    .inner_size(1200.0, 1000.0)
                    .build()?;
                }
                Err(err) => {
                    show_error_dialog(app.handle(), "Error starting ISL".into(), err);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn show_error_dialog(handle: tauri::AppHandle, title: String, err: String) {
    println!("{}", err);
    std::thread::spawn(move || {
        // the dialog requires a window to show successfully,
        // so we make a hidden blank window
        _ = tauri::WindowBuilder::new(&handle, "Error", tauri::WindowUrl::App("index.html".into()))
            .visible(false)
            .build()
            .unwrap();

        MessageDialogBuilder::new(title, err.to_string())
            .kind(MessageDialogKind::Error)
            .show(|_| {
                std::process::exit(1);
            });
    });
}

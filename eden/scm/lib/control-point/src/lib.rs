/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::bail;
use anyhow::Result;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use serde_json;
use serde_json::Map;
use serde_json::Value;
use tracing::debug;
use util::file::atomic_write;

const FILE_NAME_ENV: &str = "CONTROL_POINT_FILE";

lazy_static! {
    static ref STATE: Mutex<Value> = Mutex::new(Value::Object(Map::new()));
}

pub fn control_point(name: &str) {
    if env::var(FILE_NAME_ENV).is_ok() {
        let result = (|| {
            if let Some((instruction, args)) = read_instruction(name)? {
                execute(name, &instruction, args)
            } else {
                Ok(())
            }
        })();
        if let Err(e) = result {
            panic!("control-point({:?}) failed: {:?}", name, e);
        }
    }
}

fn execute(name: &str, instruction: &str, args: Map<String, Value>) -> Result<()> {
    STATE
        .lock()
        .as_object_mut()
        .unwrap()
        .insert(name.to_string(), Value::String("processing".to_string()));
    write_state()?;

    let result = (|| -> Result<()> {
        let mut current_instruction = instruction.to_string();
        loop {
            debug!(
                "{:?} - processing instruction: {:?} with arguments {:?}",
                name, instruction, args
            );
            match current_instruction.as_str() {
                "wait" => {
                    std::thread::sleep(Duration::from_millis(1));
                    if let Some((instruction, _args)) = read_instruction(name)? {
                        current_instruction = instruction.to_string();
                    } else {
                        bail!(
                            "missing instruction for currently waiting {:?} control-point",
                            name
                        );
                    }

                    continue;
                }
                "continue" => {
                    return Ok(());
                }
                "panic" => {
                    panic!(
                        "{:?}",
                        args.get("message")
                            .map(|m| m.to_string())
                            .unwrap_or_else(|| "control-point panic".to_string())
                    );
                }
                _ => {
                    bail!(
                        "unknown control-point instruction {:?} with arguments {:?}",
                        instruction,
                        args
                    );
                }
            };
        }
    })();

    STATE.lock().as_object_mut().unwrap().remove(name);
    write_state()?;
    result
}

fn read_instruction(name: &str) -> Result<Option<(String, Map<String, Value>)>> {
    // Unwrap since this code can only be reached if we go through the env check in
    // control_point().
    let contents = fs::read_to_string(env::var(FILE_NAME_ENV).unwrap())?;
    let mut instructions = match serde_json::from_str(&contents)? {
        Value::Object(map) => map,
        _ => bail!("invalid control-point instruction contents {:?}", contents),
    };

    Ok(match instructions.remove(name) {
        Some(Value::Array(instruction)) => {
            let len = instruction.len();
            if len != 1 && len != 2 {
                bail!("invalid control-point instruction {:?}", instruction);
            }
            let mut iter = instruction.into_iter();
            let (name, args) = match (iter.next(), iter.next()) {
                (Some(Value::String(name)), None) => (name, Map::new()),
                (Some(Value::String(name)), Some(Value::Object(map))) => (name, map),
                (first, second) => bail!(
                    "invalid control-point instructions {:?} & {:?}",
                    first,
                    second
                ),
            };

            Some((name, args))
        }
        Some(other) => bail!("invalid control-point non-array instruction {:?}", other),
        None => None,
    })
}

fn write_state() -> Result<()> {
    let path = Path::new(&env::var(FILE_NAME_ENV).unwrap()).with_extension("response");
    atomic_write(&path, |f| {
        f.write(STATE.lock().to_string().as_bytes())?;
        Ok(())
    })?;
    Ok(())
}

pub fn wait(file: &Path, name: &str, timeout: &Duration) -> Result<()> {
    let now = std::time::Instant::now();
    loop {
        if now.elapsed() > *timeout {
            bail!("waited too long for control-point {:?} in {:?}", name, file);
        }
        if !file.exists() {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        let contents = fs::read_to_string(file)?;
        let map = match serde_json::from_str(&contents) {
            Ok(Value::Object(map)) => map,
            _ => bail!("invalid json map in control-point response {:?}", contents),
        };
        if let Some(value) = map.get(name) {
            if value == "processing" {
                return Ok(());
            }
        }

        std::thread::sleep(Duration::from_millis(1));
    }
}

pub fn set_actions(path: &Path, map: &Value) -> Result<()> {
    atomic_write(path, |f| {
        f.write(map.to_string().as_bytes())?;
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::thread::spawn;

    use serde_json::json;
    use tempdir::TempDir;

    use super::*;

    #[test]
    fn test_wait() {
        let timeout = Duration::from_millis(5000);

        let dir = TempDir::new("testdir").expect("tempdir");
        let file = dir.path().join("control_point_file");
        let response_file = file.with_extension("response");
        std::env::set_var(FILE_NAME_ENV, &file);

        set_actions(
            &file,
            &json!({
                "first": ("wait", ),
                "second": ("wait", ),
                "third": ("wait", ),

            }),
        )
        .unwrap();

        let complete = Arc::new(AtomicU64::new(0));
        let complete2 = complete.clone();
        let action_thread = spawn(move || {
            complete2.fetch_add(1, Ordering::SeqCst);
            control_point("first");
            complete2.fetch_add(1, Ordering::SeqCst);
            control_point("second");
            complete2.fetch_add(1, Ordering::SeqCst);
            control_point("third");
            complete2.fetch_add(1, Ordering::SeqCst);
        });

        wait(&response_file, "first", &timeout).unwrap();
        assert_eq!(complete.load(Ordering::SeqCst), 1);
        set_actions(
            &file,
            &json!({
                "first": ("continue", ),
                "second": ("wait", ),
                "third": ("wait", ),

            }),
        )
        .unwrap();

        wait(&response_file, "second", &timeout).unwrap();
        assert_eq!(complete.load(Ordering::SeqCst), 2);
        set_actions(
            &file,
            &json!({
                "second": ("continue", ),
                "third": ("wait", ),

            }),
        )
        .unwrap();

        wait(&response_file, "third", &timeout).unwrap();
        assert_eq!(complete.load(Ordering::SeqCst), 3);
        set_actions(
            &file,
            &json!({
                "third": ("continue", ),

            }),
        )
        .unwrap();

        action_thread.join().unwrap();
        assert_eq!(complete.load(Ordering::SeqCst), 4);
    }
}

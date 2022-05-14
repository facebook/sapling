/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Provides a way to replace the `tracing_subscriber::EnvFilter` and the
//! output of something that takes an `io::Write`
//! (ex. `tracing_subscriber::fmt::Layer`) at runtime.
//!
//! Ideally, `tracing::subscriber::set_global_default` can just replace
//! the global subscriber at runtime.

use std::io;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::bail;
use anyhow::Result;
use once_cell::sync::Lazy;
use tracing::Subscriber;
use tracing_subscriber::reload;
use tracing_subscriber::EnvFilter;

static UPDATE_ENV_FILTER: Lazy<Mutex<Option<Box<dyn (Fn(&str) -> Result<()>) + Send + Sync>>>> =
    Lazy::new(|| Mutex::new(None));
static ENV_DIRECTIVES: Lazy<Mutex<String>> = Lazy::new(Default::default);

/// Update the directives used by `EnvFilter` returned by a previous or future
/// `reloadable_env_filter()`.
pub fn update_env_filter_directives(dirs: &str) -> Result<()> {
    *ENV_DIRECTIVES.lock().unwrap() = dirs.to_string();
    let locked = UPDATE_ENV_FILTER.lock().unwrap();
    if let Some(func) = locked.as_ref() {
        (func)(dirs)
    } else {
        Ok(())
    }
}

/// An `EnvFilter` that supports updating directives at runtime.
/// Can only create one `EnvFilter` per process.
pub fn reloadable_env_filter<S>() -> Result<reload::Layer<EnvFilter, S>>
where
    S: Subscriber,
{
    let mut locked = UPDATE_ENV_FILTER.lock().unwrap();
    if locked.is_some() {
        bail!("only one reloadable EnvFilter can be created per process");
    }
    let dirs = ENV_DIRECTIVES.lock().unwrap();
    let new_filter = |dirs: &str| {
        // EnvFilter::try_new("") is an error.
        if dirs.is_empty() {
            Ok(EnvFilter::default())
        } else {
            EnvFilter::try_new(dirs)
        }
    };
    let layer = new_filter(&*dirs)?;
    let (layer, handle) = reload::Layer::new(layer);
    let update_env_filter = move |dirs: &str| -> Result<()> {
        let layer = new_filter(dirs)?;
        handle.reload(layer)?;
        Ok(())
    };
    *locked = Some(Box::new(update_env_filter));

    Ok(layer)
}

#[derive(Clone)]
pub struct DynWrite(Arc<Mutex<Box<dyn Write + Send + Sync + 'static>>>);

impl Write for DynWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

static RELOADABLE_WRITER: Lazy<DynWrite> =
    Lazy::new(|| DynWrite(Arc::new(Mutex::new(Box::new(io::sink())))));

/// Return an `io::Write + Clone` that wraps a reloadable `io::Write`.
pub fn reloadable_writer() -> impl io::Write + Clone {
    RELOADABLE_WRITER.clone()
}

/// Replace the wrapped `io::Write` returned by `reloadable_writer`.
pub fn update_writer(writer: Box<dyn io::Write + Send + Sync + 'static>) {
    *RELOADABLE_WRITER.0.lock().unwrap() = writer;
}

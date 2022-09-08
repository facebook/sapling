/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::Any;
use std::io;
use std::mem;
use std::sync::Arc;
use std::sync::Weak;
use std::thread::spawn;
use std::thread::JoinHandle;

use configmodel::Config;
use configmodel::ConfigExt;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use parking_lot::RwLock;
use pipe::pipe;
use pipe::PipeWriter;
use streampager::config::InterfaceMode;
use streampager::config::WrappingMode;
use streampager::Pager;
use term::make_real_term;
use term::DumbTerm;
use term::DumbTty;
use term::Term;
use termwiz::surface::change::ChangeSequence;
use termwiz::surface::Change;

mod impls;
mod term;

use crate::impls::PipeWriterWithTty;

// IO is Clone, but care must be taken to drop the IO object normally
// to ensure things are cleaned up before the process exits.
#[derive(Clone)]
pub struct IO {
    inner: Arc<Mutex<Inner>>,
}

/// Implements `io::Write` on the output stream.
#[derive(Clone)]
pub struct IOOutput(Weak<Mutex<Inner>>);

/// Implements `io::Write` on the error stream.
#[derive(Clone)]
pub struct IOError(Weak<Mutex<Inner>>);

/// Provides a way to set progress, without requiring the `&IO` reference.
#[derive(Clone)]
pub struct IOProgress(Weak<Mutex<Inner>>);

struct Inner {
    input: Box<dyn Read>,
    output: Box<dyn Write>,
    error: Option<Box<dyn Write>>,
    progress: Option<Box<dyn Write>>,
    term: Option<Box<dyn Term + Send + Sync>>,

    // Used to decide whether to render progress bars.
    output_on_new_line: bool,
    error_on_new_line: bool,
    // Whether progress is non-empty.
    progress_has_content: bool,
    // Whether progress (stderr) and stdout (is likely) sharing output.
    progress_conflict_with_output: bool,
    // Whether to redirect stdout writes to stderr. More useful for pager use-case.
    redirect_err_to_out: bool,

    // How many (nested) blocks want progress output disabled.
    progress_disabled: usize,

    pager_handle: Option<JoinHandle<streampager::Result<()>>>,
}

/// The "main" IO used by the process.
///
/// This global state makes it easier for Python bindings
/// (ex. "pyio") to obtain the IO state without needing
/// to pass the state across layers. This is similar to
/// `std::io::stdout` etc being globally accessible.
///
/// Use `IO::set_main()` to set the main IO, and `IO::main()`
/// to obtain the "main" `IO`.
static MAIN_IO_REF: Lazy<RwLock<Option<Weak<Mutex<Inner>>>>> = Lazy::new(Default::default);

pub trait IsTty {
    fn is_tty(&self) -> bool;

    fn is_stdin(&self) -> bool {
        false
    }
    fn is_stdout(&self) -> bool {
        false
    }
    fn is_stderr(&self) -> bool {
        false
    }
}

pub trait CanColor: IsTty {
    fn can_color(&self) -> bool {
        self.is_tty() && !configparser::hg::is_plain(Some("color"))
    }
}

impl<T: IsTty> CanColor for T {}

pub trait Read: io::Read + IsTty + Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;

    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

pub trait Write: io::Write + IsTty + Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;

    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

impl<T: io::Read + IsTty + Any + Send + Sync> Read for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: io::Write + IsTty + Any + Send + Sync> Write for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Write to error.
impl io::Write for IOError {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return Ok(buf.len()),
        };
        let mut inner = inner.lock();
        if inner.redirect_err_to_out {
            inner.clear_progress_for_output()?;
            inner.output_on_new_line = buf.ends_with(b"\n");
            return inner.output.write(buf);
        }
        inner.clear_progress_for_error()?;
        inner.error_on_new_line = buf.ends_with(b"\n");
        if let Some(error) = inner.error.as_mut() {
            error.write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return Ok(()),
        };
        let mut inner = inner.lock();
        if let Some(error) = inner.error.as_mut() {
            error.flush()?;
        }
        Ok(())
    }
}

// Write to output.
impl io::Write for IOOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return Ok(buf.len()),
        };
        let mut inner = inner.lock();
        inner.clear_progress_for_output()?;
        inner.output_on_new_line = buf.ends_with(b"\n");
        inner.output.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return Ok(()),
        };
        let mut inner = inner.lock();
        inner.output.flush()
    }
}

impl IOProgress {
    /// Set progress to the given text.
    pub fn set(&self, text: &str) -> io::Result<()> {
        let inner = match Weak::upgrade(&self.0) {
            Some(inner) => inner,
            None => return Ok(()),
        };
        let mut inner = inner.lock();
        inner.set_progress(text)
    }
}

impl IO {
    pub fn with_input<R>(&self, f: impl FnOnce(&mut dyn Read) -> R) -> R {
        f(self.inner.lock().input.as_mut())
    }

    pub fn with_output<R>(&self, f: impl FnOnce(&dyn Write) -> R) -> R {
        f(self.inner.lock().output.as_ref())
    }

    pub fn with_error<R>(&self, f: impl FnOnce(Option<&dyn Write>) -> R) -> R {
        f(self.inner.lock().error.as_deref())
    }

    /// Returns a clonable value that impls [`io::Write`] to `error` stream.
    /// The output is associated with the `IO` so if the `IO` starts a pager,
    /// the error stream will be properly redirected to the pager.
    ///
    /// If this IO is dropped, the IOError stream will be redirected to null.
    pub fn error(&self) -> IOError {
        IOError(Arc::downgrade(&self.inner))
    }

    /// Returns a clonable value that impls [`io::Write`] to `output` stream.
    /// The output is associated with the `IO` so if the `IO` starts a pager,
    /// the error stream will be properly redirected to the pager.
    ///
    /// If this IO is dropped, the IOError stream will be redirected to null.
    pub fn output(&self) -> IOOutput {
        IOOutput(Arc::downgrade(&self.inner))
    }

    /// Returns a clonable value that provides a way to set progress text.
    pub fn progress(&self) -> IOProgress {
        IOProgress(Arc::downgrade(&self.inner))
    }

    pub fn new<IS, OS, ES>(input: IS, output: OS, error: Option<ES>) -> Self
    where
        IS: Read + 'static,
        OS: Write + 'static,
        ES: Write + 'static,
    {
        let progress_conflict_with_output = match &error {
            None => false, // No progress bar.
            Some(e) => e.is_tty() && output.is_tty(),
        };

        let inner = Inner {
            input: Box::new(input),
            output: Box::new(output),
            error: error.map(|e| Box::new(e) as Box<dyn Write>),
            progress: None,
            pager_handle: None,
            term: None,
            progress_conflict_with_output,
            output_on_new_line: true,
            error_on_new_line: true,
            progress_has_content: false,
            progress_disabled: 0,
            redirect_err_to_out: false,
        };

        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    /// Stop the pager and restore outputs to stdio.
    pub fn wait_pager(&self) -> io::Result<()> {
        let mut inner = self.inner.lock();
        inner.flush()?;

        // Drop the piped streams. This sends EOF to pager.
        // XXX: Stdio is hard-coded for wait_pager.
        inner.input = Box::new(io::stdin());
        inner.output = Box::new(io::stdout());
        inner.error = Some(Box::new(io::stderr()));
        inner.redirect_err_to_out = false;
        inner.progress = None;

        // Wait for the pager (if running).
        let mut handle = None;
        mem::swap(&mut handle, &mut inner.pager_handle);
        if let Some(handle) = handle {
            let _ = handle.join();
        }

        Ok(())
    }

    pub fn write(&self, data: impl AsRef<[u8]>) -> io::Result<()> {
        let data = data.as_ref();
        let mut inner = self.inner.lock();
        inner.clear_progress_for_output()?;
        inner.output_on_new_line = data.ends_with(b"\n");
        inner.output.write_all(data)?;
        Ok(())
    }

    pub fn write_err(&self, data: impl AsRef<[u8]>) -> io::Result<()> {
        let data = data.as_ref();
        let mut inner = self.inner.lock();
        if inner.redirect_err_to_out {
            inner.clear_progress_for_output()?;
            inner.output_on_new_line = data.ends_with(b"\n");
            inner.output.write_all(data)?;
            return Ok(());
        }
        inner.clear_progress_for_error()?;
        inner.error_on_new_line = data.ends_with(b"\n");
        if let Some(ref mut error) = inner.error {
            error.write_all(data)?;
        }
        Ok(())
    }

    pub fn set_progress(&self, data: &str) -> io::Result<()> {
        let mut inner = self.inner.lock();
        inner.set_progress(data)
    }

    pub fn flush(&self) -> io::Result<()> {
        let mut inner = self.inner.lock();
        inner.flush()
    }

    pub fn stdio() -> Self {
        let progress_conflict_with_output = io::stderr().is_tty() && io::stdout().is_tty();
        let inner = Inner {
            input: Box::new(io::stdin()),
            output: Box::new(io::stdout()),
            error: Some(Box::new(io::stderr())),
            progress: None,
            pager_handle: None,
            term: None,
            progress_conflict_with_output,
            progress_has_content: false,
            progress_disabled: 0,
            output_on_new_line: true,
            error_on_new_line: true,
            redirect_err_to_out: false,
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    pub fn setup_term(&mut self) -> Result<(), termwiz::Error> {
        let mut inner = self.inner.lock();

        if std::env::var_os("TESTTMP").is_some() {
            // Use dumb terminal with static width/height for tests.
            inner.term = Some(Box::new(DumbTerm::new(DumbTty::new(Box::new(
                io::stderr(),
            )))?));
        } else {
            inner.term = Some(make_real_term()?);
        }

        Ok(())
    }

    /// Obtain the main IO.
    ///
    /// The main IO must be set via `set_main` and is still alive.
    /// Otherwise, this function will return an error.
    pub fn main() -> io::Result<Self> {
        let opt_main_io = MAIN_IO_REF.read();
        let main_io = match opt_main_io.as_ref() {
            Some(io) => io,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "IO::main() is not available (call set_main first)",
                ));
            }
        };

        if let Some(inner) = Weak::upgrade(&*main_io) {
            Ok(Self { inner })
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "IO::main() is not available (dropped)",
            ))
        }
    }

    /// Set the current IO as the main IO.
    ///
    /// Note: If the current IO gets dropped then the main IO will be dropped
    /// too and [`IO::main`] will return an error.
    pub fn set_main(&self) {
        let mut main_io_ref = MAIN_IO_REF.write();
        *main_io_ref = Some(Arc::downgrade(&self.inner));
    }

    /// Check if the pager is active.
    pub fn is_pager_active(&self) -> bool {
        let inner = self.inner.lock();
        inner.pager_handle.is_some()
    }

    /// Starts a pager.
    ///
    /// It is recommended to run [`IO::flush`] and [`IO::wait_pager`] before exiting.
    pub fn start_pager(&self, config: &dyn Config) -> io::Result<()> {
        let mut inner = self.inner.lock();
        if inner.pager_handle.is_some() {
            return Ok(());
        }
        inner.set_progress("")?;

        let mut pager = Pager::new_using_system_terminal()
            .or_else(|_| Pager::new_using_stdio())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Configure the pager.
        // The Hybrid mode is similar to "-FX" from "less".
        let mut interface_mode = InterfaceMode::Hybrid;
        // Similar to "less" default.
        let mut scroll_past_eof = false;
        // Similar to "less" behavior - lines are wrapped and copy-paste preserves long ines.
        let mut wrapping_mode = WrappingMode::GraphemeBoundary;
        if let Some(mode_str) = config.get("pager", "interface") {
            let mode = InterfaceMode::from(mode_str.as_ref());
            interface_mode = mode;
        }
        if let Ok(Some(past_eof)) = config.get_opt("pager", "scroll-past-eof") {
            scroll_past_eof = past_eof;
        }
        if let Ok(Some(wrapping_mode_str)) = config.get_opt::<String>("pager", "wrapping-mode") {
            match wrapping_mode_str.to_lowercase().as_str() {
                "word" => wrapping_mode = WrappingMode::WordBoundary,
                "unwrapped" => wrapping_mode = WrappingMode::Unwrapped,
                _ => {}
            }
        }
        pager.set_wrapping_mode(wrapping_mode);
        pager.set_scroll_past_eof(scroll_past_eof);
        pager.set_interface_mode(interface_mode);

        let (out_read, out_write) = pipe();
        let (err_read, err_write) = pipe();
        let (prg_read, prg_write) = pipe();

        let out_is_tty = inner.output.is_tty();
        let out_is_stdout = inner.output.is_stdout();
        let err_is_tty = inner
            .error
            .as_ref()
            .map(|e| e.is_tty())
            .unwrap_or_else(|| out_is_tty);

        inner.flush()?;
        inner.output = {
            let mut pipe = PipeWriterWithTty::new(out_write, out_is_tty);
            pipe.pretend_stdout = out_is_stdout;
            Box::new(pipe)
        };
        pager
            .add_stream(out_read, "")
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        // Only use the pager for error stream if error stream is a tty.
        // This makes `hg 2>foo` works as expected.
        if err_is_tty {
            inner.error = Some(Box::new(PipeWriterWithTty::new(err_write, err_is_tty)));
            let separate =
                config.get_opt::<bool>("pager", "separate-stderr").ok() == Some(Some(true));
            inner.redirect_err_to_out = !separate;
            if separate {
                pager
                    .add_error_stream(err_read, "stderr")
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            }
        }
        inner.progress = Some(Box::new(PipeWriterWithTty::new(prg_write, false)));
        pager.set_progress_stream(prg_read);

        inner.pager_handle = Some(spawn(|| {
            pager.run()?;
            Ok(())
        }));

        Ok(())
    }

    /// Disable progress rendering.
    /// - `disable_progress(true)` disables progress rendering. It can be nested.
    /// - `disable_progress(false)` cancels out a `disable_progress(true)`.
    ///   If all `disable_progress(true)` are canceled out, restore the progress
    ///   rendering.
    pub fn disable_progress(&self, disabled: bool) -> io::Result<()> {
        let mut inner = self.inner.lock();
        if disabled {
            inner.progress_disabled += 1;
            inner.set_progress("")?;
        } else {
            if inner.progress_disabled == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "disable_progress(false) called without matching disable_progress(true)",
                ));
            }
            inner.progress_disabled -= 1;
        }
        Ok(())
    }

    pub fn set_progress_pipe_writer(&self, progress: Option<PipeWriter>) {
        let mut inner = self.inner.lock();
        inner.progress = match progress {
            Some(progress) => Some(Box::new(PipeWriterWithTty::new(progress, false))),
            None => None,
        }
    }
}

impl Inner {
    pub(crate) fn flush(&mut self) -> io::Result<()> {
        self.output.flush()?;
        if let Some(ref mut error) = self.error {
            error.flush()?;
        }
        Ok(())
    }

    /// Clear the progress (temporarily) for other output.
    fn clear_progress_for_error(&mut self) -> io::Result<()> {
        if self.progress_has_content && self.progress.is_none() {
            self.progress_has_content = false;
            if let Some(ref mut term) = self.term {
                write_term_progress(term, "")
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            }
        }
        Ok(())
    }

    /// Clear the progress (temporarily) if it ("stderr") conflicts with "stdout" output.
    fn clear_progress_for_output(&mut self) -> io::Result<()> {
        // If self.progress is set. Progress writes to streampager, and does not need clear.
        if self.progress_conflict_with_output && self.progress.is_none() {
            self.clear_progress_for_error()?;
        }
        Ok(())
    }

    fn set_progress(&mut self, data: &str) -> io::Result<()> {
        let inner = self;
        let mut data = data.trim_end();
        if inner.progress_disabled > 0 {
            data = "";
        }
        if let Some(ref mut progress) = inner.progress {
            // \x0c (\f) is defined by streampager.
            let data = format!("{}\x0c", data);
            progress.write_all(data.as_bytes())?;
            progress.flush()?;
        } else {
            if !inner.output_on_new_line || !inner.error_on_new_line {
                // There is a line that hasn't ended.
                // Not suitable to render progress bars.
                data = "";
            }

            // Fast path: empty progress, unchanged.
            if data.is_empty() && !inner.progress_has_content {
                return Ok(());
            }

            // Flush pending output if it might conflict with progress.
            if inner.progress_conflict_with_output {
                inner.output.flush()?;
            }

            if let Some(ref mut term) = inner.term {
                write_term_progress(term, data)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                inner.progress_has_content = !data.is_empty();
            }
        }
        Ok(())
    }
}

/// Write data to the progress area by clearing everything after
/// cursor, writing data, then moving cursor back.
fn write_term_progress(
    term: &mut Box<dyn Term + Send + Sync>,
    data: &str,
) -> Result<(), termwiz::Error> {
    let (cols, rows) = term.size()?;
    let mut changes = ChangeSequence::new(rows, cols);
    changes.add(Change::ClearToEndOfScreen(Default::default()));
    changes.add(data);
    changes.move_to((0, 0));

    term.render(&changes.consume())?;

    Ok(())
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.set_progress("");
        let _ = self.flush();
        // Drop the output and error. This sends EOF to pager.
        self.output = Box::new(Vec::new());
        self.error = None;
        self.progress = None;
        // Wait for the pager.
        let mut handle = None;
        mem::swap(&mut handle, &mut self.pager_handle);
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

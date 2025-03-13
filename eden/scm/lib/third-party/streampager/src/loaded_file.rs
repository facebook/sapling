//! Loaded files.
//!
//! Files where the data is loaded from somewhere.

use std::borrow::Cow;
use std::cmp::max;
#[cfg(feature = "load_file")]
use std::cmp::min;
use std::ffi::OsStr;
#[cfg(feature = "load_file")]
use std::fs::File as StdFile;
use std::io::Read;
#[cfg(feature = "load_file")]
use std::io::{Seek, SeekFrom};
#[cfg(feature = "load_file")]
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(feature = "load_file")]
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
#[cfg(feature = "load_file")]
use std::time::Duration;

#[cfg(feature = "load_file")]
use memmap2::Mmap;
#[cfg(feature = "load_file")]
use notify::{DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher};

use crate::buffer::Buffer;
#[cfg(feature = "load_file")]
use crate::buffer_cache::BufferCache;
use crate::error::{Error, Result};
use crate::event::{Event, EventSender};
#[cfg(feature = "load_file")]
use crate::event::UniqueInstance;
use crate::file::{FileIndex, FileInfo, DEFAULT_NEEDED_LINES};

/// Buffer size to use when loading and parsing files.  This is also the block
/// size when parsing memory mapped files or caching files read from disk.
const BUFFER_SIZE: usize = 1024 * 1024;

#[cfg(feature = "load_file")]
/// Size of the file cache in buffers.
const CACHE_SIZE: usize = 16;

/// The data content of the file.
#[derive(Clone)]
enum FileData {
    /// Data content is being streamed from an input stream, and stored in a
    /// vector of buffers.
    Streamed { buffers: Arc<RwLock<Vec<Buffer>>> },

    #[cfg(feature = "load_file")]
    /// Data content should be read from a file on disk.
    File {
        buffer_cache: Arc<Mutex<BufferCache>>,
        events: mpsc::Sender<FileEvent>,
    },

    #[cfg(feature = "load_file")]
    /// Data content has been memory mapped.
    Mapped { mmap: Arc<Mmap> },

    #[cfg(feature = "load_file")]
    /// File is empty.
    Empty,

    /// Static content.
    Static { data: Arc<Cow<'static, [u8]>> },
}

/// Metadata about a file that is being loaded.
struct FileMeta {
    /// The index of the file.
    index: FileIndex,

    /// The loaded file's title.  Usually its name.
    title: String,

    /// Information about the file.
    info: RwLock<Vec<String>>,

    /// The length of the file that has been parsed.
    length: AtomicUsize,

    /// The offset of each newline in the file.
    newlines: RwLock<Vec<usize>>,

    /// During reload, the number of lines the file had before reloading.
    reload_old_line_count: RwLock<Option<usize>>,

    /// Set to true when the file has been loaded and parsed.
    finished: AtomicBool,

    /// Set to true when the file has been dropped. Checked by background
    /// threads to exit early.
    dropped: AtomicBool,

    /// The most recent error encountered when loading the file.
    error: RwLock<Option<Error>>,

    /// If needed_lines > newlines.len(), pause loading.
    needed_lines: AtomicUsize,

    /// CondVar to wake up file loading.
    waker: Condvar,

    /// Mutex used by waker.
    waker_mutex: Mutex<()>,
}

    #[cfg(feature = "load_file")]
/// Event triggered by changes to a file on disk.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FileEvent {
    /// File has been appended to.
    Append,

    /// File has changed and needs reloading.
    Reload,
}

/// Guard to stop reading from a file when it is dropped
struct FileGuard {
    meta: Arc<FileMeta>,
}

impl FileMeta {
    /// Create new file metadata.
    fn new(index: FileIndex, title: String) -> FileMeta {
        FileMeta {
            index,
            title,
            info: RwLock::new(Vec::new()),
            length: AtomicUsize::new(0usize),
            newlines: RwLock::new(Vec::new()),
            reload_old_line_count: RwLock::new(None),
            finished: AtomicBool::new(false),
            dropped: AtomicBool::new(false),
            error: RwLock::new(None),
            needed_lines: AtomicUsize::new(DEFAULT_NEEDED_LINES),
            waker: Condvar::new(),
            waker_mutex: Mutex::new(()),
        }
    }
}

impl FileData {
    /// Create a new streamed file.
    ///
    /// A background thread is started to read from `input` and store the
    /// content in buffers.  Metadata about loading is written to `meta`.
    ///
    /// Returns `FileData` containing the buffers that the background thread
    /// is loading into.
    fn new_streamed(
        mut input: impl Read + Send + 'static,
        meta: Arc<FileMeta>,
        event_sender: EventSender,
    ) -> FileData {
        let buffers = Arc::new(RwLock::new(Vec::new()));
        thread::Builder::new()
            .name(format!("sp-stream-{}", meta.index))
            .spawn({
                let buffers = buffers.clone();
                move || -> Result<()> {
                    let mut offset = 0usize;
                    let mut total_buffer_size = 0usize;
                    let mut waker_mutex = meta.waker_mutex.lock().unwrap();
                    loop {
                        // Check if a new buffer must be allocated.
                        if offset == total_buffer_size {
                            let mut buffers = buffers.write().unwrap();
                            buffers.push(Buffer::new(BUFFER_SIZE));
                            total_buffer_size += BUFFER_SIZE;
                        }
                        let buffers = buffers.read().unwrap();
                        let mut write = buffers.last().unwrap().write();
                        match input.read(&mut write) {
                            Ok(0) => {
                                // The end of the file has been reached.  Complete.
                                meta.finished.store(true, Ordering::SeqCst);
                                event_sender.send(Event::Loaded(meta.index))?;
                                return Ok(());
                            }
                            Ok(len) => {
                                if meta.dropped.load(Ordering::SeqCst) {
                                    return Ok(());
                                }
                                // Some data has been read.  Parse its newlines.
                                let line_count = {
                                    let mut newlines = meta.newlines.write().unwrap();
                                    for i in 0..len {
                                        if write[i] == b'\n' {
                                            newlines.push(offset + i);
                                        }
                                    }
                                    // Mark that the data has been written.  This
                                    // needs to be done here before we drop the
                                    // lock for `newlines`.
                                    offset += len;
                                    write.written(len);
                                    meta.length.fetch_add(len, Ordering::SeqCst);
                                    newlines.len()
                                };
                                while line_count >= meta.needed_lines.load(Ordering::SeqCst) {
                                    // Enough data is loaded. Pause.
                                    waker_mutex = meta.waker.wait(waker_mutex).unwrap();
                                    if meta.dropped.load(Ordering::SeqCst) {
                                        return Ok(());
                                    }
                                }
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(e) => {
                                let mut error = meta.error.write().unwrap();
                                *error = Some(e.into());
                            }
                        }
                    }
                }
            })
            .unwrap();
        FileData::Streamed { buffers }
    }

    #[cfg(feature = "load_file")]
    /// Create a new file from disk.
    fn new_file<P: AsRef<Path>>(
        path: P,
        meta: Arc<FileMeta>,
        event_sender: EventSender,
    ) -> Result<FileData> {
        let path = path.as_ref();
        let mut file = Some(StdFile::open(path)?);
        let (events, event_rx) = mpsc::channel();
        let appending = Arc::new(AtomicBool::new(false));
        let buffer_cache = Arc::new(Mutex::new(BufferCache::new(path, BUFFER_SIZE, CACHE_SIZE)));

        // Create a thread to watch for file change notifications.
        thread::Builder::new()
            .name(format!("sp-fchg-{}", meta.index))
            .spawn({
                let events = events.clone();
                let appending = appending.clone();
                let meta = meta.clone();
                let path = path.to_path_buf();
                move || -> Result<()> {
                    loop {
                        let (tx, rx) = mpsc::channel();
                        let mut watcher: RecommendedWatcher =
                            Watcher::new(tx, Duration::from_millis(500)).expect("create watcher");
                        watcher
                            .watch(path.clone(), RecursiveMode::NonRecursive)
                            .expect("watch file");
                        loop {
                            if meta.dropped.load(Ordering::SeqCst) {
                                return Ok(());
                            }
                            let event = rx.recv();
                            match event {
                                Ok(DebouncedEvent::NoticeWrite(_)) => {
                                    appending.store(true, Ordering::SeqCst);
                                    events.send(FileEvent::Append)?;
                                }
                                Ok(DebouncedEvent::Write(_)) => {
                                    appending.store(false, Ordering::SeqCst);
                                    events.send(FileEvent::Append)?;
                                }
                                Ok(DebouncedEvent::Create(_)) => {
                                    events.send(FileEvent::Append)?;
                                }
                                Ok(DebouncedEvent::Rename(_, _)) => {
                                    events.send(FileEvent::Reload)?;
                                }
                                Ok(DebouncedEvent::NoticeRemove(_))
                                | Ok(DebouncedEvent::Chmod(_)) => {
                                    events.send(FileEvent::Reload)?;
                                    break;
                                }
                                Err(_) => {
                                    // The watcher failed for some reason.
                                    // Wait before retrying.
                                    thread::sleep(Duration::from_secs(1));
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            })
            .unwrap();

        // Create a thread to load the file.
        thread::Builder::new()
            .name(format!("sp-file-{}", meta.index))
            .spawn({
                let buffer_cache = buffer_cache.clone();
                let path = path.to_path_buf();
                move || -> Result<()> {
                    let loaded_instance = UniqueInstance::new();
                    let appending_instance = UniqueInstance::new();
                    let reloading_instance = UniqueInstance::new();
                    let mut total_length = 0;
                    let mut end_data = Vec::new();
                    loop {
                        meta.length.store(total_length, Ordering::SeqCst);
                        if let Some(mut file) = file.take() {
                            let mut buffer = Vec::new();
                            buffer.resize(BUFFER_SIZE, 0);
                            loop {
                                match file.read(buffer.as_mut_slice()) {
                                    Ok(0) => break,
                                    Ok(len) => {
                                        if meta.dropped.load(Ordering::SeqCst) {
                                            return Ok(());
                                        }
                                        let mut newlines = meta.newlines.write().unwrap();
                                        for (i, byte) in buffer.iter().enumerate().take(len) {
                                            if *byte == b'\n' {
                                                newlines.push(total_length + i);
                                            }
                                        }
                                        total_length += len;
                                        meta.length.store(total_length, Ordering::SeqCst);
                                    }
                                    Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                                    Err(e) => {
                                        let mut error = meta.error.write().unwrap();
                                        *error = Some(e.into());
                                    }
                                }
                            }

                            // Attempt to read the last 4k of the file.  If the file changes, we will
                            // check this portion of the file to see if we need to reload the file.
                            let end_len = total_length.min(4096);
                            end_data.clear();
                            if file.seek(SeekFrom::End(-(end_len as i64))).is_ok() {
                                end_data.resize(end_len, 0);
                                if let Ok(len) = file.read(end_data.as_mut_slice()) {
                                    if len != end_len {
                                        end_data.clear();
                                    }
                                } else {
                                    end_data.clear();
                                }
                            }
                        }
                        let (send_event, mut reload) = if appending.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(100));
                            (false, end_data.is_empty())
                        } else {
                            meta.finished.store(true, Ordering::SeqCst);
                            event_sender
                                .send_unique(Event::Loaded(meta.index), &loaded_instance)?;
                            {
                                let mut reload_old_line_count =
                                    meta.reload_old_line_count.write().unwrap();
                                *reload_old_line_count = None;
                            }
                            match event_rx.recv() {
                                Ok(FileEvent::Append) => (true, end_data.is_empty()),
                                Ok(FileEvent::Reload) => (true, true),
                                Err(e) => {
                                    let mut error = meta.error.write().unwrap();
                                    *error = Some(e.into());
                                    return Ok(());
                                }
                            }
                        };
                        match StdFile::open(&path) {
                            Ok(mut f) => {
                                if !reload {
                                    let mut new_data = Vec::new();
                                    new_data.resize(end_data.len(), 0);
                                    let offset = total_length - end_data.len();
                                    if f.seek(SeekFrom::Start(offset as u64)).is_ok()
                                        && f.read(new_data.as_mut_slice()).ok()
                                            == Some(end_data.len())
                                        && new_data == end_data
                                    {
                                        // We can continue where we left off
                                    } else {
                                        reload = true;
                                    }
                                }
                                file = Some(f);
                            }
                            Err(_) => {
                                reload = true;
                            }
                        }
                        if reload {
                            buffer_cache.lock().unwrap().clear();
                            let mut reload_old_line_count =
                                meta.reload_old_line_count.write().unwrap();
                            let mut newlines = meta.newlines.write().unwrap();
                            let count = max(
                                reload_old_line_count.unwrap_or(0),
                                line_count(newlines.as_slice(), total_length),
                            );
                            *reload_old_line_count = Some(count);
                            newlines.clear();
                            total_length = 0;
                            if send_event {
                                event_sender.send_unique(
                                    Event::Reloading(meta.index),
                                    &reloading_instance,
                                )?;
                            }
                        } else if send_event {
                            event_sender
                                .send_unique(Event::Appending(meta.index), &appending_instance)?;
                        }
                        meta.finished.store(false, Ordering::SeqCst);
                    }
                }
            })
            .unwrap();

        Ok(FileData::File {
            buffer_cache,
            events,
        })
    }

    #[cfg(feature = "load_file")]
    /// Create a new memory mapped file.
    ///
    /// The `file` is memory mapped and then a background thread is started to
    /// parse the newlines in the file.  The parsing progress is stored in
    /// `meta`.
    ///
    /// Returns `FileData` containing the memory map.
    fn new_mapped(
        file: StdFile,
        meta: Arc<FileMeta>,
        event_sender: EventSender,
    ) -> Result<FileData> {
        // We can't mmap empty files, so just return an empty filedata if the
        // file's length is 0.
        if file.metadata()?.len() == 0 {
            meta.finished.store(true, Ordering::SeqCst);
            event_sender.send(Event::Loaded(meta.index))?;
            return Ok(FileData::Empty);
        }
        let mmap = Arc::new(unsafe { Mmap::map(&file)? });
        thread::Builder::new()
            .name(format!("sp-mmap-{}", meta.index))
            .spawn({
                let mmap = mmap.clone();
                move || -> Result<()> {
                    let len = mmap.len();
                    let blocks = (len + BUFFER_SIZE - 1) / BUFFER_SIZE;
                    for block in 0..blocks {
                        if meta.dropped.load(Ordering::SeqCst) {
                            return Ok(());
                        }
                        let mut newlines = meta.newlines.write().unwrap();
                        for i in block * BUFFER_SIZE..min((block + 1) * BUFFER_SIZE, len) {
                            if mmap[i] == b'\n' {
                                newlines.push(i);
                            }
                        }
                    }
                    meta.length.store(len, Ordering::SeqCst);
                    meta.finished.store(true, Ordering::SeqCst);
                    event_sender.send(Event::Loaded(meta.index))?;
                    Ok(())
                }
            })
            .unwrap();
        Ok(FileData::Mapped { mmap })
    }

    /// Create a new file from static data.
    ///
    /// Returns `FileData` containing the static data.
    fn new_static(
        data: impl Into<Cow<'static, [u8]>>,
        meta: Arc<FileMeta>,
        event_sender: EventSender,
    ) -> FileData {
        let data = Arc::new(data.into());
        thread::Builder::new()
            .name(format!("sp-static-{}", meta.index))
            .spawn({
                let data = data.clone();
                move || -> Result<()> {
                    let len = data.len();
                    let blocks = len.div_ceil(BUFFER_SIZE);
                    for block in 0..blocks {
                        if meta.dropped.load(Ordering::SeqCst) {
                            return Ok(());
                        }
                        let mut newlines = meta.newlines.write().unwrap();
                        for (i, byte) in data
                            .iter()
                            .enumerate()
                            .skip(block * BUFFER_SIZE)
                            .take(BUFFER_SIZE)
                        {
                            if *byte == b'\n' {
                                newlines.push(i);
                            }
                        }
                    }
                    meta.length.store(len, Ordering::SeqCst);
                    meta.finished.store(true, Ordering::SeqCst);
                    event_sender.send(Event::Loaded(meta.index))?;
                    Ok(())
                }
            })
            .unwrap();
        FileData::Static { data }
    }

    /// Runs the `call` function, passing it a slice of the data from `start` to `end`.
    /// Tries to avoid copying the data if possible.
    fn with_slice<T, F>(&self, start: usize, end: usize, mut call: F) -> T
    where
        F: FnMut(Cow<'_, [u8]>) -> T,
    {
        match self {
            FileData::Streamed { buffers } => {
                let start_buffer = start / BUFFER_SIZE;
                let end_buffer = (end - 1) / BUFFER_SIZE;
                let buffers = buffers.read().unwrap();
                if start_buffer == end_buffer {
                    let data = buffers[start_buffer].read();
                    call(Cow::Borrowed(
                        &data[start % BUFFER_SIZE..=(end - 1) % BUFFER_SIZE],
                    ))
                } else {
                    // The data spans multiple buffers, so we must make a copy to make it contiguous.
                    let mut v = Vec::with_capacity(end - start);
                    v.extend_from_slice(&buffers[start_buffer].read()[start % BUFFER_SIZE..]);
                    for b in start_buffer + 1..end_buffer {
                        v.extend_from_slice(buffers[b].read());
                    }
                    v.extend_from_slice(&buffers[end_buffer].read()[..=(end - 1) % BUFFER_SIZE]);
                    call(Cow::Owned(v))
                }
            }
    #[cfg(feature = "load_file")]
            FileData::File {
                events,
                buffer_cache,
                ..
            } => {
                let mut buffer_cache = buffer_cache.lock().unwrap();
                buffer_cache
                    .with_slice(start, end, |data| {
                        if data
                            .iter()
                            .take(data.len().saturating_sub(1))
                            .any(|c| *c == b'\n')
                        {
                            events.send(FileEvent::Reload).unwrap();
                        }
                        call(data)
                    })
                    .unwrap()
            }
    #[cfg(feature = "load_file")]
            FileData::Mapped { mmap } => call(Cow::Borrowed(&mmap[start..end])),
    #[cfg(feature = "load_file")]
            FileData::Empty => call(Cow::Borrowed(&[])),
            FileData::Static { data } => call(Cow::Borrowed(&data[start..end])),
        }
    }
}

/// A loaded file.
pub(crate) struct LoadedFile {
    /// The data for the file.
    data: FileData,

    /// Metadata about the loading of the file.
    meta: Arc<FileMeta>,

    /// Guard to stop loading the file when the original reference to it is dropped.
    _guard: Option<FileGuard>,
}

impl Clone for LoadedFile {
    fn clone(&self) -> LoadedFile {
        LoadedFile {
            data: self.data.clone(),
            meta: self.meta.clone(),
            _guard: None,
        }
    }
}

impl LoadedFile {
    fn new(data: FileData, meta: Arc<FileMeta>) -> Self {
        let _guard = Some(FileGuard { meta: meta.clone() });
        LoadedFile { data, meta, _guard }
    }

    /// Load stream.
    pub(crate) fn new_streamed(
        index: FileIndex,
        stream: impl Read + Send + 'static,
        title: &str,
        event_sender: EventSender,
    ) -> LoadedFile {
        let meta = Arc::new(FileMeta::new(index, title.to_string()));
        let data = FileData::new_streamed(stream, meta.clone(), event_sender);
        LoadedFile::new(data, meta)
    }

    #[cfg(feature = "load_file")]
    pub(crate) fn new_file(
        index: FileIndex,
        filename: &OsStr,
        event_sender: EventSender,
    ) -> Result<LoadedFile> {
        let title = filename.to_string_lossy().into_owned();
        let meta = Arc::new(FileMeta::new(index, title.to_string()));
        let mut file = StdFile::open(filename).map_err(|err| Error::from(err).with_file(title))?;
        // Determine whether this file is a real file, or some kind of pipe, by
        // attempting to do a no-op seek.  If it fails, we won't be able to seek
        // around and load parts of the file at will, so treat it as a stream.
        let data = match file.seek(SeekFrom::Current(0)) {
            Ok(_) => FileData::new_file(filename, meta.clone(), event_sender)?,
            Err(_) => FileData::new_streamed(file, meta.clone(), event_sender),
        };
        Ok(LoadedFile::new(data, meta))
    }

    #[cfg(feature = "load_file")]
    /// Load a file by memory mapping it if possible.
    #[allow(unused)]
    pub(crate) fn new_mapped(
        index: FileIndex,
        filename: &OsStr,
        event_sender: EventSender,
    ) -> Result<LoadedFile> {
        let title = filename.to_string_lossy().into_owned();
        let meta = Arc::new(FileMeta::new(index, title.clone()));
        let mut file = StdFile::open(filename).map_err(|err| Error::from(err).with_file(title))?;
        // Determine whether this file is a real file, or some kind of pipe, by
        // attempting to do a no-op seek.  If it fails, assume we can't mmap
        // it.
        let data = match file.seek(SeekFrom::Current(0)) {
            Ok(_) => FileData::new_mapped(file, meta.clone(), event_sender)?,
            Err(_) => FileData::new_streamed(file, meta.clone(), event_sender),
        };
        Ok(LoadedFile::new(data, meta))
    }

    /// Load the output and error of a command
    pub(crate) fn new_command<I, S>(
        index: FileIndex,
        command: &OsStr,
        args: I,
        title: &str,
        event_sender: EventSender,
    ) -> Result<(LoadedFile, LoadedFile)>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let title_err = format!("STDERR for {}", title);
        let mut process = Command::new(command)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| Error::from(err).with_command(command))?;
        let out = process.stdout.take().unwrap();
        let err = process.stderr.take().unwrap();
        let out_file = LoadedFile::new_streamed(index, out, title, event_sender.clone());
        let err_file = LoadedFile::new_streamed(index + 1, err, &title_err, event_sender.clone());
        thread::Builder::new()
            .name(format!("sp-cmd-{}", index))
            .spawn({
                let out_file = out_file.clone();
                move || -> Result<()> {
                    if let Ok(rc) = process.wait() {
                        if !rc.success() {
                            let mut info = out_file.meta.info.write().unwrap();
                            match rc.code() {
                                Some(code) => info.push(format!("rc: {}", code)),
                                None => info.push("killed!".to_string()),
                            }
                            event_sender.send(Event::RefreshOverlay)?;
                        }
                    }
                    Ok(())
                }
            })
            .unwrap();
        Ok((out_file, err_file))
    }

    /// Load a file from static data.
    pub(crate) fn new_static(
        index: FileIndex,
        title: &str,
        data: impl Into<Cow<'static, [u8]>>,
        event_sender: EventSender,
    ) -> LoadedFile {
        let meta = Arc::new(FileMeta::new(index, title.to_string()));
        let data = FileData::new_static(data, meta.clone(), event_sender);
        LoadedFile::new(data, meta)
    }
}

impl FileInfo for LoadedFile {
    /// The file's index.
    fn index(&self) -> FileIndex {
        self.meta.index
    }

    /// The file's title.
    fn title(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.meta.title)
    }

    /// The file's info.
    fn info(&self) -> Cow<'_, str> {
        let info = self.meta.info.read().unwrap();
        Cow::Owned(info.join(" "))
    }

    /// True once the file is loaded and all newlines have been parsed.
    fn loaded(&self) -> bool {
        self.meta.finished.load(Ordering::SeqCst)
    }

    /// Returns the number of lines in the file.
    fn lines(&self) -> usize {
        let lines = if !self.meta.finished.load(Ordering::SeqCst) {
            let reload_old_line_count = self.meta.reload_old_line_count.read().unwrap();
            reload_old_line_count.unwrap_or(0)
        } else {
            0
        };
        let newlines = self.meta.newlines.read().unwrap();
        max(
            lines,
            line_count(newlines.as_slice(), self.meta.length.load(Ordering::SeqCst)),
        )
    }

    /// Runs the `call` function, passing it the contents of line `index`.
    /// Tries to avoid copying the data if possible, however the borrowed
    /// line only lasts as long as the function call.
    fn with_line<T, F>(&self, index: usize, call: F) -> Option<T>
    where
        F: FnMut(Cow<'_, [u8]>) -> T,
    {
        let newlines = self.meta.newlines.read().unwrap();
        if index > newlines.len() {
            return None;
        }
        let start = if index == 0 {
            0
        } else {
            newlines[index - 1] + 1
        };
        let end = if index < newlines.len() {
            newlines[index] + 1
        } else {
            self.meta.length.load(Ordering::SeqCst)
        };
        if start == end {
            return None;
        }
        Some(self.data.with_slice(start, end, call))
    }

    /// Set how many lines are needed.
    ///
    /// If `self.lines()` exceeds that number, pause loading until
    /// `set_needed_lines` is called with a larger number.
    /// This is only effective for "streamed" input.
    fn set_needed_lines(&self, lines: usize) {
        // This can be simplified by `fetch_max` when it's stable.
        if self.meta.needed_lines.load(Ordering::SeqCst) >= lines {
            return;
        }
        self.meta.needed_lines.store(lines, Ordering::SeqCst);
        self.meta.waker.notify_all();
    }

    /// True if the loading thread has been paused.
    fn paused(&self) -> bool {
        !self.loaded() && self.meta.waker_mutex.try_lock().is_ok()
    }
}

impl Drop for FileGuard {
    fn drop(&mut self) {
        self.meta.dropped.store(true, Ordering::SeqCst);
        // The thread might be blocked. Wake it up so it can notice the change
        // in `dropped`.
        self.meta.waker.notify_all();
    }
}

fn line_count(newlines: &[usize], length: usize) -> usize {
    let mut lines = newlines.len();
    let after_last_newline_offset = if lines == 0 {
        0
    } else {
        newlines[lines - 1] + 1
    };
    if length > after_last_newline_offset {
        lines += 1;
    }
    lines
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(target_vendor = "apple")]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

use criterion::criterion_group;
use criterion::criterion_main;
use criterion::Bencher;
use criterion::Criterion;
use criterion::Throughput;
use rand::prelude::SliceRandom;
use rand::thread_rng;
use rand::Rng;

const PAGE_SIZE: usize = 4096;
const DEFAULT_FILE_SIZE: u64 = 16 * 1024 * 1024;

mod pthread {
    use std::ffi::c_int;

    const PTHREAD_CANCEL_DISABLE: c_int = 0;
    const PTHREAD_CANCEL_ASYNCHRONOUS: c_int = 1;

    extern "C" {
        fn pthread_setcancelstate(__state: c_int, __oldstate: *mut c_int) -> c_int;
        fn pthread_setcanceltype(__type: c_int, __oldtype: *mut c_int) -> c_int;
    }

    pub fn without_pthread_cancellation<T, F: FnOnce() -> T>(func: F) -> T {
        let mut oldstate: c_int = 0;
        let mut oldtype: c_int = 0;
        unsafe {
            assert_eq!(
                0,
                pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &mut oldstate)
            );
            assert_eq!(
                0,
                pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &mut oldtype)
            );
        }

        let result = func();

        unsafe {
            assert_eq!(0, pthread_setcanceltype(oldtype, &mut oldtype));
            assert_eq!(0, pthread_setcancelstate(oldstate, &mut oldstate));
        }

        result
    }
}

trait PosIO {
    fn read_full_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()>;
    fn write_full_at(&self, buf: &[u8], offset: u64) -> io::Result<()>;
}

#[cfg(unix)]
impl PosIO for File {
    fn read_full_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        self.read_exact_at(buf, offset)
    }

    fn write_full_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        self.write_all_at(buf, offset)
    }
}

#[cfg(windows)]
impl PosIO for File {
    fn read_full_at(&self, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match self.seek_read(buf, offset) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "unexpected eof",
                    ));
                }
                Ok(n) => {
                    offset += n as u64;
                    buf = &mut buf[n..];
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn write_full_at(&self, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match self.seek_write(buf, offset) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "unexpected zero write",
                    ));
                }
                Ok(n) => {
                    offset += n as u64;
                    buf = &buf[n..];
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

// Criterion does not support custom command-line options, so read from an
// environment variable.
fn get_tempfile_path<T: AsRef<Path>>(name: T) -> PathBuf {
    match std::env::var_os("EDENFS_BENCHMARK_DIR") {
        Some(val) => PathBuf::from(val).join(name),
        None => PathBuf::from(name.as_ref()),
    }
}

fn random_4k_reads_direct(b: &mut Bencher) {
    let mut rng = thread_rng();
    let mut page = [0u8; PAGE_SIZE];

    let path = get_tempfile_path("random_reads.tmp");

    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(target_os = "linux")]
    options.custom_flags(libc::O_DIRECT);
    #[cfg(windows)]
    options.custom_flags(winapi::um::winbase::FILE_FLAG_NO_BUFFERING);
    let file = options
        .open(&path)
        .expect(&format!("failed to open {}", path.display()));

    #[cfg(target_vendor = "apple")]
    unsafe {
        libc::fcntl(file.as_raw_fd(), libc::F_NOCACHE, 1);
    }

    std::fs::remove_file(&path).expect(&format!("failed to remove {}", path.display()));

    const FILE_SIZE: u64 = 20 * (1 << 30); // 20 GiB

    file.set_len(FILE_SIZE).expect("failed to set file size");

    const OFFSET_COUNT: usize = (FILE_SIZE / PAGE_SIZE as u64) as usize;
    let mut offsets = Vec::with_capacity(OFFSET_COUNT);
    for i in 0..OFFSET_COUNT {
        offsets.push((i * PAGE_SIZE) as u64);
    }
    offsets.shuffle(&mut rng);

    let mut offset_idx: usize = 0;

    b.iter(|| {
        let offset = offsets[offset_idx];
        offset_idx += 1;
        if offset_idx == offsets.len() {
            offset_idx = 0;
        }
        file.read_full_at(&mut page, offset)
            .expect("failed to write_full_at");
    });
}

fn random_4k_writes(b: &mut Bencher) {
    let mut rng = thread_rng();
    let mut page = [0; PAGE_SIZE];
    rng.fill(&mut page);

    let path = get_tempfile_path("random_writes.tmp");

    let file = File::create(&path).expect(&format!("failed to open {}", path.display()));
    std::fs::remove_file(&path).expect(&format!("failed to remove {}", path.display()));

    file.set_len(DEFAULT_FILE_SIZE)
        .expect("failed to set file size");

    let mut offsets = [0; (DEFAULT_FILE_SIZE / PAGE_SIZE as u64) as usize];
    let mut i: u64 = 0;
    for offset in &mut offsets {
        *offset = i;
        i += PAGE_SIZE as u64;
    }
    offsets.shuffle(&mut rng);

    let mut offset_idx: usize = 0;
    b.iter(|| {
        let offset = offsets[offset_idx];
        offset_idx += 1;
        if offset_idx == offsets.len() {
            offset_idx = 0;
        }
        file.write_full_at(&page, offset)
            .expect("failed to write_full_at");
    });
}

fn random_4k(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    group.throughput(Throughput::Bytes(PAGE_SIZE as u64));
    group.bench_function("random_4k_reads_direct", random_4k_reads_direct);
    group.bench_function("random_4k_writes", random_4k_writes);

    // glibc's pthread_cancel implementation causes a pair of atomic cmpxchg
    // operations per syscall. If pthread cancellation is disabled, the
    // implementation becomes slightly cheaper. It appears Rust automatically
    // disables pthread cancellation on a particular tested CentOS machine,
    // but try benchmarking anyway:
    if cfg!(all(target_os = "linux", not(target_env = "musl"))) {
        group.bench_function("random_4k_reads_direct_no_pthread", |b| {
            pthread::without_pthread_cancellation(|| random_4k_reads_direct(b))
        });
        group.bench_function("random_4k_writes_no_pthread", |b| {
            pthread::without_pthread_cancellation(|| random_4k_writes(b))
        });
    }
    group.finish();
}

#[allow(dead_code)]
fn print_current_exe_path() {
    // Buck2 makes it hard to see where the binary is, so print our exe path like Google Benchmark does for convenience.
    let path = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            return;
        }
    };
    eprintln!("{}", path.display());
}

criterion_group!(main_group, random_4k);
criterion_main!(main_group);

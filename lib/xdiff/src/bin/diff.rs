// Copyright 2019 Facebook, Inc.

//! A simple binary that computes the diff between two files.

use std::fs;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use xdiff::{diff_unified, CopyInfo, DiffFile, DiffOpts, FileType};

const EXEC_BIT: u32 = 0o0000100;

#[derive(Debug, StructOpt)]
#[structopt(name = "diff", about = "A showcase binary for xdiff diff library.")]

struct Opt {
    /// Input file
    #[structopt(parse(from_os_str))]
    file_a: PathBuf,

    /// Output file, stdout if not present
    #[structopt(parse(from_os_str))]
    file_b: PathBuf,

    /// Treat the <file-b> as a copy of the <file-a>
    #[structopt(short, long)]
    copy: bool,

    /// Treat the <file-b> as a move of the <file-a>
    #[structopt(short, long)]
    move_: bool,

    /// Do not follow symlinks - compare them instead (POSIX-only)
    #[structopt(short, long)]
    symlink: bool,

    /// Number of lines of unified context (default: 3)
    #[structopt(short = "U", long, default_value = "3")]
    unified: usize,
}

fn main() -> Result<(), std::io::Error> {
    let opt = Opt::from_args();

    #[cfg(target_family = "unix")]
    fn file_mode_and_contents(
        opt: &Opt,
        path: &Path,
    ) -> Result<(FileType, Vec<u8>), std::io::Error> {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        if opt.symlink && path.symlink_metadata()?.file_type().is_symlink() {
            let dest = path.read_link()?;
            let dest: &OsStr = dest.as_ref();
            Ok((FileType::Symlink, dest.as_bytes().to_owned()))
        } else if (path.metadata()?.permissions().mode() & EXEC_BIT) > 0 {
            Ok((FileType::Executable, fs::read(path)?))
        } else {
            Ok((FileType::Regular, fs::read(path)?))
        }
    }

    #[cfg(target_family = "windows")]
    fn file_mode_and_contents(
        _opt: &Opt,
        path: &Path,
    ) -> Result<(FileType, Vec<u8>), std::io::Error> {
        Ok((FileType::Regular, fs::read(path)?))
    }

    let copy_info = match (opt.copy, opt.move_) {
        (true, false) => CopyInfo::Copy,
        (false, true) => CopyInfo::Move,
        (false, false) => CopyInfo::None,
        (true, true) => panic!("file can't be marked as both copy and move"),
    };

    let a_path_str = opt.file_a.to_string_lossy();
    let a = if opt.file_a.is_file() {
        let (mode, contents) = file_mode_and_contents(&opt, &opt.file_a)?;
        Some(DiffFile::new(a_path_str.as_bytes(), contents, mode))
    } else {
        None
    };
    let b_path_str = opt.file_b.to_string_lossy();
    let b = if opt.file_b.is_file() {
        let (mode, contents) = file_mode_and_contents(&opt, &opt.file_b)?;
        Some(DiffFile::new(b_path_str.as_bytes(), contents, mode))
    } else {
        None
    };

    let diff = diff_unified(
        a,
        b,
        DiffOpts {
            context: opt.unified,
            copy_info,
        },
    );

    print!("{}", String::from_utf8_lossy(&diff));
    Ok(())
}

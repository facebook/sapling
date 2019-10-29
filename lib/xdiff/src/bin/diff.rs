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
}

fn main() -> Result<(), std::io::Error> {
    let opt = Opt::from_args();

    #[cfg(target_family = "unix")]
    fn file_mode(path: &Path) -> Result<FileType, std::io::Error> {
        if (path.metadata()?.permissions().mode() & EXEC_BIT) > 0 {
            Ok(FileType::Executable)
        } else {
            Ok(FileType::Regular)
        }
    }

    #[cfg(target_family = "windows")]
    fn file_mode(path: &Path) -> Result<FileType, std::io::Error> {
        Ok(FileType::Regular)
    }

    let copy_info = match (opt.copy, opt.move_) {
        (true, false) => CopyInfo::Copy,
        (false, true) => CopyInfo::Move,
        (false, false) => CopyInfo::None,
        (true, true) => panic!("file can't be marked as both copy and move"),
    };

    let a_path_str = opt.file_a.to_string_lossy();
    let a = if opt.file_a.is_file() {
        Some(DiffFile::new(
            a_path_str.as_bytes(),
            fs::read(&opt.file_a)?,
            file_mode(&opt.file_a)?,
        ))
    } else {
        None
    };
    let b_path_str = opt.file_b.to_string_lossy();
    let b = if opt.file_b.is_file() {
        Some(DiffFile::new(
            b_path_str.as_bytes(),
            fs::read(&opt.file_b)?,
            file_mode(&opt.file_b)?,
        ))
    } else {
        None
    };

    let diff = diff_unified(
        a,
        b,
        DiffOpts {
            context: 3,
            copy_info,
        },
    );

    print!("{}", String::from_utf8_lossy(&diff));
    Ok(())
}

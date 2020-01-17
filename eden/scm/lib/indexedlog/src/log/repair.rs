/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::{IoResultExt, ResultExt};
use crate::lock::ScopedDirLock;
use crate::log::{
    LogMetadata, OpenOptions, META_FILE, PRIMARY_FILE, PRIMARY_HEADER, PRIMARY_START_OFFSET,
};
use crate::repair::OpenOptionsRepair;
use crate::utils::{self, atomic_write, mmap_len};
use std::collections::BTreeMap;
use std::fs::{self};
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

// Repair
impl OpenOptions {
    /// Attempt to repair a broken [`Log`] at the given directory.
    ///
    /// This is done by truncating entries in the primary log, and rebuilding
    /// corrupted indexes.
    ///
    /// Backup files are written for further investigation.
    ///
    /// Return message useful for human consumption.
    pub fn repair(&self, dir: impl AsRef<Path>) -> crate::Result<String> {
        let dir = dir.as_ref();
        let mut message = String::new();
        let result: crate::Result<_> = (|| {
            if !dir.exists() {
                return Ok(format!("{:?} does not exist. Nothing to repair.\n", dir));
            }

            let lock = ScopedDirLock::new(dir)?;

            let primary_path = dir.join(PRIMARY_FILE);
            let meta_path = dir.join(META_FILE);

            // Make sure the header of the primary log file is okay.
            (|| -> crate::Result<()> {
                #[allow(clippy::never_loop)]
                let header_corrupted = loop {
                    if let Err(e) = primary_path.metadata() {
                        if e.kind() == io::ErrorKind::NotFound {
                            break true;
                        }
                    }
                    let mut file = fs::OpenOptions::new()
                        .read(true)
                        .open(&primary_path)
                        .context(&primary_path, "cannot open for read")?;
                    let mut buf = [0; PRIMARY_START_OFFSET as usize];
                    break match file.read_exact(&mut buf) {
                        Ok(_) => buf != PRIMARY_HEADER,
                        Err(_) => true,
                    };
                };
                if header_corrupted {
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .open(&primary_path)
                        .context(&primary_path, "cannot open for write")?;
                    file.write_all(PRIMARY_HEADER)
                        .context(&primary_path, "cannot re-write header")?;
                    let _ = utils::fix_perm_file(&file, false);
                    message += "Fixed header in log\n";
                }
                Ok(())
            })()
            .context("while making sure log has the right header")?;

            // Make sure the "primary_len" is large enough.
            (|| -> crate::Result<()> {
                let primary_len = primary_path
                    .metadata()
                    .context(&primary_path, "cannot read fs metadata")?
                    .len();
                match LogMetadata::read_file(&meta_path)
                    .context(&meta_path, "cannot read log metadata")
                    .context("repair cannot fix metadata corruption")
                {
                    Ok(meta) => {
                        // If metadata can be read, trust it.
                        if meta.primary_len > primary_len {
                            use fs2::FileExt;
                            // Log was truncated for some reason...
                            // (This should be relatively rare)
                            // Fill Log with 0s.
                            let file = fs::OpenOptions::new()
                                .write(true)
                                .open(&primary_path)
                                .context(&primary_path, "cannot open for write")?;
                            file.allocate(meta.primary_len)
                                .context(&primary_path, "cannot fallocate")?;
                            message += &format!(
                                "Extended log to {:?} bytes required by meta\n",
                                meta.primary_len
                            );
                        }
                    }
                    Err(meta_err) => {
                        // Attempt to rebuild metadata.
                        let meta = LogMetadata {
                            primary_len,
                            indexes: BTreeMap::new(),
                            epoch: utils::epoch(),
                        };
                        meta.write_file(&meta_path, self.fsync)
                            .context("while recreating meta")
                            .source(meta_err)?;
                        message += "Rebuilt metadata\n";
                    }
                }
                Ok(())
            })()
            .context("while making sure log.length >= meta.log_length")?;

            // Reload the latest log without indexes.
            //
            // At this time log is likely open-able.
            //
            // Try to open it with indexes so we might reuse them. If that
            // fails, retry with all indexes disabled.
            let mut log = self
                .open_with_lock(dir, &lock)
                .or_else(|_| self.clone().index_defs(Vec::new()).open(dir))
                .context("cannot open log for repair")?;

            let mut iter = log.iter();

            // Read entries until hitting a checksum error.
            let mut entry_count = 0;
            while let Some(Ok(_)) = iter.next() {
                entry_count += 1;
            }

            let valid_len = iter.next_offset;
            assert!(valid_len >= PRIMARY_START_OFFSET);
            assert!(valid_len <= log.meta.primary_len);

            if valid_len == log.meta.primary_len {
                message += &format!(
                    "Verified {} entries, {} bytes in log\n",
                    entry_count, valid_len
                );
            } else {
                message += &format!(
                    "Verified first {} entries, {} of {} bytes in log\n",
                    entry_count, valid_len, log.meta.primary_len
                );

                // Backup the part to be truncated.
                (|| -> crate::Result<()> {
                    let mut primary_file = fs::OpenOptions::new()
                        .read(true)
                        .open(&primary_path)
                        .context(&primary_path, "cannot open for read")?;
                    let backup_path = dir.join(format!(
                        "log.bak.epoch{}.offset{}",
                        log.meta.epoch, valid_len
                    ));
                    let mut backup_file = fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&backup_path)
                        .context(&backup_path, "cannot open")?;

                    primary_file
                        .seek(SeekFrom::Start(valid_len))
                        .context(&primary_path, "cannot seek")?;

                    let mut reader = io::BufReader::new(primary_file);
                    loop {
                        let len = {
                            let buf = reader.fill_buf().context(&primary_path, "cannot read")?;
                            if buf.is_empty() {
                                break;
                            }
                            backup_file
                                .write_all(buf)
                                .context(&backup_path, "cannot write")?;
                            buf.len()
                        };
                        reader.consume(len);
                    }
                    message += &format!("Backed up corrupted log to {:?}\n", backup_path);
                    Ok(())
                })()
                .context("while trying to backup corrupted log")?;

                // Update metadata. Invalidate indexes.
                // Bump epoch since this is a non-append-only change.
                // Reload disk buffer.
                log.meta.primary_len = valid_len;
                log.meta.indexes.clear();
                log.meta.epoch = log.meta.epoch.wrapping_add(1);
                log.disk_buf = Arc::new(mmap_len(&primary_path, valid_len)?);

                log.meta
                    .write_file(&meta_path, log.open_options.fsync)
                    .context("while trying to update metadata with verified log length")?;
                message += &format!("Reset log size to {}\n", valid_len);
            }

            // Also rebuild corrupted indexes.
            // Without this, indexes are empty until the next `sync`, which
            // can lead to bad performance.
            log.open_options.index_defs = self.index_defs.clone();
            message += &log
                .rebuild_indexes_with_lock(false, &lock)
                .context("while trying to update indexes with reapired log")?;

            Ok(message)
        })();

        result.context(|| format!("in log::OpenOptions::repair({:?})", dir))
    }
}

impl OpenOptionsRepair for OpenOptions {
    fn open_options_repair(&self, dir: impl AsRef<Path>) -> crate::Result<String> {
        OpenOptions::repair(self, dir.as_ref())
    }
}

impl OpenOptions {
    /// Attempt to change a [`Log`] at the given directory so it becomes
    /// empty and hopefully recovers from some corrupted state.
    ///
    /// Warning: This deletes data, and there is no backup!
    pub fn delete_content(&self, dir: impl AsRef<Path>) -> crate::Result<()> {
        let dir = dir.as_ref();
        let result: crate::Result<()> = (|| {
            // Ensure the directory exist.
            utils::mkdir_p(dir)?;

            // Prevent other writers.
            let lock = ScopedDirLock::new(dir)?;

            // Replace the metadata to an empty state.
            let meta = LogMetadata {
                primary_len: PRIMARY_START_OFFSET,
                indexes: BTreeMap::new(),
                epoch: utils::epoch(),
            };
            let meta_path = dir.join(META_FILE);
            meta.write_file(&meta_path, self.fsync)?;

            // Replace the primary log.
            let primary_path = dir.join(PRIMARY_FILE);
            atomic_write(primary_path, PRIMARY_HEADER, self.fsync)?;

            // Replace indexes so they become empty.
            let log = self
                .clone()
                .create(true)
                .open_with_lock(dir, &lock)
                .context("cannot open")?;
            log.rebuild_indexes_with_lock(true, &lock)?;

            Ok(())
        })();

        result.context(|| format!("in log::OpenOptions::delete_content({:?})", dir))
    }
}

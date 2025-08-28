/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![allow(non_camel_case_types)]

use std::fs::File;
use std::io;
use std::mem::offset_of;
use std::mem::size_of;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::ptr;

/// Return file's physical size on disk (i.e. compressed size).
/// This is relatively slow and fsyncs the file, so don't call a lot.
pub fn physical_size(file: &File) -> io::Result<u64> {
    // btrfs can buffer a _lot_ of data before writing extents to disk. Unfortunately that means we
    // really must fsync to get an accurate size reading.
    file.sync_all()?;

    let ino = file.metadata()?.ino();

    #[repr(C)]
    #[derive(Debug, Copy, Clone, Default)]
    struct btrfs_ioctl_search_key {
        pub tree_id: u64,
        pub min_objectid: u64,
        pub max_objectid: u64,
        pub min_offset: u64,
        pub max_offset: u64,
        pub min_transid: u64,
        pub max_transid: u64,
        pub min_type: u32,
        pub max_type: u32,
        pub nr_items: u32,
        pub unused: u32,
        pub unused1: u64,
        pub unused2: u64,
        pub unused3: u64,
        pub unused4: u64,
    }

    const SEARCH_BUF_SIZE: usize = 65536;

    #[repr(C)]
    #[derive(Debug)]
    struct btrfs_ioctl_search_args_v2 {
        pub key: btrfs_ioctl_search_key,
        pub buf_size: u64,
        pub buf: [u8; SEARCH_BUF_SIZE],
    }

    const BTRFS_EXTENT_DATA_KEY: u32 = 108;

    let mut args = btrfs_ioctl_search_args_v2 {
        key: btrfs_ioctl_search_key {
            tree_id: 0,
            min_objectid: ino,
            max_objectid: ino,
            min_offset: 0,
            max_offset: u64::MAX,
            min_transid: 0,
            max_transid: u64::MAX,
            min_type: BTRFS_EXTENT_DATA_KEY,
            max_type: BTRFS_EXTENT_DATA_KEY,
            nr_items: u32::MAX,
            ..Default::default()
        },
        buf_size: SEARCH_BUF_SIZE as u64,
        buf: [0; SEARCH_BUF_SIZE],
    };

    const BTRFS_IOC_TREE_SEARCH_V2: u64 = 3228603409;

    let mut total_size = 0;
    loop {
        if unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                BTRFS_IOC_TREE_SEARCH_V2,
                &mut args as *mut _ as *mut u8,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }

        if args.key.nr_items == 0 {
            return Ok(total_size);
        }

        #[repr(C)]
        #[derive(Debug, Copy, Clone)]
        pub struct btrfs_ioctl_search_header {
            pub transid: u64,
            pub objectid: u64,
            pub offset: u64,
            pub type_: u32,
            pub len: u32,
        }

        let mut buf = args.buf.as_ptr();
        for _ in 0..args.key.nr_items {
            let item: btrfs_ioctl_search_header =
                unsafe { ptr::read_unaligned(buf as *const btrfs_ioctl_search_header) };

            buf = unsafe { buf.add(size_of::<btrfs_ioctl_search_header>()) };

            // Update min_offset for next query.
            args.key.min_offset = item.offset + 1;

            #[repr(C, packed)]
            #[derive(Debug, Copy, Clone)]
            pub struct btrfs_file_extent_item {
                pub generation: [u8; 8],
                pub ram_bytes: [u8; 8],
                pub compression: u8,
                pub encryption: u8,
                pub other_encoding: [u8; 2],
                pub type_: u8,
                pub disk_bytenr: [u8; 8],
                pub disk_num_bytes: [u8; 8],
                pub offset: [u8; 8],
                pub num_bytes: [u8; 8],
            }

            const BTRFS_FILE_EXTENT_INLINE: u8 = 0;

            let extent: btrfs_file_extent_item =
                unsafe { ptr::read_unaligned(buf as *const btrfs_file_extent_item) };

            buf = unsafe { buf.add(item.len as usize) };

            if extent.type_ == BTRFS_FILE_EXTENT_INLINE {
                // File data is stored inline in the extent. Data length is item.len minus the
                // header size up through type_.
                total_size +=
                    item.len as u64 - offset_of!(btrfs_file_extent_item, disk_bytenr) as u64;
            } else {
                total_size += u64::from_le_bytes(extent.disk_num_bytes);
            }
        }

        // Optimization from compsize.c. There are no short reads, so if we got a small amount of
        // extents (relative to our buffer size), we can be sure there will be zero items on the
        // next call (so we can avoid making another call).
        if args.key.nr_items < 512 {
            return Ok(total_size);
        }

        // Reset for next query.
        args.key.nr_items = u32::MAX;
    }
}

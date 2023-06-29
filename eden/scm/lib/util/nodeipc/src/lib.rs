/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! NDJSON (new line delimited json) IPC over a duplex file descriptor
//! that is compatible with nodejs child process IPC.
//!
//! Note this library is meant to be simple and intentionally avoids complex
//! parts of the nodejs IPC, including:
//! - The "advanced" serialization [1]. It is implemented in V8's C++ code
//!   and does not have a Rust equivalent.
//! - The ability to send file descriptor around. This is complicated [2]
//!   and not yet interesting for Sapling's use-cases.
//!
//! [1]: https://github.com/nodejs/node/blob/fe514bf960ca1243b71657af662e7df29f5b57cf/lib/internal/child_process/serialization.js#L54
//! [2]: https://github.com/nodejs/node/commit/db6253f94a7e499b2bacf5998a246c7cd06f7245

pub mod derive;
pub(crate) mod nodeipc;
mod sendfd;
pub(crate) mod singleton;

/// Shortcut for the `define_ipc!` macro.
pub use nodeipc_derive::ipc;

pub use self::nodeipc::NodeIpc;
pub use self::singleton::get_singleton;

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod transaction;

pub use transaction::MultiRepoBookmarksTransaction;
pub use transaction::MultiRepoBookmarksTransactionResult;
#[doc(hidden)]
pub use transaction::retry_commit_loop;

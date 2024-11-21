/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::ContentId;
use mononoke_types::FileContents;
pub mod dummy;

pub trait ModernSyncSender {
    fn upload_content(&self, content_id: ContentId, _blob: FileContents);
}

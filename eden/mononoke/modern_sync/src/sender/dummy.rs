/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::ContentId;
use mononoke_types::FileContents;
use slog::info;
use slog::Logger;

use crate::sender::ModernSyncSender;

#[derive(Clone)]
pub struct DummySender {
    logger: Logger,
}

impl DummySender {
    pub fn new(logger: Logger) -> Self {
        Self { logger }
    }
}

impl ModernSyncSender for DummySender {
    fn upload_content(&self, content_id: ContentId, _blob: FileContents) {
        info!(&self.logger, "Uploading content with id: {:?}", content_id)
    }
}

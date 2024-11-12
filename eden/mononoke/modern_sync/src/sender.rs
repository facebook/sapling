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

pub struct ModernSyncSender {
    logger: Logger,
}

impl ModernSyncSender {
    pub fn new(logger: Logger) -> Self {
        Self { logger }
    }

    pub fn upload_content(&self, content_id: ContentId, _blob: FileContents) {
        info!(&self.logger, "Uploading content with id: {:?}", content_id)
    }
}

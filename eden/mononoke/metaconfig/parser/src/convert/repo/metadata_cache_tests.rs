/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use metaconfig_types::MetadataCacheConfig;
use metaconfig_types::MetadataCacheUpdateMode;
use mononoke_macros::mononoke;
use repos::RawMetadataCacheConfig;
use repos::RawMetadataCacheUpdateMode;
use repos::RawPolling;
use repos::RawTailing;

use crate::convert::Convert;

#[mononoke::test]
fn absent_batching_preserves_processing_modes() {
    for processing in [None, Some(false), Some(true)] {
        let parsed = RawMetadataCacheConfig {
            wbc_process_bookmark_notifications: processing,
            ..Default::default()
        }
        .convert()
        .unwrap();
        assert_eq!(
            parsed,
            MetadataCacheConfig {
                wbc_process_bookmark_notifications: processing.unwrap_or(false),
                ..Default::default()
            }
        );
    }
}

#[mononoke::test]
fn false_batching_preserves_processing_modes() {
    for processing in [None, Some(false), Some(true)] {
        let parsed = RawMetadataCacheConfig {
            wbc_process_bookmark_notifications: processing,
            wbc_event_router_batching_enabled: Some(false),
            ..Default::default()
        }
        .convert()
        .unwrap();
        assert_eq!(
            parsed,
            MetadataCacheConfig {
                wbc_process_bookmark_notifications: processing.unwrap_or(false),
                ..Default::default()
            }
        );
    }
}

#[mononoke::test]
fn true_batching_with_processing_enabled() {
    let parsed = RawMetadataCacheConfig {
        wbc_process_bookmark_notifications: Some(true),
        wbc_event_router_batching_enabled: Some(true),
        ..Default::default()
    }
    .convert()
    .unwrap();
    assert_eq!(
        parsed,
        MetadataCacheConfig {
            wbc_process_bookmark_notifications: true,
            wbc_event_router_batching_enabled: true,
            ..Default::default()
        }
    );
}

#[mononoke::test]
fn true_batching_rejects_processing_unset() {
    let error = RawMetadataCacheConfig {
        wbc_event_router_batching_enabled: Some(true),
        ..Default::default()
    }
    .convert()
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "wbc_event_router_batching_enabled requires wbc_process_bookmark_notifications=true"
    );
}

#[mononoke::test]
fn true_batching_rejects_processing_false() {
    let error = RawMetadataCacheConfig {
        wbc_process_bookmark_notifications: Some(false),
        wbc_event_router_batching_enabled: Some(true),
        ..Default::default()
    }
    .convert()
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "wbc_event_router_batching_enabled requires wbc_process_bookmark_notifications=true"
    );
}

#[mononoke::test]
fn batching_preserves_existing_metadata() {
    for batching in [None, Some(false), Some(true)] {
        let parsed = RawMetadataCacheConfig {
            wbc_update_mode: Some(RawMetadataCacheUpdateMode::tailing(RawTailing {
                category: "legacy_bookmarks".to_string(),
                ..Default::default()
            })),
            wbc_event_router_update_mode: Some(RawMetadataCacheUpdateMode::tailing(RawTailing {
                category: "event_router_bookmarks".to_string(),
                ..Default::default()
            })),
            wbc_process_bookmark_notifications: Some(true),
            wbc_event_router_batching_enabled: batching,
            tags_update_mode: Some(RawMetadataCacheUpdateMode::polling(RawPolling::default())),
            content_refs_update_mode: Some(RawMetadataCacheUpdateMode::tailing(RawTailing {
                category: "content_refs".to_string(),
                ..Default::default()
            })),
            ..Default::default()
        }
        .convert()
        .unwrap();
        assert_eq!(
            parsed,
            MetadataCacheConfig {
                wbc_update_mode: Some(MetadataCacheUpdateMode::Tailing {
                    category: "legacy_bookmarks".to_string(),
                }),
                wbc_event_router_update_mode: Some(MetadataCacheUpdateMode::Tailing {
                    category: "event_router_bookmarks".to_string(),
                }),
                wbc_process_bookmark_notifications: true,
                wbc_event_router_batching_enabled: batching.unwrap_or(false),
                tags_update_mode: Some(MetadataCacheUpdateMode::Polling),
                content_refs_update_mode: Some(MetadataCacheUpdateMode::Tailing {
                    category: "content_refs".to_string(),
                }),
            }
        );
    }
}

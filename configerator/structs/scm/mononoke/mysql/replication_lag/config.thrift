// @generated SignedSource<<ba13011680625a0ed1cf963da7fe53ba>>
// DO NOT EDIT THIS FILE MANUALLY!
// This file is a mechanical copy of the version in the configerator repo. To
// modify it, edit the copy in the configerator repo instead and copy it over by
// running the following in your fbcode directory:
//
// configerator-thrift-updater scm/mononoke/mysql/replication_lag/config.thrift
/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "thrift/annotation/rust.thrift"

@rust.Exhaustive
struct ReplicationLagTableConfig {
  1: i32 max_replication_lag_allowed_ms;
  2: i32 poll_interval_ms = 2000;
}

@rust.Exhaustive
struct ReplicationLagBlobstoreConfig {
  1: optional ReplicationLagTableConfig sync_queue;
  2: optional ReplicationLagTableConfig xdb_blobstore;
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Struct representing the raw packfile item for base objects in Git
struct GitPackfileBaseItem {
  1: binary id;
  2: i64 decompressed_size;
  3: binary compressed_data;
  4: GitObjectKind kind;
} (rust.exhaustive)

/// Enum determining the type of Git base object
enum GitObjectKind {
  Tree = 0,
  Blob = 1,
  Commit = 2,
  Tag = 3,
} (rust.exhaustive)

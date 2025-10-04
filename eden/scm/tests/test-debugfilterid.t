
#require no-eden

# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# plain

  $ setconfig experimental.use-filter-storage=true
  $ hg init

# Legacy Filters
  $ setconfig experimental.filter-version=Legacy

# Single path, Legacy Filters
  $ hg debugfilterid --rev 38723ede98cf632e4f029acb8c0a166dcc6f8eee filter/file1
  filter/file1:38723ede98cf632e4f029acb8c0a166dcc6f8eee (no-eol)

# Multiple paths, Legacy Filters
  $ hg debugfilterid --rev 38723ede98cf632e4f029acb8c0a166dcc6f8eee filter/file1 filter/file2
  abort: V1 filters are disabled, but multiple filter paths are specified
  [255]

# V1 Filters
  $ setconfig experimental.filter-version=V1

# Single path, V1 Filters
  $ hg debugfilterid --rev 38723ede98cf632e4f029acb8c0a166dcc6f8eee filter/file1 | python -c 'import sys; print(sys.stdin.buffer.read())'
  b'\x01\x01\x08v\xf6\xa7\xfb\xec5\xe1`'

# Multiple paths, V1 Filters
  $ hg debugfilterid --rev 38723ede98cf632e4f029acb8c0a166dcc6f8eee filter/file1 filter/file2 | python -c 'import sys; print(sys.stdin.buffer.read())'
  b'\x01\x01\x08"l\xc2\xc2\x1b\x86\xe8\xda'

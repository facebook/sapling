#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#require no-icasefs

  $ newrepo
  $ mkdir -p dirA/SUBDIRA dirA/subdirA dirB dirA/mixed DIRB
  $ touch dirA/SUBDIRA/file1 dirA/subdirA/file2 dirA/mixed/file3 dirA/Mixed dirA/MIXED dirB/file4 dirB/FILE4 DIRB/File4
  $ hg commit -Aqm base

# Check for all collisions

  $ hg debugexistingcasecollisions
  <root> contains collisions: DIRB, dirB
  "dirA" contains collisions: MIXED, Mixed, mixed
  "dirA" contains collisions: SUBDIRA, subdirA
  "dirB" contains collisions: FILE4, file4

# Check for collisions in a directory

  $ hg debugexistingcasecollisions dirB
  "dirB" contains collisions: FILE4, file4

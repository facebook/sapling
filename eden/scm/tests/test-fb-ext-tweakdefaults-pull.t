#require no-eden

# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ eagerepo
  $ enable tweakdefaults
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig commands.update.check=noconflict
  $ setconfig tweakdefaults.defaultdest=remote/main

setup server and client

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -Aqm a
  $ hg book main
  $ newclientrepo b ~/a
  $ hg pull -u -B main
  pulling from $TESTTMP/a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

test a local modification

  $ echo aa > a
  $ hg pull -u -B main
  pulling from $TESTTMP/a
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  M a

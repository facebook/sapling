#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > preventpremegarepoupdateshook=
  > EOF

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -A -m pre_megarepo_commit
  adding a
  $ mkdir .megarepo
  $ touch .megarepo/remapping_state
  $ hg commit -A -m megarepo_merge
  adding .megarepo/remapping_state
  $ touch b
  $ hg commit -A -m another_commit
  adding b

  $ hg goto .^
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo n | hg goto --config ui.interactive=true .^
  Checking out commits from before megarepo merge is discouraged. The resulting checkout will contain just the contents of one git subrepo. Many tools might not work as expected. Do you want to continue (Yn)?   n
  abort: preupdate.preventpremegarepoupdates hook failed
  [255]

  $ hg goto --config ui.interactive=false .^
  Checking out commits from before megarepo merge is discouraged. The resulting checkout will contain just the contents of one git subrepo. Many tools might not work as expected. Do you want to continue (Yn)?   y
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg goto tip^
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ HGPLAIN=1 hg goto --config ui.interactive=true .^
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

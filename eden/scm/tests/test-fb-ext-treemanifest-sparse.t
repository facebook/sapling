#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# test interaction between sparse and treemanifest (sparse file listing)

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > sparse=
  > treemanifest=
  > [treemanifest]
  > treeonly = True
  > [remotefilelog]
  > reponame = master
  > cachepath = $PWD/hgcache
  > EOF

# Setup the repository

  $ hg init myrepo
  $ cd myrepo
  $ touch show
  $ touch hide
  $ mkdir -p subdir/foo/spam subdir/bar/ham hiddensub/foo hiddensub/bar
  $ touch subdir/foo/spam/show
  $ touch subdir/bar/ham/hide
  $ touch hiddensub/foo/spam
  $ touch hiddensub/bar/ham
  $ hg add .
  adding hiddensub/bar/ham
  adding hiddensub/foo/spam
  adding hide
  adding show
  adding subdir/bar/ham/hide
  adding subdir/foo/spam/show
  $ hg commit -m Init
  $ hg sparse include show
  $ hg sparse exclude hide
  $ hg sparse include subdir
  $ hg sparse exclude subdir/foo

# Test cwd

  $ hg sparse cwd
  - hiddensub
  - hide
    show
    subdir
  $ cd subdir
  $ hg sparse cwd
    bar
  - foo
  $ hg sparse include foo
  $ hg sparse cwd
    bar
    foo

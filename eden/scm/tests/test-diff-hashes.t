#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ configure modernclient
  $ newclientrepo

  $ hg diff inexistent1 inexistent2
  inexistent1: * (glob)
  inexistent2: * (glob)

  $ drawdag <<EOS
  > B  # B/foo = foobar\n
  > |
  > A  # A/foo = bar\n
  >    # drawdag.defaultfiles=false
  > EOS

  $ hg --quiet diff -r $A -r $B
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg diff -r $A -r $B
  diff -r ad359e6ee61c -r 18cc8665bedf foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg --verbose diff -r $A -r $B
  diff -r ad359e6ee61c -r 18cc8665bedf foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg --debug diff -r $A -r $B
  diff -r ad359e6ee61c347b0f1e4cda50d401a2c3e5a137 -r 18cc8665bedf4f832b2ca4d3f73e4b6095826c89 foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

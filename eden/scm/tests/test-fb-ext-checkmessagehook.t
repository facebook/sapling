#debugruntest-compatible

#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Build up a repo

  $ eagerepo
  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -A -l "$TESTDIR/ctrlchar-msg.txt"
  adding a
  abort: non-printable characters in commit message:
      This is a commit with bad chars in the message - but this one is OK
      
      That was a blank line
    > This has a sneaky ctrl-A: 
                                ^
    > And this has esc: 
                        ^
      
      Finish off with an OK line
  (edit commit message to fix this issue)
  [255]
  $ hg commit -A -l "$TESTDIR/perfectlyok-msg.txt"
  adding a
  $ hg log -r .
  commit:      d9cf9881be7b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     This commit message is perfectly OK, and has no sneaky control characters.

# Try force adding a non-printable character

  $ touch b
  $ hg commit -A -l "$TESTDIR/ctrlchar-msg.txt" --config commit.allow-non-printable=True
  adding b

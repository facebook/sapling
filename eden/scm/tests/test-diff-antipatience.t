#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Test case that makes use of the weakness of patience diff algorithm

  $ hg init repo
  $ cd repo

  >>> with open("a", "wb") as f:
  ...     f.write("\n".join(list("a" + "x" * 10 + "u" + "x" * 30 + "a\n")).encode()) and None

  $ hg commit -m 1 -A a

  >>> with open("a", "wb") as f:
  ...     f.write("\n".join(list("b" + "x" * 30 + "u" + "x" * 10 + "b\n")).encode()) and None

  $ hg diff
  diff -r f0aeecb49805 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,4 +1,4 @@
  -a
  +b
   x
   x
   x
  @@ -9,7 +9,6 @@
   x
   x
   x
  -u
   x
   x
   x
  @@ -30,6 +29,7 @@
   x
   x
   x
  +u
   x
   x
   x
  @@ -40,5 +40,5 @@
   x
   x
   x
  -a
  +b
   

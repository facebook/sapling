# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

Setup
  $ source "$TESTDIR/setup.sh"

Generate test files
  $ cat << EOF > a
  > a
  > b
  > c
  > d
  > e
  > f
  > EOF

  $ cat << EOF > b
  > a
  > x
  > c
  > d
  > f
  > g
  > EOF

  $ echo -n > empty
  $ echo -ne "a\0" > binary_a
  $ echo -ne "b\0" > binary_b


Basic diff test
  $ xdiff a b
  --- a/a
  +++ b/b
  @@ -1,6 +1,6 @@
   a
  -b
  +x
   c
   d
  -e
   f
  +g

Test with empty file
  $ xdiff a empty
  --- a/a
  +++ b/empty
  @@ -1,6 +1,1 @@
  -a
  -b
  -c
  -d
  -e
  -f

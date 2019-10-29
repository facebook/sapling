# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

Setup
  $ . "$TESTDIR/setup.sh"

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

  $ printf "" > empty
  $ printf "a\0" > binary_a
  $ printf "b\0" > binary_b
  $ cp a a_exec
  $ chmod +x a_exec


Basic diff test
  $ xdiff a b
  diff --git a/a b/b
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
  diff --git a/a b/empty
  --- a/a
  +++ b/empty
  @@ -1,6 +1,1 @@
  -a
  -b
  -c
  -d
  -e
  -f

Test with non-existent file
  $ xdiff a non-existent
  diff --git a/a b/a
  deleted file mode 100644
  --- a/a
  +++ /dev/null
  @@ -1,6 +0,0 @@
  -a
  -b
  -c
  -d
  -e
  -f

Test with executable file
  $ xdiff b a_exec
  diff --git a/b b/a_exec
  old mode 100644
  new mode 100755
  --- a/b
  +++ b/a_exec
  @@ -1,6 +1,6 @@
   a
  -x
  +b
   c
   d
  +e
   f
  -g

Test copy
  $ xdiff --copy a b -U 0
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -2,1 +2,1 @@
  -b
  +x
  @@ -5,1 +5,0 @@
  -e
  @@ -7,0 +6,1 @@
  +g

Test move
  $ xdiff --move b a -U 1
  diff --git a/b b/a
  rename from b
  rename to a
  --- a/b
  +++ b/a
  @@ -1,6 +1,6 @@
   a
  -x
  +b
   c
   d
  +e
   f
  -g

Test with binary file
  $ xdiff a binary_a
  diff --git a/a b/binary_a
  Binary files a/a and b/binary_a differ

  $ xdiff binary_a binary_b
  diff --git a/binary_a b/binary_b
  Binary files a/binary_a and b/binary_b differ

  $ xdiff binary_b non-existent
  diff --git a/binary_b b/binary_b
  deleted file mode 100644
  Binary file binary_b has changed

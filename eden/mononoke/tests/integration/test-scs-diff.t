# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup config repo:
  $ setup_common_config
  $ cd "$TESTTMP"

Setup testing repo for mononoke:
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server

Helper for making commit:
  $ function commit() { # the arg is used both for commit message and variable name
  >   hg commit -qAm $1 # create commit
  >   export COMMIT_$1="$(hg --debug id -i)" # save hash to variable
  > }

First two simple commits and bookmark:
  $ echo -e "a\nb\nc\nd\ne" > a
  $ commit A

  $ echo -e "a\nb\nd\ne\nf" > b
  $ commit B
  $ hg bookmark -i BOOKMARK_B

A commit with a file change and binary file
  $ echo -e "b\nc\nd\ne\nf" > b
  $ echo -e "\0 10" > binary
  $ commit C

A commit with file move and copy
  $ hg update -q $COMMIT_B
  $ hg move a moved_a
  $ echo x >> moved_a
  $ hg cp b copied_b
  $ commit D

A commit that adds things in two different subdirectories
  $ mkdir dir_a dir_b
  $ hg move moved_a dir_a/a
  $ echo x >> dir_a/a
  $ echo y > dir_b/y
  $ hg add dir_b/y
  $ commit E

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start SCS server
  $ start_and_wait_for_scs_server --scuba-log-file "$TESTTMP/scuba.json"

diff paths only with bonsai id

  $ scsc diff --repo repo --hg-commit-id "$COMMIT_A" --hg-commit-id "$COMMIT_B"
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,5 @@
  +a
  +b
  +d
  +e
  +f
  $ scsc diff --repo repo --hg-commit-id "$COMMIT_A" --hg-commit-id "$COMMIT_B" --placeholders-only
  diff --git a/b b/b
  new file mode 100644
  Binary file b has changed

  $ scsc diff --repo repo -B BOOKMARK_B -i "$COMMIT_C"
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,5 +1,5 @@
  -a
   b
  +c
   d
   e
   f
  diff --git a/binary b/binary
  new file mode 100644
  Binary file binary has changed

  $ scsc diff --repo repo --paths-only -B BOOKMARK_B -i "$COMMIT_C"
  M b
  A binary

  $ scsc diff --repo repo -i "$COMMIT_B" -i "$COMMIT_D"
  diff --git a/b b/copied_b
  copy from b
  copy to copied_b
  diff --git a/a b/moved_a
  rename from a
  rename to moved_a
  --- a/a
  +++ b/moved_a
  @@ -3,3 +3,4 @@
   c
   d
   e
  +x

paths-only mode

  $ scsc diff --repo repo --paths-only -i "$COMMIT_B" -i "$COMMIT_D"
  C b -> copied_b
  R a -> moved_a

  $ scsc diff --repo repo --paths-only -i "$COMMIT_D" -i "$COMMIT_E"
  A dir_b/y
  R moved_a -> dir_a/a

with bonsai
  $ scsc lookup --repo repo -i "$COMMIT_C" -S bonsai
  d5ded5e738f4fc36b03c3e09db9cdd9259d167352a03fb6130f5ee138b52972f

  $ scsc diff --repo repo --paths-only -B BOOKMARK_B --bonsai-id "d5ded5e738f4fc36b03c3e09db9cdd9259d167352a03fb6130f5ee138b52972f"
  M b
  A binary

test filtering paths in diff

  $ scsc diff --repo repo --paths-only -B BOOKMARK_B -i "$COMMIT_C" -p binary
  A binary

  $ scsc diff --repo repo --paths-only -B BOOKMARK_B -i "$COMMIT_C" -p x/y

  $ scsc diff --repo repo --paths-only -i "$COMMIT_D" -i "$COMMIT_E" --path dir_a/
  R moved_a -> dir_a/a

  $ scsc diff --repo repo -i "$COMMIT_D" -i "$COMMIT_E" --path dir_a/a
  diff --git a/moved_a b/dir_a/a
  rename from moved_a
  rename to dir_a/a
  --- a/moved_a
  +++ b/dir_a/a
  @@ -4,3 +4,4 @@
   d
   e
   x
  +x

  $ scsc diff --repo repo --paths-only -i "$COMMIT_D" -i "$COMMIT_E" --path dir_b/
  A dir_b/y

  $ scsc diff --repo repo -i "$COMMIT_B"
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,5 @@
  +a
  +b
  +d
  +e
  +f

  $ scsc diff --repo repo -i "$COMMIT_B" -i "$COMMIT_D" --skip-copies-renames
  diff --git a/a b/a
  deleted file mode 100644
  --- a/a
  +++ /dev/null
  @@ -1,5 +0,0 @@
  -a
  -b
  -c
  -d
  -e
  diff --git a/copied_b b/copied_b
  new file mode 100644
  --- /dev/null
  +++ b/copied_b
  @@ -0,0 +1,5 @@
  +a
  +b
  +d
  +e
  +f
  diff --git a/moved_a b/moved_a
  new file mode 100644
  --- /dev/null
  +++ b/moved_a
  @@ -0,0 +1,6 @@
  +a
  +b
  +c
  +d
  +e
  +x

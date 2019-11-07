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
  >   hg commit -Am $1 # create commit
  >   declare $COMMIT_$1="$(hg --debug id -i)" # save hash to variable
  > }

First two simple commits and bookmark:
  $ echo -e "a\nb\nc\nd\ne" > a
  $ commit A
  adding a

  $ echo -e "a\nb\nd\ne\nf" > b
  $ commit B
  adding b

  $ hg bookmark BOOKMARK_B

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start SCS server
  $ start_and_wait_for_scs_server

repos
  $ scsc repos
  repo

lookup
  $ scsc lookup --repo repo  -B BOOKMARK_B
  323afe77a1b1e632e54e8d5a683ba2cc8511f299

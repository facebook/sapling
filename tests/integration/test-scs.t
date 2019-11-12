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
  >   export COMMIT_$1="$(hg --debug id -i)" # save hash to variable
  > }

First two simple commits and bookmark:
  $ echo -e "a\nb\nc\nd\ne" > a
  $ commit A
  adding a

  $ echo -e "a\nb\nd\ne\nf" > b
  $ commit B
  adding b

  $ hg bookmark -i BOOKMARK_B

A commit with a file change and binary file

  $ echo -e "b\nc\nd\ne\nf" > b
  $ echo -e "\0 10" > binary
  $ commit C
  adding binary


import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

try talking to the server before it is up
  $ SCS_PORT=$(get_free_socket) scsc lookup --repo repo  -B BOOKMARK_B
  error: apache::thrift::transport::TTransportException: AsyncSocketException: connect failed, type = Socket not open, errno = 111 (Connection refused): Connection refused
  [1]

start SCS server
  $ start_and_wait_for_scs_server

repos
  $ scsc repos
  repo

lookup
  $ scsc lookup --repo repo  -B BOOKMARK_B
  323afe77a1b1e632e54e8d5a683ba2cc8511f299

diff
  $ scsc diff --repo repo -B BOOKMARK_B -i $COMMIT_C
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

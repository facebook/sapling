# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

An invalid prefix in a suffix query ('..' is not a valid repo path) is a
client error: the server answers 400 Bad Request instead of panicking.

FIXME: BUG! Today the handler unwraps MPath::try_from on each client prefix,
so the request panics inside the server and the connection is torn down. This
test pins the current behaviour; the next diff fixes it.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config

  $ testtool_drawdag -R repo --print-hg-hashes --no-default-files << EOF
  > A
  > # modify: A "foo.txt" "one\n"
  > # bookmark: A master_bookmark
  > EOF
  A=* (glob)
  $ start_and_wait_for_mononoke_server

  $ hg clone -q mono:repo repo
  $ cd repo
  $ hg up -q "$A"

A valid query works:
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i None
  [{"file_path": "foo.txt"}]

FIXME: BUG! An invalid prefix should be rejected with 400 Bad Request; today
the handler panics, the client sees a transport error and the server process
dies.
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i "['..']"
  * error: * (glob)
  
  (is your client certificate valid?)
  [255]

FIXME: BUG! The server should still be healthy afterwards; today it is gone.
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i None
  abort: [7] Couldn't connect to server * (glob)
  [255]

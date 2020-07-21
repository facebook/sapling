# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup config repo:
  $ setup_common_config
  $ setup_configerator_configs
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

Three simple commits:
  $ echo -e "a\nb\nc\nd\ne" > a
  $ commit A

  $ echo -e "a\nb\nd\ne\nf" > b
  $ commit B

  $ echo -e "b\nc\nd\ne\nf" > b
  $ commit C

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start SCS server
  $ start_and_wait_for_scs_server --scuba-log-file "$TESTTMP/scuba.json"

  $ scsc blame --repo repo -i "$COMMIT_C" --path b
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: b
  482568559f8a410956c5c4be57f322c117f16733: c
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: d
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: e
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: f

  $ scsc blame --repo repo -i "$COMMIT_C" --parent --path b
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: a
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: b
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: d
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: e
  323afe77a1b1e632e54e8d5a683ba2cc8511f299: f

  $ scsc --json blame --repo repo -i "$COMMIT_C" --path b | jq -S .
  [
    {
      "author": "test",
      "commit": {
        "bonsai": "c63b71178d240f05632379cf7345e139fe5d4eb1deca50b3e23c26115493bbbb",
        "hg": "323afe77a1b1e632e54e8d5a683ba2cc8511f299"
      },
      "contents": "b",
      "datetime": "1970-01-01T00:00:00+00:00",
      "line": 1,
      "origin_line": 2,
      "path": "b"
    },
    {
      "author": "test",
      "commit": {
        "bonsai": "a3b89780ba11c5ed7d6f0ef48eb18e624bf60c16d84bf0f4416dd78c8e225ca8",
        "hg": "482568559f8a410956c5c4be57f322c117f16733"
      },
      "contents": "c",
      "datetime": "1970-01-01T00:00:00+00:00",
      "line": 2,
      "origin_line": 2,
      "path": "b"
    },
    {
      "author": "test",
      "commit": {
        "bonsai": "c63b71178d240f05632379cf7345e139fe5d4eb1deca50b3e23c26115493bbbb",
        "hg": "323afe77a1b1e632e54e8d5a683ba2cc8511f299"
      },
      "contents": "d",
      "datetime": "1970-01-01T00:00:00+00:00",
      "line": 3,
      "origin_line": 3,
      "path": "b"
    },
    {
      "author": "test",
      "commit": {
        "bonsai": "c63b71178d240f05632379cf7345e139fe5d4eb1deca50b3e23c26115493bbbb",
        "hg": "323afe77a1b1e632e54e8d5a683ba2cc8511f299"
      },
      "contents": "e",
      "datetime": "1970-01-01T00:00:00+00:00",
      "line": 4,
      "origin_line": 4,
      "path": "b"
    },
    {
      "author": "test",
      "commit": {
        "bonsai": "c63b71178d240f05632379cf7345e139fe5d4eb1deca50b3e23c26115493bbbb",
        "hg": "323afe77a1b1e632e54e8d5a683ba2cc8511f299"
      },
      "contents": "f",
      "datetime": "1970-01-01T00:00:00+00:00",
      "line": 5,
      "origin_line": 5,
      "path": "b"
    }
  ]

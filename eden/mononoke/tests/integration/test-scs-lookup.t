# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup config repo:
  $ POPULATE_GIT_MAPPING=1 setup_common_config
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

Commit with globalrev:
  $ touch c
  $ hg add
  adding c
  $ hg commit -Am "commit with globalrev" --extra global_rev=9999999999
  $ hg bookmark -i BOOKMARK_C

Commit git SHA:
  $ touch d
  $ hg add
  adding d
  $ hg commit -Am "commit with git sha" --extra convert_revision=37b0a167e07f2b84149c918cec818ffeb183dddd --extra hg-git-rename-source=git
  $ hg bookmark -i BOOKMARK_D

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
  $ blobimport repo-hg/.hg repo --has-globalrev

start SCS server
  $ start_and_wait_for_scs_server --scuba-log-file "$TESTTMP/scuba.json"

lookup using bookmark
  $ scsc lookup --repo repo -B BOOKMARK_C -S bonsai
  006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b

lookup, commit with git
  $ scsc lookup --repo repo  -B BOOKMARK_D -S bonsai,hg,globalrev,git
  bonsai=227d4402516061c45a7ba66cf4561bdadaf3ac96eb12c6e75aa9c72dbabd42b6
  git=37b0a167e07f2b84149c918cec818ffeb183dddd
  hg=6e602c2eaa591b482602f5f3389de6c2749516d5

lookup, commit with globalrev
  $ scsc lookup --repo repo -B BOOKMARK_C -S bonsai,hg,globalrev,git
  bonsai=006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b
  globalrev=9999999999
  hg=ee87eb8cfeb218e7352a94689b241ea973b80402

lookup using bonsai to identify commit
  $ scsc lookup --repo repo --bonsai-id 006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b -S bonsai,hg,globalrev
  bonsai=006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b
  globalrev=9999999999
  hg=ee87eb8cfeb218e7352a94689b241ea973b80402

lookup using globalrev to identify commit
  $ scsc lookup --repo repo --globalrev 9999999999 -S bonsai,hg,globalrev
  bonsai=006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b
  globalrev=9999999999
  hg=ee87eb8cfeb218e7352a94689b241ea973b80402

lookup using git to identify commit
  $ scsc lookup --repo repo --git 37b0a167e07f2b84149c918cec818ffeb183dddd -S bonsai,hg,globalrev
  bonsai=227d4402516061c45a7ba66cf4561bdadaf3ac96eb12c6e75aa9c72dbabd42b6
  hg=6e602c2eaa591b482602f5f3389de6c2749516d5

lookup using hg to identify commit
  $ scsc lookup --repo repo --hg-commit-id ee87eb8cfeb218e7352a94689b241ea973b80402 -S bonsai,hg,globalrev
  bonsai=006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b
  globalrev=9999999999
  hg=ee87eb8cfeb218e7352a94689b241ea973b80402

lookup using bonsai needed resolving to identify commit
  $ scsc lookup --repo repo -i 006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b -S bonsai,hg,globalrev
  bonsai=006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b
  globalrev=9999999999
  hg=ee87eb8cfeb218e7352a94689b241ea973b80402

lookup using bonsai prefix needed resolving to identify commit
  $ scsc lookup --repo repo -i 006c988c4a9f60080a6bc2 -S bonsai,hg,globalrev
  bonsai=006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b
  globalrev=9999999999
  hg=ee87eb8cfeb218e7352a94689b241ea973b80402

lookup using globalrev needed resolving to identify commit
  $ scsc lookup --repo repo -i 9999999999 -S bonsai,hg,globalrev
  bonsai=006c988c4a9f60080a6bc2a2fff47565fafea2ca5b16c4d994aecdef0c89973b
  globalrev=9999999999
  hg=ee87eb8cfeb218e7352a94689b241ea973b80402

lookup using hg needed resolving to identify commit
  $ scsc lookup --repo repo -i "$COMMIT_E" -S bonsai,hg,globalrev
  bonsai=29c11c4d7a26279ad9a90edac504ac6599c0b62cb55455fbed0b7abe125086bb
  hg=82d5da62960d05281995c370fd083299ff66ba16

lookup using hg prefix needed resolving to identify commit
  $ scsc lookup --repo repo -i 82d5da6 -S bonsai,hg,globalrev
  bonsai=29c11c4d7a26279ad9a90edac504ac6599c0b62cb55455fbed0b7abe125086bb
  hg=82d5da62960d05281995c370fd083299ff66ba16

lookup using hg prefix needed resolving to identify commit (ambiguous case)
  $ scsc lookup --repo repo -i 8 -S bonsai,hg,globalrev
  note: several hg commits with the prefix '8' exist
  error: commit not found: 8
  [1]

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup

  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 setup_common_config "blob_files"
  $ cat >> repos/repo/server.toml << EOF
  > [[bookmarks]]
  > regex=".*"
  > [[bookmarks]]
  > name = "date-rewrite"
  > rewrite_dates = true
  > [[bookmarks]]
  > name = "no-date-rewrite"
  > rewrite_dates = false
  > [[bookmarks]]
  > name = "use-repo-config"
  > [[bookmarks]]
  > regex="..*"
  > EOF
  $ cd $TESTTMP

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > pushrebase =
  > remotenames=
  > EOF

Prepare the server-side repo

  $ hginit_treemanifest repo
  $ cd repo
  $ hg debugdrawdag <<EOF
  > B
  > |
  > A
  > EOF

- Create two bookmarks, one with rewritedate enabled, one disabled

  $ hg bookmark date-rewrite -r B
  $ hg bookmark no-date-rewrite -r B
  $ hg bookmark use-repo-config -r B

- Import and start Mononoke (the Mononoke repo name is 'repo')

  $ cd $TESTTMP
  $ blobimport repo/.hg repo
  $ start_and_wait_for_mononoke_server
Prepare the client-side repo

  $ hg clone -q mono:repo client-repo --noupdate
  $ cd $TESTTMP/client-repo
  $ hg debugdrawdag <<'EOS'
  > E C D
  >  \|/
  >   A
  > EOS

Push

  $ hg push -r C --to date-rewrite -q
  $ hg push -r D --to no-date-rewrite -q
  $ hg push -r E --to use-repo-config -q

Check result

  $ hg log -r 'desc(A)+desc(B)::' -G -T '{desc} {date}'
  o  E 0.00
  │
  │ o  D 0.00
  ├─╯
  │ o  C * (glob)
  ├─╯
  o  B 0.00
  │
  o  A 0.00
  

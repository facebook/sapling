# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup

  $ setconfig push.edenapi=true
  $ setup_common_config "blob_files"
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
  > EOF

Prepare the server-side repo

  $ quiet testtool_drawdag -R repo <<EOF
  > B
  > |
  > A
  > # bookmark: B date-rewrite
  > # bookmark: B no-date-rewrite
  > # bookmark: B use-repo-config
  > EOF
  $ start_and_wait_for_mononoke_server
Prepare the client-side repo

  $ hg clone -q mono:repo client-repo --noupdate
  $ cd $TESTTMP/client-repo
  $ hg pull -r date-rewrite -q
  $ A=$(hg log -r 'desc(A)' -T '{node}')
  $ hg debugdrawdag << EOS
  > E C D
  >  \|/
  >   $A
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
  

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

This test checks that when the client has "main branch" config different
from the server. Lazy changelog still behaves sane regarding on pull, or
commit<->location translation.

  $ . "${TEST_FIXTURES}/library.sh"

Set up server repo with 2 branches:

  $ configure modern
  $ setup_common_config

  $ start_and_wait_for_mononoke_server

  $ newrepo repo-server
  $ setconfig paths.default=mononoke://$(mononoke_address)/repo

  $ drawdag << 'EOS'
  >   P
  >   :
  > H I
  > :/
  > D
  > :
  > A
  > EOS

  $ hg push -q -r $D --to branch1 --create
  $ hg push -q -r $D --to master --create

Build segmented changelog up to common branch D:

  $ quiet segmented_changelog_tailer_reseed --repo-name=repo --head=master
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > enabled=true
  > heads_to_include = [
  >    { bookmark = "master" },
  > ]
  > CONFIG

  $ start_and_wait_for_mononoke_server

Prepare client repo with two branches (master, branch1) up to D:

  $ hgedenapi clone -U "mononoke://$(mononoke_address)/repo" "$TESTTMP/repo-client"
  fetching lazy changelog
  populating main commit graph
  tip commit: f585351a92f85104bff7c284233c338b10eb1df7
  fetching selected remote bookmarks
  $ cd "$TESTTMP/repo-client"
  $ hgedenapi pull -qB branch1
  $ hgedenapi log -r 'max(all())' -T '{remotenames} {desc}\n'
  remote/branch1 remote/master D

  $ cp -R "$TESTTMP/repo-client" "$TESTTMP/repo-client2"

Move master and branch1 to different branches:

  $ cd "$TESTTMP/repo-server"
  $ hg push -qr $H --to master
  $ hg push -qr $P --to branch1
  $ hg log -Gr "$D+$H+$P" -T '{desc} {node}'
  o  P 686c5883d796a5b917b3067c1e48a599361f8a35
  ╷
  ╷ o  H a31451c3c1debad52cf22ef2aebfc88c75dc899a
  ╭─╯
  o  D f585351a92f85104bff7c284233c338b10eb1df7
  │
  ~

Client changes main branch to branch1, then do a pull.
Because the server does not have segments on branch1, fastpath cannot be used:

  $ cd "$TESTTMP/repo-client"
  $ setconfig remotenames.selectivepulldefault=branch1
  $ hgedenapi pull 2>&1 | grep -v '^   '
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  failed to get fast pull data * (glob)
  * using fallback path (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes

I..P are pulled via non-lazy fallback pull path. They can be resolved locally:

  $ LOG=dag::protocol=debug hgedenapi log -Gr $J+$K -T "{desc}\n"
  o  K
  │
  o  J
  │
  ~

  $ LOG=dag::protocol=debug hgedenapi log -Gr "$P~3" -T "{desc}\n"
  o  M
  │
  ~

Allow server to build up temporary segments on demand:

  $ merge_tunables <<'EOS'
  > {"ints": {"segmented_changelog_client_max_commits_to_traverse": 100}}
  > EOS

Pulling branch1 as main branch now uses fastpath:

  $ cd "$TESTTMP/repo-client2"
  $ setconfig remotenames.selectivepulldefault=branch1
  $ hgedenapi pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  imported commit graph for 8 commits (1 segment)
  searching for changes
  adding changesets
  adding manifests
  adding file changes

The pulled commits I..P are lazy. They can be resolved via a new server:

  $ start_and_wait_for_mononoke_server

  $ LOG=dag::protocol=debug hgedenapi log -Gr $J+$K -T "{desc}\n"
  DEBUG dag::protocol: resolve names [adcd9184fd6b2b0dc2fbdeb471ad2b3be7272564] remotely
  DEBUG dag::protocol: resolve names [996d4ef77ff379445b2e8cad7427fa4a472b3f02] remotely
  o  K
  │
  o  J
  │
  ~

  $ LOG=dag::protocol=debug hgedenapi log -Gr "$P~3" -T "{desc}\n"
  DEBUG dag::protocol: resolve ids [8] remotely
  DEBUG dag::protocol: resolve ids [7] remotely
  o  M
  │
  ~

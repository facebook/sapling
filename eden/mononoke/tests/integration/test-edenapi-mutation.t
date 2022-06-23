# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree. 

  $ . "${TEST_FIXTURES}/library.sh"
  $ configure modern
  $ setconfig ui.ignorerevnum=false

Set up local hgrc and Mononoke config, with commit cloud, http pull and upload.
  $ export READ_ONLY_REPO=1
  $ INFINITEPUSH_ALLOW_WRITES=true \
  >   setup_common_config
  $ cd $TESTTMP
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "mutation_advertise_for_infinitepush": true,
  >     "mutation_accept_for_infinitepush": true,
  >     "mutation_generate_for_draft": true
  >   }
  > }
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend =
  > commitcloud =
  > infinitepush =
  > rebase =
  > remotenames =
  > share =
  > [commitcloud]
  > hostname = testhost
  > servicetype = local
  > servicelocation = $TESTTMP
  > token_enforced = False
  > owner_team = The Test Team
  > usehttpupload = True
  > [visibility]
  > enabled = True
  > [mutation]
  > record = True
  > enabled = True
  > date = 0 0
  > [remotefilelog]
  > reponame=repo
  > [pull]
  > httpcommitgraph = true
  > httphashprefix = true
  > EOF
Custom smartlog
  $ function sl {
  >  hgedenapi log -G -T "{node|short} '{desc|firstline}' {join(mutations % '(Rewritten using {operation} into {join(successors % \'{node|short}\', \', \')})', ' ')}" --hidden
  > }

Initialize test repo.
  $ hginit_treemanifest repo
  $ cd repo
  $ mkcommit base_commit
  $ hg log -T '{short(node)}\n'
  8b2dca0c8a72


Import and start mononoke
  $ cd $TESTTMP
  $ hgclone_treemanifest ssh://user@dummy/repo client1 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo client2 --noupdate
  $ blobimport repo/.hg repo
  $ start_and_wait_for_mononoke_server
Test mutations on client 1
  $ cd client1
  $ hgedenapi up 8b2dca0c8a72 -q
  $ hgedenapi cloud join -q
  $ mkcommit A
  $ hg log -T{node} -r .
  929f2b9071cf032d9422b3cce9773cbe1c574822 (no-eol)
  $ hgedenapi cloud upload -q
  $ hgedenapi debugapi -e commitmutations -i '["929f2b9071cf032d9422b3cce9773cbe1c574822"]'
  []
  $ hg metaedit -r . -m new_message
  $ hg log -T{node} -r .
  f643b098cd183f085ba3e6107b6867ca472e87d1 (no-eol)
  $ hgedenapi cloud upload -q
  $ hgedenapi debugapi -e commitmutations -i '["f643b098cd183f085ba3e6107b6867ca472e87d1"]'
  [{"op": "metaedit",
    "tz": 0,
    "time": 0,
    "user": [116,
             101,
             115,
             116],
    "split": [],
    "extras": [],
    "successor": bin("f643b098cd183f085ba3e6107b6867ca472e87d1"),
    "predecessors": [bin("929f2b9071cf032d9422b3cce9773cbe1c574822")]}]
  $ hgedenapi debugapi -e commitmutations -i '["929f2b9071cf032d9422b3cce9773cbe1c574822"]'
  []
  $ hgedenapi debugapi -e commitmutations -i '["f643b098cd183f085ba3e6107b6867ca472e87d1", "929f2b9071cf032d9422b3cce9773cbe1c574822"]'
  [{"op": "metaedit",
    "tz": 0,
    "time": 0,
    "user": [116,
             101,
             115,
             116],
    "split": [],
    "extras": [],
    "successor": bin("f643b098cd183f085ba3e6107b6867ca472e87d1"),
    "predecessors": [bin("929f2b9071cf032d9422b3cce9773cbe1c574822")]}]
  $ sl
  @  f643b098cd18 'new_message'
  │
  │ x  929f2b9071cf 'A' (Rewritten using metaedit into f643b098cd18)
  ├─╯
  o  8b2dca0c8a72 'base_commit'
  
Test how they are propagated to client 2
  $ cd ../client2
  $ hgedenapi debugchangelog --migrate lazy
  $ hgedenapi pull -r f643b098cd18 -q
  $ hgedenapi pull -r 929f2b9071cf -q
  $ sl
  x  929f2b9071cf 'A' (Rewritten using metaedit into f643b098cd18)
  │
  │ o  f643b098cd18 'new_message'
  ├─╯
  o  8b2dca0c8a72 'base_commit'
  

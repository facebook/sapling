# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ setup_configerator_configs
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

Populate test repo
  $ echo "my commit message" > test.txt
  $ hg commit -Aqm "add test.txt"
  $ COMMIT_1=$(hg log -r . -T '{node}')
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ COMMIT_2=$(hg log -r . -T '{node}')
  $ echo "this is the second file" > test2.txt
  $ hg commit -Aqm "update test2.txt"
  $ COMMIT_B1=$(hg log -r . -T '{node}')
  $ hg co -q $COMMIT_2
  $ echo "this is the first file" > test.txt
  $ hg commit -Aqm "update test.txt"
  $ COMMIT_B2=$(hg log -r . -T '{node}')
  $ hg merge -q $COMMIT_B1
  $ hg commit -m "merge commit!!!"
  $ COMMIT_MERGE=$(hg log -r . -T '{node}')
  $ echo "third file" > test3.txt
  $ hg commit -Aqm "add test3.txt"
  $ COMMIT_M1=$(hg log -r . -T '{node}')
  $ hg bookmark "master_bookmark"
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  @  b5bc5249412595662f15a1aca5ae50fec4a93628 add test3.txt
  │
  o    ce33edd793793f108fbe78aa90f3fedbeae09082 merge commit!!!
  ├─╮
  │ o  b6f0fa5a73b54553c0d4b6f483c8ef18efb3bde2 update test.txt
  │ │
  o │  45a08a9d95ee1053cf34273c8a427973d4ffd11a update test2.txt
  ├─╯
  o  c7dcf24fab3a8ab956273fa40d5cc44bc26ec655 copy test.txt to test2.txt
  │
  o  e83645968c8f2954b97a3c79ce5a6b90a464c54d add test.txt
  


Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ SEGMENTED_CHANGELOG_ENABLE=1 setup_mononoke_config
  $ start_and_wait_for_mononoke_server
Prepare request.
  $ cat > req <<EOF
  > [
  >   ("$COMMIT_B1", 1, 2),
  >   ("$COMMIT_B1", 2, 1),
  >   ("$COMMIT_M1", 1, 1),
  > ]
  > EOF

Check files in response.
  $ hgedenapi debugapi -e commitlocationtohash -f req --sort
  [{"count": 2,
    "hgids": [bin("c7dcf24fab3a8ab956273fa40d5cc44bc26ec655"),
              bin("e83645968c8f2954b97a3c79ce5a6b90a464c54d")],
    "location": {"distance": 1,
                 "descendant": bin("45a08a9d95ee1053cf34273c8a427973d4ffd11a")}},
   {"count": 1,
    "hgids": [bin("e83645968c8f2954b97a3c79ce5a6b90a464c54d")],
    "location": {"distance": 2,
                 "descendant": bin("45a08a9d95ee1053cf34273c8a427973d4ffd11a")}},
   {"count": 1,
    "hgids": [bin("ce33edd793793f108fbe78aa90f3fedbeae09082")],
    "location": {"distance": 1,
                 "descendant": bin("b5bc5249412595662f15a1aca5ae50fec4a93628")}}]

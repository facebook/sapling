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
  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo --print-hg-hashes <<EOF
  > COMMIT_M1
  > |
  > COMMIT_MERGE
  > |\
  > | COMMIT_B2
  > | |
  > COMMIT_B1 |
  > |/
  > COMMIT_2
  > |
  > COMMIT_1
  > # modify: COMMIT_1 "test.txt" "my commit message\n"
  > # modify: COMMIT_2 "copy.txt" "my commit message\n"
  > # modify: COMMIT_B1 "test2.txt" "this is the second file\n"
  > # modify: COMMIT_B2 "test.txt" "this is the first file\n"
  > # modify: COMMIT_M1 "test3.txt" "third file\n"
  > # message: COMMIT_1 "add test.txt"
  > # message: COMMIT_2 "copy test.txt to test2.txt"
  > # message: COMMIT_B1 "update test2.txt"
  > # message: COMMIT_B2 "update test.txt"
  > # message: COMMIT_MERGE "merge commit!!!"
  > # message: COMMIT_M1 "add test3.txt"
  > # bookmark: COMMIT_M1 master_bookmark
  > EOF
  COMMIT_1=2c73711572dcc5fba150bc86885bed40a3950176
  COMMIT_2=87d56d8162fe716b567ccf245ad56fa9a90b5069
  COMMIT_B1=356dd8c2d65699b1b43e2be32547e33c80dca639
  COMMIT_B2=b5a17706990db58956e666fe67766032a8938359
  COMMIT_M1=7062f7f9196c60b81ecb10086c3f9049f9379cd7
  COMMIT_MERGE=017843be220beb61aa305bbce56de076f029c7a8


Import test repo.
  $ cd ..

Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
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
  $ hg debugapi mono:repo -e commitlocationtohash -f req --sort
  [{"count": 2,
    "hgids": [bin("87d56d8162fe716b567ccf245ad56fa9a90b5069"),
              bin("2c73711572dcc5fba150bc86885bed40a3950176")],
    "location": {"distance": 1,
                 "descendant": bin("356dd8c2d65699b1b43e2be32547e33c80dca639")}},
   {"count": 1,
    "hgids": [bin("2c73711572dcc5fba150bc86885bed40a3950176")],
    "location": {"distance": 2,
                 "descendant": bin("356dd8c2d65699b1b43e2be32547e33c80dca639")}},
   {"count": 1,
    "hgids": [bin("017843be220beb61aa305bbce56de076f029c7a8")],
    "location": {"distance": 1,
                 "descendant": bin("7062f7f9196c60b81ecb10086c3f9049f9379cd7")}}]

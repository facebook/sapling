# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config

create a repo with some long paths and filenames

  $ cd $TESTTMP
  $ LONG_PATH='very/long/path/one/two/three/four/five/six/seven/eight/nine/ten/eleven/twelve/thirteen/fourteen/fifteen/sixteen/seventeen/eighteen/nineteen/twenty/@special@/ONE/TWO/THREE/FOUR/FIVE/SIX/SEVEN/EIGHT/NINE/TEN/ELEVEN/TWELVE/THIRTEEN/FOURTEEN/FIFTEEN/SIXTEEN'
  $ LONG_FILENAME='very_long_filename_to_test_Mononoke_can_handle_both_long_paths_and_file_names_This_path_name_will_have_two_hundred_and_fifty_three_characters_in_order_to_fully_test_the_limits_of_what_Mononoke_can_handle_and_to_ensure_we_dont_introduce_unexpected_limits'

  $ testtool_drawdag -R repo --no-default-files --derive-all <<EOF
  > A
  > # bookmark: A master_bookmark
  > # message: A long
  > # modify: A "$LONG_PATH/$LONG_FILENAME" "content"
  > EOF
  A=7597083f87f7a184567da47f40aca6c27fce394a39f58f94a36caf65715cbb4c

  $ start_and_wait_for_mononoke_server

clone the repo and check that mercurial can access the file

  $ cd $TESTTMP
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo-hg
  $ cd repo-hg
  $ hgmn log
  commit:      41c590dc2a01
  bookmark:    master_bookmark
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long
  

  $ du "$LONG_PATH/$LONG_FILENAME"
  4	very/long/path/one/two/three/four/five/six/seven/eight/nine/ten/eleven/twelve/thirteen/fourteen/fifteen/sixteen/seventeen/eighteen/nineteen/twenty/@special@/ONE/TWO/THREE/FOUR/FIVE/SIX/SEVEN/EIGHT/NINE/TEN/ELEVEN/TWELVE/THIRTEEN/FOURTEEN/FIFTEEN/SIXTEEN/very_long_filename_to_test_Mononoke_can_handle_both_long_paths_and_file_names_This_path_name_will_have_two_hundred_and_fifty_three_characters_in_order_to_fully_test_the_limits_of_what_Mononoke_can_handle_and_to_ensure_we_dont_introduce_unexpected_limits

push another long path with a large file

  $ mkdir -p "${LONG_PATH}2"
  $ dd if=/dev/zero of="${LONG_PATH}2/${LONG_FILENAME}2" bs=10M count=1
  1+0 records in
  1+0 records out
  10485760 bytes (10 MB, 10 MiB) copied* (glob)
  $ hg add "${LONG_PATH}2/${LONG_FILENAME}2"
  $ hg ci -mlong2
  $ hg log
  commit:      bddcd6316ae7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long2
  
  commit:      41c590dc2a01
  bookmark:    master_bookmark
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long
  
  $ hgmn push --to master_bookmark
  pushing rev bddcd6316ae7 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

  $ cd $TESTTMP
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo-hg2
  $ cd repo-hg2
  $ du "${LONG_PATH}2/${LONG_FILENAME}2"
  10240	very/long/path/one/two/three/four/five/six/seven/eight/nine/ten/eleven/twelve/thirteen/fourteen/fifteen/sixteen/seventeen/eighteen/nineteen/twenty/@special@/ONE/TWO/THREE/FOUR/FIVE/SIX/SEVEN/EIGHT/NINE/TEN/ELEVEN/TWELVE/THIRTEEN/FOURTEEN/FIFTEEN/SIXTEEN2/very_long_filename_to_test_Mononoke_can_handle_both_long_paths_and_file_names_This_path_name_will_have_two_hundred_and_fifty_three_characters_in_order_to_fully_test_the_limits_of_what_Mononoke_can_handle_and_to_ensure_we_dont_introduce_unexpected_limits2


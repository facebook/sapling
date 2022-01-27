# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config

  $ cd $TESTTMP
  $ LONG_PATH='this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/blobimport/as/well/as/mononoke/works/correctly/when/given/such/a/long/path/which/I/hope/will/have/enough/characters/for/the/purpose/of/testing/I/need/few/more/to/go/pass/255/chars'
  $ LONG_FILENAME='this_is_a_very_long_file_name_that_we_want_to_test_in_order_to_ensure_our_blobimport_as_well_as_mononoke_works_correctly_when_given_such_a_long_path_which_I_hope_will_have_enough_characters_for_the_purpose_of_testing_I_need_few_more_to_go_pass_255_chars'

init repo-hg

  $ hginit_treemanifest repo-hg

setup repo2 and repo3

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo3

setup repo-hg

  $ cd repo-hg
  $ mkdir -p ${LONG_PATH}
  $ dd if=/dev/zero of=${LONG_PATH}/${LONG_FILENAME} bs=150M count=1
  1+0 records in
  1+0 records out
  157286400 bytes (157 MB* (glob)
  $ hg add ${LONG_PATH}/${LONG_FILENAME}
  $ hg ci -mlong
  $ hg log
  commit:      b8119d283b73
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long
   (re)

create master bookmark
  $ hg bookmark master_bookmark -r tip

blobimport and start mononoke

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
  $ start_and_wait_for_mononoke_server
pull on repo2

  $ cd $TESTTMP/repo2
  $ hgmn pull --config ui.disable-stream-clone=true
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  warning: stream clone is disabled
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  adding remote bookmark master_bookmark
  $ hgmn log
  commit:      b8119d283b73
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long
   (re)
  $ hgmn update -r master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ du ${LONG_PATH}/${LONG_FILENAME}
  153600	this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/blobimport/as/well/as/mononoke/works/correctly/when/given/such/a/long/path/which/I/hope/will/have/enough/characters/for/the/purpose/of/testing/I/need/few/more/to/go/pass/255/chars/this_is_a_very_long_file_name_that_we_want_to_test_in_order_to_ensure_our_blobimport_as_well_as_mononoke_works_correctly_when_given_such_a_long_path_which_I_hope_will_have_enough_characters_for_the_purpose_of_testing_I_need_few_more_to_go_pass_255_chars

push one more long path from repo2

  $ mkdir -p ${LONG_PATH}2
  $ dd if=/dev/zero of=${LONG_PATH}2/${LONG_FILENAME}2 bs=151M count=1
  1+0 records in
  1+0 records out
  158334976 bytes (158 MB* (glob)
  $ hg add ${LONG_PATH}2/${LONG_FILENAME}2
  $ hg ci -mlong2
  $ hg log
  commit:      8fffbbe6af55
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long2
   (re)
  commit:      b8119d283b73
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long
   (re)
  $ hgmn push
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  updating bookmark master_bookmark

pull on repo3

  $ cd $TESTTMP/repo3
  $ hgmn pull --config ui.disable-stream-clone=true
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  warning: stream clone is disabled
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  adding remote bookmark master_bookmark
  $ hgmn log
  commit:      8fffbbe6af55
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long2
   (re)
  commit:      b8119d283b73
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long
   (re)
  $ hgmn update -r master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ du ${LONG_PATH}/${LONG_FILENAME}
  153600	this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/blobimport/as/well/as/mononoke/works/correctly/when/given/such/a/long/path/which/I/hope/will/have/enough/characters/for/the/purpose/of/testing/I/need/few/more/to/go/pass/255/chars/this_is_a_very_long_file_name_that_we_want_to_test_in_order_to_ensure_our_blobimport_as_well_as_mononoke_works_correctly_when_given_such_a_long_path_which_I_hope_will_have_enough_characters_for_the_purpose_of_testing_I_need_few_more_to_go_pass_255_chars
  $ du ${LONG_PATH}2/${LONG_FILENAME}2
  154624	this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/blobimport/as/well/as/mononoke/works/correctly/when/given/such/a/long/path/which/I/hope/will/have/enough/characters/for/the/purpose/of/testing/I/need/few/more/to/go/pass/255/chars2/this_is_a_very_long_file_name_that_we_want_to_test_in_order_to_ensure_our_blobimport_as_well_as_mononoke_works_correctly_when_given_such_a_long_path_which_I_hope_will_have_enough_characters_for_the_purpose_of_testing_I_need_few_more_to_go_pass_255_chars2

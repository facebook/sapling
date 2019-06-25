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
  157286400 bytes (157 MB) copied, * (glob)
  $ hg add ${LONG_PATH}/${LONG_FILENAME}
  this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/blobimport/as/well/as/mononoke/works/correctly/when/given/such/a/long/path/which/I/hope/will/have/enough/characters/for/the/purpose/of/testing/I/need/few/more/to/go/pass/255/chars/this_is_a_very_long_file_name_that_we_want_to_test_in_order_to_ensure_our_blobimport_as_well_as_mononoke_works_correctly_when_given_such_a_long_path_which_I_hope_will_have_enough_characters_for_the_purpose_of_testing_I_need_few_more_to_go_pass_255_chars: up to 471 MB of RAM may be required to manage this file
  (use 'hg revert this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/blobimport/as/well/as/mononoke/works/correctly/when/given/such/a/long/path/which/I/hope/will/have/enough/characters/for/the/purpose/of/testing/I/need/few/more/to/go/pass/255/chars/this_is_a_very_long_file_name_that_we_want_to_test_in_order_to_ensure_our_blobimport_as_well_as_mononoke_works_correctly_when_given_such_a_long_path_which_I_hope_will_have_enough_characters_for_the_purpose_of_testing_I_need_few_more_to_go_pass_255_chars' to cancel the pending addition)
  $ hg ci -mlong
  $ hg log
  changeset:   0:b8119d283b73
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long
   (re)

create master bookmark
  $ hg bookmark master_bookmark -r tip

check that the file created had both the content and path large enough to
create a hashed index and data revlogs

  $ du .hg/store/dh/this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/this_i75ebb3f31bf65e471c16ebbef3bc32a326d92ae6.i
  4	.hg/store/dh/this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/this_i75ebb3f31bf65e471c16ebbef3bc32a326d92ae6.i
  $ du .hg/store/dh/this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/this_i4680ea2a5b12ad9620ef7e598dcae10adf62b11c.d
  (152|156)	.hg/store/dh/this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/this_i4680ea2a5b12ad9620ef7e598dcae10adf62b11c.d (re)

blobimport and start mononoke

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

pull on repo2

  $ cd $TESTTMP/repo2
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  warning: stream clone requested but client is missing requirements: lz4revlog
  (see https://www.mercurial-scm.org/wiki/MissingRequirement for more information)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  adding remote bookmark master_bookmark
  new changesets b8119d283b73
  $ hgmn log
  changeset:   0:b8119d283b73
  bookmark:    master_bookmark
  tag:         tip
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
  158334976 bytes (158 MB) copied, * (glob)
  $ hg add ${LONG_PATH}2/${LONG_FILENAME}2
  this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/blobimport/as/well/as/mononoke/works/correctly/when/given/such/a/long/path/which/I/hope/will/have/enough/characters/for/the/purpose/of/testing/I/need/few/more/to/go/pass/255/chars2/this_is_a_very_long_file_name_that_we_want_to_test_in_order_to_ensure_our_blobimport_as_well_as_mononoke_works_correctly_when_given_such_a_long_path_which_I_hope_will_have_enough_characters_for_the_purpose_of_testing_I_need_few_more_to_go_pass_255_chars2: up to 475 MB of RAM may be required to manage this file
  (use 'hg revert this/is/a/very/long/path/that/we/want/to/test/in/order/to/ensure/our/blobimport/as/well/as/mononoke/works/correctly/when/given/such/a/long/path/which/I/hope/will/have/enough/characters/for/the/purpose/of/testing/I/need/few/more/to/go/pass/255/chars2/this_is_a_very_long_file_name_that_we_want_to_test_in_order_to_ensure_our_blobimport_as_well_as_mononoke_works_correctly_when_given_such_a_long_path_which_I_hope_will_have_enough_characters_for_the_purpose_of_testing_I_need_few_more_to_go_pass_255_chars2' to cancel the pending addition)
  $ hg ci -mlong2
  $ hg log
  changeset:   1:8fffbbe6af55
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long2
   (re)
  changeset:   0:b8119d283b73
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long
   (re)
  $ hgmn push
  pushing to ssh://user@dummy/repo
  searching for changes
  updating bookmark master_bookmark

pull on repo3

  $ cd $TESTTMP/repo3
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  warning: stream clone requested but client is missing requirements: lz4revlog
  (see https://www.mercurial-scm.org/wiki/MissingRequirement for more information)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  adding remote bookmark master_bookmark
  new changesets b8119d283b73:8fffbbe6af55
  $ hgmn log
  changeset:   1:8fffbbe6af55
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     long2
   (re)
  changeset:   0:b8119d283b73
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

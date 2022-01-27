# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'

verify content
  $ hg log
  commit:      0e7ec5675652
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

setup push source repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2

start mononoke

  $ merge_tunables <<EOF
  > {
  >   "ints": {
  >     "unbundle_limit_num_of_commits_in_push": 2
  >   }
  > }
  > EOF
  $ start_and_wait_for_mononoke_server
create new commit in repo2 and check that push fails

  $ cd repo2
  $ echo "1" >> a
  $ hg addremove
  $ hg ci -ma

  $ hgmn push mononoke://$(mononoke_address)/repo -r . --to master_bookmark --config extensions.remotenames=
  pushing rev 2b761f0782ab to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark


  $ echo "1" >> a
  $ hg ci -maa
  $ echo "1" >> a
  $ hg ci -maaa
  $ echo "1" >> a
  $ hg ci -maaaa
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --to master_bookmark --config extensions.remotenames=
  pushing rev 3a090ff5a2b7 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote: 
  remote:   Root cause:
  remote:     Trying to push too many commits! Limit is 2, tried to push 3
  remote: 
  remote:   Caused by:
  remote:     While resolving Changegroup
  remote:   Caused by:
  remote:     Trying to push too many commits! Limit is 2, tried to push 3
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "bundle2_resolver error",
  remote:         source: Error {
  remote:             context: "While resolving Changegroup",
  remote:             source: "Trying to push too many commits! Limit is 2, tried to push 3",
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

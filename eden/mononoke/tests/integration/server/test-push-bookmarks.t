# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export ONLY_FAST_FORWARD_BOOKMARK="master_bookmark"
  $ export ONLY_FAST_FORWARD_BOOKMARK_REGEX="ffonly.*"
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

start mononoke
  $ start_and_wait_for_mononoke_server

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hg clone -q mono:repo repo-push
  $ hg clone -q mono:repo repo-pull


Push with bookmark
  $ cd repo-push
  $ echo withbook > withbook && hg addremove && hg ci -m withbook
  adding withbook
  $ hg push --to withbook --create
  pushing rev cdbb2b8b2cf1 to destination mono:repo bookmark withbook
  searching for changes
  exporting bookmark withbook

Pull the bookmark
  $ cd ../repo-pull

  $ hg pull -q
  $ hg book --remote
     remote/master_bookmark           20ca2a4749a439b459125ef0f6a4f26e88ee7538
     remote/withbook                  cdbb2b8b2cf1612cd6a1271c96a7a89d98b36dd4

Update the bookmark
  $ cd ../repo-push
  $ echo update > update && hg addremove && hg ci -m update
  adding update
  $ hg push --to withbook
  pushing rev 31b9c167eeea to destination mono:repo bookmark withbook
  searching for changes
  updating bookmark withbook
  $ cd ../repo-pull
  $ hg pull -q
  $ hg book --remote
     remote/master_bookmark           20ca2a4749a439b459125ef0f6a4f26e88ee7538
     remote/withbook                  31b9c167eeeaeb53634df68ea168918a7395bed7

Try non fastforward moves (backwards and across branches)
  $ cd ../repo-push
  $ hg update -q master_bookmark
  $ echo other_commit > other_commit && hg -q addremove && hg ci -m other_commit
  $ hg push --to master_bookmark
  pushing rev 638df78bae5c to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ hg push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 20ca2a4749a4 --to master_bookmark
  pushing rev 20ca2a4749a4 to destination mono:repo bookmark master_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:     Caused by:
  remote:         0: Failed to move bookmark
  remote:         1: Non fast-forward bookmark move of 'master_bookmark' from 9b9805995990bb9a787f5290e75bd7926146098df1f2ce3420e91063d41789b7 to aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hg push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 31b9c167eeea --to master_bookmark
  pushing rev 31b9c167eeea to destination mono:repo bookmark master_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:     Caused by:
  remote:         0: Failed to move bookmark
  remote:         1: Non fast-forward bookmark move of 'master_bookmark' from 9b9805995990bb9a787f5290e75bd7926146098df1f2ce3420e91063d41789b7 to 99853792aa9e4c9ab4519940c25bd2c840dd7af70f1b2f8aaf5e52beec5fc372
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hg push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 20ca2a4749a4 --to withbook
  pushing rev 20ca2a4749a4 to destination mono:repo bookmark withbook
  searching for changes
  no changes found
  updating bookmark withbook
  $ cd ../repo-pull
  $ hg pull -q
  $ hg book --remote
     remote/master_bookmark           638df78bae5c6ebbe95ab00886b7b15c9ee143ee
     remote/withbook                  20ca2a4749a439b459125ef0f6a4f26e88ee7538

Try non fastfoward moves on regex bookmark
  $ hg push -r 638df78bae5c --to ffonly_bookmark --create -q
  $ hg push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 20ca2a4749a4 --to ffonly_bookmark
  pushing rev 20ca2a4749a4 to destination mono:repo bookmark ffonly_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:     Caused by:
  remote:         0: Failed to move bookmark
  remote:         1: Non fast-forward bookmark move of 'ffonly_bookmark' from 9b9805995990bb9a787f5290e75bd7926146098df1f2ce3420e91063d41789b7 to aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  abort: unexpected EOL, expected netstring digit
  [255]

Try to delete master
  $ cd ../repo-push
  $ hg push --delete master_bookmark
  pushing to mono:repo
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:     Caused by:
  remote:         0: Failed to delete bookmark
  remote:         1: Deletion of 'master_bookmark' is prohibited
  abort: unexpected EOL, expected netstring digit
  [255]

Delete the bookmark
  $ hg push --delete withbook
  pushing to mono:repo
  searching for changes
  no changes found
  deleting remote bookmark withbook
  [1]
  $ cd ../repo-pull
  $ hg pull -q
  $ hg book --remote
     remote/ffonly_bookmark           638df78bae5c6ebbe95ab00886b7b15c9ee143ee
     remote/master_bookmark           638df78bae5c6ebbe95ab00886b7b15c9ee143ee

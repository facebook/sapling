# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ setup_configerator_configs
  $ cd "$TESTTMP"

Setup testing repo for mononoke:
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server

  $ drawdag << EOS
  > E F H I  L M
  > |/  |/   |/
  > D   G    K N
  > |   |    |/
  > C   C    J    P  A2
  > |        |    |  |
  > B        B    O  A1
  > |             |  |
  > Z             Z  Z
  > EOS

  $ hg book -r $A1 alpha1
  $ hg book -r $A2 alpha2
  $ hg book -r $E echo
  $ hg book -r $F foxtrot
  $ hg book -r $G golf
  $ hg book -r $H hotel
  $ hg book -r $I indigo
  $ hg book -r $L lima
  $ hg book -r $M mike
  $ hg book -r $N november
  $ hg book -r $P papa

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start SCS server
  $ start_and_wait_for_scs_server

list bookmarks

  $ scsc list-bookmarks -R repo
  alpha1                                   1f85d318726e7b67003a6721a0f7d52e4a65df56
  alpha2                                   4ea53c9703999b7cfc95f65cc08bedbb85256db5
  echo                                     a66f31238ae926c5e077a4074e5d2d7cc89c7642
  foxtrot                                  ad17e2b35d3ca4b4ecaf9b97f0e12b25b7ad5240
  golf                                     e51fbd5c6698cb7f247157201bad895ac0fbe351
  hotel                                    c8b41630521932eb2296498044c8ca7ed915a7c7
  indigo                                   1ca7d2194f493e124f18a02bb14c28283a7c4ae9
  lima                                     260ba4730b0a61a8bdd00c5df448666e5858b2c6
  mike                                     d05b4d81f9d17a1070d55bc0b00465a1a800dba2
  november                                 aeb51b067aeb9df8fee564f957816f4f4a28e876
  papa                                     2d9743c121bd7585b327ad13e338ef33b66aa550

list bookmarks with pagination

  $ scsc list-bookmarks -R repo --limit 5
  alpha1                                   1f85d318726e7b67003a6721a0f7d52e4a65df56
  alpha2                                   4ea53c9703999b7cfc95f65cc08bedbb85256db5
  echo                                     a66f31238ae926c5e077a4074e5d2d7cc89c7642
  foxtrot                                  ad17e2b35d3ca4b4ecaf9b97f0e12b25b7ad5240
  golf                                     e51fbd5c6698cb7f247157201bad895ac0fbe351

  $ scsc list-bookmarks -R repo --limit 5 --after golf
  hotel                                    c8b41630521932eb2296498044c8ca7ed915a7c7
  indigo                                   1ca7d2194f493e124f18a02bb14c28283a7c4ae9
  lima                                     260ba4730b0a61a8bdd00c5df448666e5858b2c6
  mike                                     d05b4d81f9d17a1070d55bc0b00465a1a800dba2
  november                                 aeb51b067aeb9df8fee564f957816f4f4a28e876

  $ scsc list-bookmarks -R repo --limit 5 --after november
  papa                                     2d9743c121bd7585b327ad13e338ef33b66aa550

list bookmarks with prefix

  $ scsc list-bookmarks -R repo --prefix alpha
  alpha1                                   1f85d318726e7b67003a6721a0f7d52e4a65df56
  alpha2                                   4ea53c9703999b7cfc95f65cc08bedbb85256db5

list bookmarks with multiple commit identity schemes

  $ scsc list-bookmarks -R repo --prefix alpha -S bonsai,hg
  alpha1:
      bonsai=870751fd5240976367b6f01f85a287fa61e47e42866c85f408d9e6d5ab71c1fb
      hg=1f85d318726e7b67003a6721a0f7d52e4a65df56
  alpha2:
      bonsai=8e49a87534d39850c165dadeb7a0b8db7ce2c60d3695a83c2b482d18ccd46ccb
      hg=4ea53c9703999b7cfc95f65cc08bedbb85256db5

list descendant bookmarks

  $ scsc list-bookmarks -R repo -i $C
  echo                                     a66f31238ae926c5e077a4074e5d2d7cc89c7642
  foxtrot                                  ad17e2b35d3ca4b4ecaf9b97f0e12b25b7ad5240
  golf                                     e51fbd5c6698cb7f247157201bad895ac0fbe351
  hotel                                    c8b41630521932eb2296498044c8ca7ed915a7c7
  indigo                                   1ca7d2194f493e124f18a02bb14c28283a7c4ae9

  $ scsc list-bookmarks -R repo -B golf
  golf                                     e51fbd5c6698cb7f247157201bad895ac0fbe351
  hotel                                    c8b41630521932eb2296498044c8ca7ed915a7c7
  indigo                                   1ca7d2194f493e124f18a02bb14c28283a7c4ae9

list descendant bookmarks with pagination

  $ scsc list-bookmarks -R repo -i $B --limit 5
  echo                                     a66f31238ae926c5e077a4074e5d2d7cc89c7642
  foxtrot                                  ad17e2b35d3ca4b4ecaf9b97f0e12b25b7ad5240
  golf                                     e51fbd5c6698cb7f247157201bad895ac0fbe351
  hotel                                    c8b41630521932eb2296498044c8ca7ed915a7c7
  indigo                                   1ca7d2194f493e124f18a02bb14c28283a7c4ae9

  $ scsc list-bookmarks -R repo -i $B --limit 5 --after indigo
  lima                                     260ba4730b0a61a8bdd00c5df448666e5858b2c6
  mike                                     d05b4d81f9d17a1070d55bc0b00465a1a800dba2
  november                                 aeb51b067aeb9df8fee564f957816f4f4a28e876

list descendant bookmarks with prefix

  $ scsc list-bookmarks -R repo --prefix alpha -i $Z
  alpha1                                   1f85d318726e7b67003a6721a0f7d52e4a65df56
  alpha2                                   4ea53c9703999b7cfc95f65cc08bedbb85256db5

add a couple of scratch bookmarks

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo --noupdate
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > infinitepush=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hg checkout -q $H
  $ echo scratch > scratch
  $ hg commit -Aqm "scratch commit"

  $ mononoke
  $ wait_for_mononoke
  $ hgmn push ssh://user@dummy/repo -r . --to scratch/hotel1 --create
  pushing to ssh://user@dummy/repo
  searching for changes
  $ hgmn push ssh://user@dummy/repo -r . --to scratch/hotel2 --create
  pushing to ssh://user@dummy/repo
  searching for changes

  $ hg checkout -q $I
  $ echo scratch > scratch
  $ hg commit -Aqm "scratch commit 2"
  $ hgmn push ssh://user@dummy/repo -r . --to scratch/indigo1 --create
  pushing to ssh://user@dummy/repo
  searching for changes

normal lists don't include them

  $ scsc list-bookmarks -R repo --prefix scratch

add --include-scratch to list the scratch bookmarks

  $ scsc list-bookmarks -R repo --include-scratch --prefix scratch
  scratch/hotel1                           07ad55e1876f2daa716e4026515262afc1abc4b8
  scratch/hotel2                           07ad55e1876f2daa716e4026515262afc1abc4b8
  scratch/indigo1                          d4108cc5870616e9dfeb000dc9781af8f9c09d3d

this works for descendant requests, too

  $ scsc list-bookmarks -R repo --include-scratch --prefix scratch -B hotel
  scratch/hotel1                           07ad55e1876f2daa716e4026515262afc1abc4b8
  scratch/hotel2                           07ad55e1876f2daa716e4026515262afc1abc4b8

the --include-scratch option requires the prefix

  $ scsc list-bookmarks -R repo --include-scratch
  error: SourceControlService::repo_list_bookmarks failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "prefix required to list scratch bookmarks" }
  [1]

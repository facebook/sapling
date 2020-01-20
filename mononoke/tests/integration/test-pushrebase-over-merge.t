# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'


Try to push merge commit
  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg up -q 0
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ hg merge -q -r 3 && hg ci -m "merge 1 and 2"
  $ log -r ":"
  @    merge 1 and 2 [draft;rev=5;3e1c4ca1f9be]
  |\
  | o  2 [draft;rev=4;c9b2673d3218]
  | |
  o |  1 [draft;rev=3;a0c9c5791058]
  |/
  | o  C [public;rev=2;26805aba1e60] default/master_bookmark
  | |
  | o  B [public;rev=1;112478962961]
  |/
  o  A [public;rev=0;426bada5c675]
  $

  $ hgmn push -r . --to master_bookmark -q

Now try to push over a merge commit
  $ hgmn up -q 0
  $ echo 'somefile' > somefile
  $ hg add somefile
  $ hg ci -m 'pushrebase over merge'
  $ hgmn push -r . --to master_bookmark -q
  $ hg log -r master_bookmark
  changeset:   10:c8a34708eb3a
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  parent:      8:2a9ef460b971
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushrebase over merge
  * (glob)

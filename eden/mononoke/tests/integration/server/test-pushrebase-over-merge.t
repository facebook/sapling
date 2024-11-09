# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false
  $ setconfig push.edenapi=true
  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2


Try to push merge commit
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg up -q "min(all())"
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ hg merge -q -r 26f143b427a3 && hg ci -m "merge 1 and 2"
  $ log -r ":"
  @    merge 1 and 2 [draft;rev=281474976710658;540b69c58d33]
  ├─╮
  │ o  2 [draft;rev=281474976710657;d9fe1d08ff73]
  │ │
  o │  1 [draft;rev=281474976710656;26f143b427a3]
  ├─╯
  │ o  C [public;rev=2;d3b399ca8757] remote/master_bookmark
  │ │
  │ o  B [public;rev=1;80521a640a0c]
  ├─╯
  o  A [public;rev=0;20ca2a4749a4]
  $

  $ hg push -r . --to master_bookmark -q

Now try to push over a merge commit
  $ hg up -q 0
  $ echo 'somefile' > somefile
  $ hg add somefile
  $ hg ci -m 'pushrebase over merge'
  $ hg push -r . --to master_bookmark -q
  $ hg log -r master_bookmark
  commit:      a652c7e3bce5
  bookmark:    remote/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushrebase over merge
  * (glob)

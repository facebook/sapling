  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > graphlog=
  > [phases]
  > # public changeset are not obsolete
  > publish=false
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "add $1"
  > }
  $ getid() {
  >    hg id --debug --hidden -ir "desc('$1')"
  > }

  $ cat > debugkeys.py <<EOF
  > def reposetup(ui, repo):
  >     class debugkeysrepo(repo.__class__):
  >         def listkeys(self, namespace):
  >             ui.write('listkeys %s\n' % (namespace,))
  >             return super(debugkeysrepo, self).listkeys(namespace)
  > 
  >     if repo.local():
  >         repo.__class__ = debugkeysrepo
  > EOF

  $ hg init tmpa
  $ cd tmpa
  $ mkcommit kill_me

Checking that the feature is properly disabled

  $ hg debugobsolete -d '0 0' `getid kill_me` -u babar
  abort: obsolete feature is not enabled on this repo
  [255]

Enabling it

  $ cat > ../obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH

Killing a single changeset without replacement

  $ hg debugobsolete 0
  abort: changeset references must be full hexadecimal node identifiers
  [255]
  $ hg debugobsolete '00'
  abort: changeset references must be full hexadecimal node identifiers
  [255]
  $ hg debugobsolete -d '0 0' `getid kill_me` -u babar
  $ hg debugobsolete
  97b7c2d76b1845ed3eb988cd612611e72406cef0 0 {'date': '0 0', 'user': 'babar'}

(test that mercurial is not confused)

  $ hg up null --quiet # having 0 as parent prevents it to be hidden
  $ hg tip
  changeset:   -1:000000000000
  tag:         tip
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  $ hg up --hidden tip --quiet
  $ cd ..

Killing a single changeset with replacement

  $ hg init tmpb
  $ cd tmpb
  $ mkcommit a
  $ mkcommit b
  $ mkcommit original_c
  $ hg up "desc('b')"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit new_c
  created new head
  $ hg log -r 'hidden()' --template '{rev}:{node|short} {desc}\n' --hidden
  $ hg debugobsolete --flag 12 `getid original_c`  `getid new_c` -d '56 12'
  $ hg log -r 'hidden()' --template '{rev}:{node|short} {desc}\n' --hidden
  2:245bde4270cd add original_c
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}

do it again (it read the obsstore before adding new changeset)

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit new_2_c
  created new head
  $ hg debugobsolete -d '1337 0' `getid new_c` `getid new_2_c`
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}

Register two markers with a missing node

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit new_3_c
  created new head
  $ hg debugobsolete -d '1338 0' `getid new_2_c` 1337133713371337133713371337133713371337
  $ hg debugobsolete -d '1339 0' 1337133713371337133713371337133713371337 `getid new_3_c`
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 {'date': '1338 0', 'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 {'date': '1339 0', 'user': 'test'}

Refuse pathological nullid successors
  $ hg debugobsolete -d '9001 0' 1337133713371337133713371337133713371337 0000000000000000000000000000000000000000
  transaction abort!
  rollback completed
  abort: bad obsolescence marker detected: invalid successors nullid
  [255]

Check that graphlog detect that a changeset is obsolete:

  $ hg glog
  @  changeset:   5:5601fb93a350
  |  tag:         tip
  |  parent:      1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add new_3_c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  

check that heads does not report them

  $ hg heads
  changeset:   5:5601fb93a350
  tag:         tip
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add new_3_c
  
  $ hg heads --hidden
  changeset:   5:5601fb93a350
  tag:         tip
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add new_3_c
  
  changeset:   4:ca819180edb9
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add new_2_c
  
  changeset:   3:cdbce2fbb163
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add new_c
  
  changeset:   2:245bde4270cd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add original_c
  


check that summary does not report them

  $ hg init ../sink
  $ echo '[paths]' >> .hg/hgrc
  $ echo 'default=../sink' >> .hg/hgrc
  $ hg summary --remote
  parent: 5:5601fb93a350 tip
   add new_3_c
  branch: default
  commit: (clean)
  update: (current)
  remote: 3 outgoing

  $ hg summary --remote --hidden
  parent: 5:5601fb93a350 tip
   add new_3_c
  branch: default
  commit: (clean)
  update: 3 new changesets, 4 branch heads (merge)
  remote: 3 outgoing

check that various commands work well with filtering

  $ hg tip
  changeset:   5:5601fb93a350
  tag:         tip
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add new_3_c
  
  $ hg log -r 6
  abort: unknown revision '6'!
  [255]
  $ hg log -r 4
  abort: unknown revision '4'!
  [255]

Check that public changeset are not accounted as obsolete:

  $ hg --hidden phase --public 2
  $ hg --config 'extensions.graphlog=' glog
  @  changeset:   5:5601fb93a350
  |  tag:         tip
  |  parent:      1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add new_3_c
  |
  | o  changeset:   2:245bde4270cd
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add original_c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  

And that bumped changeset are detected
--------------------------------------

If we didn't filtered obsolete changesets out, 3 and 4 would show up too. Also
note that the bumped changeset (5:5601fb93a350) is not a direct successor of
the public changeset

  $ hg log --hidden -r 'bumped()'
  changeset:   5:5601fb93a350
  tag:         tip
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add new_3_c
  

And that we can't push bumped changeset

  $ hg push ../tmpa -r 0 --force #(make repo related)
  pushing to ../tmpa
  searching for changes
  warning: repository is unrelated
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  $ hg push ../tmpa
  pushing to ../tmpa
  searching for changes
  abort: push includes bumped changeset: 5601fb93a350!
  [255]

Fixing "bumped" situation
We need to create a clone of 5 and add a special marker with a flag

  $ hg up '5^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg revert -ar 5
  adding new_3_c
  $ hg ci -m 'add n3w_3_c'
  created new head
  $ hg debugobsolete -d '1338 0' --flags 1 `getid new_3_c` `getid n3w_3_c`
  $ hg log -r 'bumped()'
  $ hg log -G
  @  changeset:   6:6f9641995072
  |  tag:         tip
  |  parent:      1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add n3w_3_c
  |
  | o  changeset:   2:245bde4270cd
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add original_c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  



  $ cd ..

Exchange Test
============================

Destination repo does not have any data
---------------------------------------

Simple incoming test

  $ hg init tmpc
  $ cd tmpc
  $ hg incoming ../tmpb
  comparing with ../tmpb
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  changeset:   1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add b
  
  changeset:   2:245bde4270cd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add original_c
  
  changeset:   6:6f9641995072
  tag:         tip
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add n3w_3_c
  

Try to pull markers
(extinct changeset are excluded but marker are pushed)

  $ hg pull ../tmpb
  pulling from ../tmpb
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 {'date': '1338 0', 'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 {'date': '1339 0', 'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 {'date': '1338 0', 'user': 'test'}

Rollback//Transaction support

  $ hg debugobsolete -d '1340 0' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 {'date': '1338 0', 'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 {'date': '1339 0', 'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 {'date': '1338 0', 'user': 'test'}
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 0 {'date': '1340 0', 'user': 'test'}
  $ hg rollback -n
  repository tip rolled back to revision 3 (undo debugobsolete)
  $ hg rollback
  repository tip rolled back to revision 3 (undo debugobsolete)
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 {'date': '1338 0', 'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 {'date': '1339 0', 'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 {'date': '1338 0', 'user': 'test'}

  $ cd ..

Try to push markers

  $ hg init tmpd
  $ hg -R tmpb push tmpd
  pushing to tmpd
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 4 files (+1 heads)
  $ hg -R tmpd debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 {'date': '1338 0', 'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 {'date': '1339 0', 'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 {'date': '1338 0', 'user': 'test'}

Check obsolete keys are exchanged only if source has an obsolete store

  $ hg init empty
  $ hg --config extensions.debugkeys=debugkeys.py -R empty push tmpd
  pushing to tmpd
  no changes found
  listkeys phases
  listkeys bookmarks
  [1]

clone support
(markers are copied and extinct changesets are included to allow hardlinks)

  $ hg clone tmpb clone-dest
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R clone-dest log -G --hidden
  @  changeset:   6:6f9641995072
  |  tag:         tip
  |  parent:      1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add n3w_3_c
  |
  | x  changeset:   5:5601fb93a350
  |/   parent:      1:7c3bad9141dc
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add new_3_c
  |
  | x  changeset:   4:ca819180edb9
  |/   parent:      1:7c3bad9141dc
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add new_2_c
  |
  | x  changeset:   3:cdbce2fbb163
  |/   parent:      1:7c3bad9141dc
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add new_c
  |
  | o  changeset:   2:245bde4270cd
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add original_c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
  $ hg -R clone-dest debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 {'date': '1338 0', 'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 {'date': '1339 0', 'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 {'date': '1338 0', 'user': 'test'}


Destination repo have existing data
---------------------------------------

On pull

  $ hg init tmpe
  $ cd tmpe
  $ hg debugobsolete -d '1339 0' 2448244824482448244824482448244824482448 1339133913391339133913391339133913391339
  $ hg pull ../tmpb
  pulling from ../tmpb
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg debugobsolete
  2448244824482448244824482448244824482448 1339133913391339133913391339133913391339 0 {'date': '1339 0', 'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 {'date': '1338 0', 'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 {'date': '1339 0', 'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 {'date': '1338 0', 'user': 'test'}


On push

  $ hg push ../tmpc
  pushing to ../tmpc
  searching for changes
  no changes found
  [1]
  $ hg -R ../tmpc debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C {'date': '56 12', 'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 {'date': '1337 0', 'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 {'date': '1338 0', 'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 {'date': '1339 0', 'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 {'date': '1338 0', 'user': 'test'}
  2448244824482448244824482448244824482448 1339133913391339133913391339133913391339 0 {'date': '1339 0', 'user': 'test'}

detect outgoing obsolete and unstable
---------------------------------------


  $ hg glog
  o  changeset:   3:6f9641995072
  |  tag:         tip
  |  parent:      1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add n3w_3_c
  |
  | o  changeset:   2:245bde4270cd
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add original_c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
  $ hg up 'desc("n3w_3_c")'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit original_d
  $ mkcommit original_e
  $ hg debugobsolete `getid original_d` -d '0 0'
  $ hg log -r 'obsolete()'
  changeset:   4:94b33453f93b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add original_d
  
  $ hg glog -r '::unstable()'
  @  changeset:   5:cda648ca50f5
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add original_e
  |
  x  changeset:   4:94b33453f93b
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add original_d
  |
  o  changeset:   3:6f9641995072
  |  parent:      1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add n3w_3_c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  

refuse to push obsolete changeset

  $ hg push ../tmpc/ -r 'desc("original_d")'
  pushing to ../tmpc/
  searching for changes
  abort: push includes obsolete changeset: 94b33453f93b!
  [255]

refuse to push unstable changeset

  $ hg push ../tmpc/
  pushing to ../tmpc/
  searching for changes
  abort: push includes unstable changeset: cda648ca50f5!
  [255]

Test that extinct changeset are properly detected

  $ hg log -r 'extinct()'

Don't try to push extinct changeset

  $ hg init ../tmpf
  $ hg out  ../tmpf
  comparing with ../tmpf
  searching for changes
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  changeset:   1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add b
  
  changeset:   2:245bde4270cd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add original_c
  
  changeset:   3:6f9641995072
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add n3w_3_c
  
  changeset:   4:94b33453f93b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add original_d
  
  changeset:   5:cda648ca50f5
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add original_e
  
  $ hg push ../tmpf -f # -f because be push unstable too
  pushing to ../tmpf
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 6 changes to 6 files (+1 heads)

no warning displayed

  $ hg push ../tmpf
  pushing to ../tmpf
  searching for changes
  no changes found
  [1]

Do not warn about new head when the new head is a successors of a remote one

  $ hg glog
  @  changeset:   5:cda648ca50f5
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add original_e
  |
  x  changeset:   4:94b33453f93b
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add original_d
  |
  o  changeset:   3:6f9641995072
  |  parent:      1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add n3w_3_c
  |
  | o  changeset:   2:245bde4270cd
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add original_c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
  $ hg up -q 'desc(n3w_3_c)'
  $ mkcommit obsolete_e
  created new head
  $ hg debugobsolete `getid 'original_e'` `getid 'obsolete_e'`
  $ hg outgoing ../tmpf # parasite hg outgoing testin
  comparing with ../tmpf
  searching for changes
  changeset:   6:3de5eca88c00
  tag:         tip
  parent:      3:6f9641995072
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add obsolete_e
  
  $ hg push ../tmpf
  pushing to ../tmpf
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)

#if serve

check hgweb does not explode
====================================

  $ hg unbundle $TESTDIR/bundles/hgweb+obs.hg
  adding changesets
  adding manifests
  adding file changes
  added 62 changesets with 63 changes to 9 files (+60 heads)
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ for node in `hg log -r 'desc(babar_)' --template '{node}\n'`;
  > do
  >    hg debugobsolete $node
  > done
  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

check changelog view

  $ "$TESTDIR/get-with-headers.py" --headeronly localhost:$HGPORT 'shortlog/'
  200 Script output follows

check graph view

  $ "$TESTDIR/get-with-headers.py" --headeronly localhost:$HGPORT 'graph'
  200 Script output follows

check filelog view

  $ "$TESTDIR/get-with-headers.py" --headeronly localhost:$HGPORT 'log/'`hg id --debug --id`/'babar'
  200 Script output follows

  $ "$TESTDIR/get-with-headers.py" --headeronly localhost:$HGPORT 'rev/68'
  200 Script output follows
  $ "$TESTDIR/get-with-headers.py" --headeronly localhost:$HGPORT 'rev/67'
  404 Not Found
  [1]

check that web.view config option:

  $ "$TESTDIR/killdaemons.py" hg.pid
  $ cat >> .hg/hgrc << EOF
  > [web]
  > view=all
  > EOF
  $ wait
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ "$TESTDIR/get-with-headers.py" --headeronly localhost:$HGPORT 'rev/67'
  200 Script output follows
  $ "$TESTDIR/killdaemons.py" hg.pid

Checking _enable=False warning if obsolete marker exists

  $ echo '[extensions]' >> $HGRCPATH
  $ echo "obs=!" >> $HGRCPATH
  $ hg log -r tip
  obsolete feature not enabled but 68 markers found!
  changeset:   68:c15e9edfca13
  tag:         tip
  parent:      7:50c51b361e60
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add celestine
  

reenable for later test

  $ echo '[extensions]' >> $HGRCPATH
  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH

#endif

Test incoming/outcoming with changesets obsoleted remotely, known locally
===============================================================================

This test issue 3805

  $ hg init repo-issue3805
  $ cd repo-issue3805
  $ echo "foo" > foo
  $ hg ci -Am "A"
  adding foo
  $ hg clone . ../other-issue3805
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "bar" >> foo
  $ hg ci --amend
  $ cd ../other-issue3805
  $ hg log -G
  @  changeset:   0:193e9254ce7e
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  
  $ hg log -G -R ../repo-issue3805
  @  changeset:   2:3816541e5485
     tag:         tip
     parent:      -1:000000000000
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  
  $ hg incoming
  comparing with $TESTTMP/tmpe/repo-issue3805 (glob)
  searching for changes
  changeset:   2:3816541e5485
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
  
  $ hg incoming --bundle ../issue3805.hg
  comparing with $TESTTMP/tmpe/repo-issue3805 (glob)
  searching for changes
  changeset:   2:3816541e5485
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
  
  $ hg outgoing
  comparing with $TESTTMP/tmpe/repo-issue3805 (glob)
  searching for changes
  no changes found
  [1]

#if serve

  $ hg serve -R ../repo-issue3805 -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

  $ hg incoming http://localhost:$HGPORT
  comparing with http://localhost:$HGPORT/
  searching for changes
  changeset:   1:3816541e5485
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
  
  $ hg outgoing http://localhost:$HGPORT
  comparing with http://localhost:$HGPORT/
  searching for changes
  no changes found
  [1]

  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

#endif

This test issue 3814

(nothing to push but locally hidden changeset)

  $ cd ..
  $ hg init repo-issue3814
  $ cd repo-issue3805
  $ hg push -r 3816541e5485 ../repo-issue3814
  pushing to ../repo-issue3814
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg out ../repo-issue3814
  comparing with ../repo-issue3814
  searching for changes
  no changes found
  [1]



  $ cat >> $HGRCPATH << EOF
  > [phases]
  > # public changeset are not obsolete
  > publish=false
  > [ui]
  > logtemplate="{rev}:{node|short} ({phase}) [{tags} {bookmarks}] {desc|firstline}\n"
  > [experimental]
  > # drop me once bundle2 is the default,
  > # added to get test change early.
  > bundle2-exp = True
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "add $1"
  > }
  $ getid() {
  >    hg log -T "{node}\n" --hidden -r "desc('$1')"
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
  abort: creating obsolete markers is not enabled on this repo
  [255]

Enabling it

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=createmarkers,exchange
  > EOF

Killing a single changeset without replacement

  $ hg debugobsolete 0
  abort: changeset references must be full hexadecimal node identifiers
  [255]
  $ hg debugobsolete '00'
  abort: changeset references must be full hexadecimal node identifiers
  [255]
  $ hg debugobsolete -d '0 0' `getid kill_me` -u babar
  $ hg debugobsolete
  97b7c2d76b1845ed3eb988cd612611e72406cef0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'babar'}

(test that mercurial is not confused)

  $ hg up null --quiet # having 0 as parent prevents it to be hidden
  $ hg tip
  -1:000000000000 (public) [tip ] 
  $ hg up --hidden tip --quiet

Killing a single changeset with itself should fail
(simple local safeguard)

  $ hg debugobsolete `getid kill_me` `getid kill_me`
  abort: bad obsmarker input: in-marker cycle with 97b7c2d76b1845ed3eb988cd612611e72406cef0
  [255]

  $ cd ..

Killing a single changeset with replacement
(and testing the format option)

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
  $ hg debugobsolete --config format.obsstore-version=0 --flag 12 `getid original_c`  `getid new_c` -d '121 120'
  $ hg log -r 'hidden()' --template '{rev}:{node|short} {desc}\n' --hidden
  2:245bde4270cd add original_c
  $ hg debugrevlog -cd
  # rev p1rev p2rev start   end deltastart base   p1   p2 rawsize totalsize compression heads chainlen
      0    -1    -1     0    59          0    0    0    0      58        58           0     1        0
      1     0    -1    59   118         59   59    0    0      58       116           0     1        0
      2     1    -1   118   193        118  118   59    0      76       192           0     1        0
      3     1    -1   193   260        193  193   59    0      66       258           0     2        0
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}

(check for version number of the obsstore)

  $ dd bs=1 count=1 if=.hg/store/obsstore 2>/dev/null
  \x00 (no-eol) (esc)

do it again (it read the obsstore before adding new changeset)

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit new_2_c
  created new head
  $ hg debugobsolete -d '1337 0' `getid new_c` `getid new_2_c`
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}

Register two markers with a missing node

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit new_3_c
  created new head
  $ hg debugobsolete -d '1338 0' `getid new_2_c` 1337133713371337133713371337133713371337
  $ hg debugobsolete -d '1339 0' 1337133713371337133713371337133713371337 `getid new_3_c`
  $ hg debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}

Test the --index option of debugobsolete command
  $ hg debugobsolete --index
  0 245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  1 cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}
  2 ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  3 1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}

Refuse pathological nullid successors
  $ hg debugobsolete -d '9001 0' 1337133713371337133713371337133713371337 0000000000000000000000000000000000000000
  transaction abort!
  rollback completed
  abort: bad obsolescence marker detected: invalid successors nullid
  [255]

Check that graphlog detect that a changeset is obsolete:

  $ hg log -G
  @  5:5601fb93a350 (draft) [tip ] add new_3_c
  |
  o  1:7c3bad9141dc (draft) [ ] add b
  |
  o  0:1f0dee641bb7 (draft) [ ] add a
  

check that heads does not report them

  $ hg heads
  5:5601fb93a350 (draft) [tip ] add new_3_c
  $ hg heads --hidden
  5:5601fb93a350 (draft) [tip ] add new_3_c
  4:ca819180edb9 (draft) [ ] add new_2_c
  3:cdbce2fbb163 (draft) [ ] add new_c
  2:245bde4270cd (draft) [ ] add original_c


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
  phases: 3 draft
  remote: 3 outgoing

  $ hg summary --remote --hidden
  parent: 5:5601fb93a350 tip
   add new_3_c
  branch: default
  commit: (clean)
  update: 3 new changesets, 4 branch heads (merge)
  phases: 6 draft
  remote: 3 outgoing

check that various commands work well with filtering

  $ hg tip
  5:5601fb93a350 (draft) [tip ] add new_3_c
  $ hg log -r 6
  abort: unknown revision '6'!
  [255]
  $ hg log -r 4
  abort: hidden revision '4'!
  (use --hidden to access hidden revisions)
  [255]
  $ hg debugrevspec 'rev(6)'
  $ hg debugrevspec 'rev(4)'
  $ hg debugrevspec 'null'
  -1

Check that public changeset are not accounted as obsolete:

  $ hg --hidden phase --public 2
  $ hg log -G
  @  5:5601fb93a350 (draft) [tip ] add new_3_c
  |
  | o  2:245bde4270cd (public) [ ] add original_c
  |/
  o  1:7c3bad9141dc (public) [ ] add b
  |
  o  0:1f0dee641bb7 (public) [ ] add a
  

And that bumped changeset are detected
--------------------------------------

If we didn't filtered obsolete changesets out, 3 and 4 would show up too. Also
note that the bumped changeset (5:5601fb93a350) is not a direct successor of
the public changeset

  $ hg log --hidden -r 'bumped()'
  5:5601fb93a350 (draft) [tip ] add new_3_c

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

  $ hg summary
  parent: 5:5601fb93a350 tip
   add new_3_c
  branch: default
  commit: (clean)
  update: 1 new changesets, 2 branch heads (merge)
  phases: 1 draft
  bumped: 1 changesets
  $ hg up '5^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg revert -ar 5
  adding new_3_c
  $ hg ci -m 'add n3w_3_c'
  created new head
  $ hg debugobsolete -d '1338 0' --flags 1 `getid new_3_c` `getid n3w_3_c`
  $ hg log -r 'bumped()'
  $ hg log -G
  @  6:6f9641995072 (draft) [tip ] add n3w_3_c
  |
  | o  2:245bde4270cd (public) [ ] add original_c
  |/
  o  1:7c3bad9141dc (public) [ ] add b
  |
  o  0:1f0dee641bb7 (public) [ ] add a
  

  $ cd ..

Revision 0 is hidden
--------------------

  $ hg init rev0hidden
  $ cd rev0hidden

  $ mkcommit kill0
  $ hg up -q null
  $ hg debugobsolete `getid kill0`
  $ mkcommit a
  $ mkcommit b

Should pick the first visible revision as "repo" node

  $ hg archive ../archive-null
  $ cat ../archive-null/.hg_archival.txt
  repo: 1f0dee641bb7258c56bd60e93edfa2405381c41e
  node: 7c3bad9141dcb46ff89abf5f61856facd56e476c
  branch: default
  latesttag: null
  latesttagdistance: 2
  changessincelatesttag: 2


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
  0:1f0dee641bb7 (public) [ ] add a
  1:7c3bad9141dc (public) [ ] add b
  2:245bde4270cd (public) [ ] add original_c
  6:6f9641995072 (draft) [tip ] add n3w_3_c

Try to pull markers
(extinct changeset are excluded but marker are pushed)

  $ hg pull ../tmpb
  pulling from ../tmpb
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 4 files (+1 heads)
  5 new obsolescence markers
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg debugobsolete
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}

Rollback//Transaction support

  $ hg debugobsolete -d '1340 0' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ hg debugobsolete
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 0 (Thu Jan 01 00:22:20 1970 +0000) {'user': 'test'}
  $ hg rollback -n
  repository tip rolled back to revision 3 (undo debugobsolete)
  $ hg rollback
  repository tip rolled back to revision 3 (undo debugobsolete)
  $ hg debugobsolete
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}

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
  5 new obsolescence markers
  $ hg -R tmpd debugobsolete | sort
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}

Check obsolete keys are exchanged only if source has an obsolete store

  $ hg init empty
  $ hg --config extensions.debugkeys=debugkeys.py -R empty push tmpd
  pushing to tmpd
  listkeys phases
  listkeys bookmarks
  no changes found
  listkeys phases
  [1]

clone support
(markers are copied and extinct changesets are included to allow hardlinks)

  $ hg clone tmpb clone-dest
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R clone-dest log -G --hidden
  @  6:6f9641995072 (draft) [tip ] add n3w_3_c
  |
  | x  5:5601fb93a350 (draft) [ ] add new_3_c
  |/
  | x  4:ca819180edb9 (draft) [ ] add new_2_c
  |/
  | x  3:cdbce2fbb163 (draft) [ ] add new_c
  |/
  | o  2:245bde4270cd (public) [ ] add original_c
  |/
  o  1:7c3bad9141dc (public) [ ] add b
  |
  o  0:1f0dee641bb7 (public) [ ] add a
  
  $ hg -R clone-dest debugobsolete
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}


Destination repo have existing data
---------------------------------------

On pull

  $ hg init tmpe
  $ cd tmpe
  $ hg debugobsolete -d '1339 0' 1339133913391339133913391339133913391339 ca819180edb99ed25ceafb3e9584ac287e240b00
  $ hg pull ../tmpb
  pulling from ../tmpb
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 4 files (+1 heads)
  5 new obsolescence markers
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg debugobsolete
  1339133913391339133913391339133913391339 ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}


On push

  $ hg push ../tmpc
  pushing to ../tmpc
  searching for changes
  no changes found
  1 new obsolescence markers
  [1]
  $ hg -R ../tmpc debugobsolete
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}
  1339133913391339133913391339133913391339 ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}

detect outgoing obsolete and unstable
---------------------------------------


  $ hg log -G
  o  3:6f9641995072 (draft) [tip ] add n3w_3_c
  |
  | o  2:245bde4270cd (public) [ ] add original_c
  |/
  o  1:7c3bad9141dc (public) [ ] add b
  |
  o  0:1f0dee641bb7 (public) [ ] add a
  
  $ hg up 'desc("n3w_3_c")'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit original_d
  $ mkcommit original_e
  $ hg debugobsolete --record-parents `getid original_d` -d '0 0'
  $ hg debugobsolete | grep `getid original_d`
  94b33453f93bdb8d457ef9b770851a618bf413e1 0 {6f96419950729f3671185b847352890f074f7557} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ hg log -r 'obsolete()'
  4:94b33453f93b (draft) [ ] add original_d
  $ hg summary
  parent: 5:cda648ca50f5 tip
   add original_e
  branch: default
  commit: (clean)
  update: 1 new changesets, 2 branch heads (merge)
  phases: 3 draft
  unstable: 1 changesets
  $ hg log -G -r '::unstable()'
  @  5:cda648ca50f5 (draft) [tip ] add original_e
  |
  x  4:94b33453f93b (draft) [ ] add original_d
  |
  o  3:6f9641995072 (draft) [ ] add n3w_3_c
  |
  o  1:7c3bad9141dc (public) [ ] add b
  |
  o  0:1f0dee641bb7 (public) [ ] add a
  

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
  0:1f0dee641bb7 (public) [ ] add a
  1:7c3bad9141dc (public) [ ] add b
  2:245bde4270cd (public) [ ] add original_c
  3:6f9641995072 (draft) [ ] add n3w_3_c
  4:94b33453f93b (draft) [ ] add original_d
  5:cda648ca50f5 (draft) [tip ] add original_e
  $ hg push ../tmpf -f # -f because be push unstable too
  pushing to ../tmpf
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 6 changes to 6 files (+1 heads)
  7 new obsolescence markers

no warning displayed

  $ hg push ../tmpf
  pushing to ../tmpf
  searching for changes
  no changes found
  [1]

Do not warn about new head when the new head is a successors of a remote one

  $ hg log -G
  @  5:cda648ca50f5 (draft) [tip ] add original_e
  |
  x  4:94b33453f93b (draft) [ ] add original_d
  |
  o  3:6f9641995072 (draft) [ ] add n3w_3_c
  |
  | o  2:245bde4270cd (public) [ ] add original_c
  |/
  o  1:7c3bad9141dc (public) [ ] add b
  |
  o  0:1f0dee641bb7 (public) [ ] add a
  
  $ hg up -q 'desc(n3w_3_c)'
  $ mkcommit obsolete_e
  created new head
  $ hg debugobsolete `getid 'original_e'` `getid 'obsolete_e'`
  $ hg outgoing ../tmpf # parasite hg outgoing testin
  comparing with ../tmpf
  searching for changes
  6:3de5eca88c00 (draft) [tip ] add obsolete_e
  $ hg push ../tmpf
  pushing to ../tmpf
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  1 new obsolescence markers

test relevance computation
---------------------------------------

Checking simple case of "marker relevance".


Reminder of the repo situation

  $ hg log --hidden --graph
  @  6:3de5eca88c00 (draft) [tip ] add obsolete_e
  |
  | x  5:cda648ca50f5 (draft) [ ] add original_e
  | |
  | x  4:94b33453f93b (draft) [ ] add original_d
  |/
  o  3:6f9641995072 (draft) [ ] add n3w_3_c
  |
  | o  2:245bde4270cd (public) [ ] add original_c
  |/
  o  1:7c3bad9141dc (public) [ ] add b
  |
  o  0:1f0dee641bb7 (public) [ ] add a
  

List of all markers

  $ hg debugobsolete
  1339133913391339133913391339133913391339 ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}
  94b33453f93bdb8d457ef9b770851a618bf413e1 0 {6f96419950729f3671185b847352890f074f7557} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  cda648ca50f50482b7055c0b0c4c117bba6733d9 3de5eca88c00aa039da7399a220f4a5221faa585 0 (*) {'user': 'test'} (glob)

List of changesets with no chain

  $ hg debugobsolete --hidden --rev ::2

List of changesets that are included on marker chain

  $ hg debugobsolete --hidden --rev 6
  cda648ca50f50482b7055c0b0c4c117bba6733d9 3de5eca88c00aa039da7399a220f4a5221faa585 0 (*) {'user': 'test'} (glob)

List of changesets with a longer chain, (including a pruned children)

  $ hg debugobsolete --hidden --rev 3
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  1339133913391339133913391339133913391339 ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  94b33453f93bdb8d457ef9b770851a618bf413e1 0 {6f96419950729f3671185b847352890f074f7557} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}

List of both

  $ hg debugobsolete --hidden --rev 3::6
  1337133713371337133713371337133713371337 5601fb93a350734d935195fee37f4054c529ff39 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  1339133913391339133913391339133913391339 ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:19 1970 +0000) {'user': 'test'}
  245bde4270cd1072a27757984f9cda8ba26f08ca cdbce2fbb16313928851e97e0d85413f3f7eb77f C (Thu Jan 01 00:00:01 1970 -0002) {'user': 'test'}
  5601fb93a350734d935195fee37f4054c529ff39 6f96419950729f3671185b847352890f074f7557 1 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  94b33453f93bdb8d457ef9b770851a618bf413e1 0 {6f96419950729f3671185b847352890f074f7557} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ca819180edb99ed25ceafb3e9584ac287e240b00 1337133713371337133713371337133713371337 0 (Thu Jan 01 00:22:18 1970 +0000) {'user': 'test'}
  cda648ca50f50482b7055c0b0c4c117bba6733d9 3de5eca88c00aa039da7399a220f4a5221faa585 0 (*) {'user': 'test'} (glob)
  cdbce2fbb16313928851e97e0d85413f3f7eb77f ca819180edb99ed25ceafb3e9584ac287e240b00 0 (Thu Jan 01 00:22:17 1970 +0000) {'user': 'test'}

#if serve

Test the debug output for exchange
----------------------------------

  $ hg pull ../tmpb --config 'experimental.obsmarkers-exchange-debug=True' --config 'experimental.bundle2-exp=True'
  pulling from ../tmpb
  searching for changes
  no changes found
  obsmarker-exchange: 346 bytes received

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

  $ get-with-headers.py --headeronly localhost:$HGPORT 'shortlog/'
  200 Script output follows

check graph view

  $ get-with-headers.py --headeronly localhost:$HGPORT 'graph'
  200 Script output follows

check filelog view

  $ get-with-headers.py --headeronly localhost:$HGPORT 'log/'`hg log -r . -T "{node}"`/'babar'
  200 Script output follows

  $ get-with-headers.py --headeronly localhost:$HGPORT 'rev/68'
  200 Script output follows
  $ get-with-headers.py --headeronly localhost:$HGPORT 'rev/67'
  404 Not Found
  [1]

check that web.view config option:

  $ killdaemons.py hg.pid
  $ cat >> .hg/hgrc << EOF
  > [web]
  > view=all
  > EOF
  $ wait
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ get-with-headers.py --headeronly localhost:$HGPORT 'rev/67'
  200 Script output follows
  $ killdaemons.py hg.pid

Checking _enable=False warning if obsolete marker exists

  $ echo '[experimental]' >> $HGRCPATH
  $ echo "evolution=" >> $HGRCPATH
  $ hg log -r tip
  obsolete feature not enabled but 68 markers found!
  68:c15e9edfca13 (draft) [tip ] add celestine

reenable for later test

  $ echo '[experimental]' >> $HGRCPATH
  $ echo "evolution=createmarkers,exchange" >> $HGRCPATH

#endif

Test incoming/outcoming with changesets obsoleted remotely, known locally
===============================================================================

This test issue 3805

  $ hg init repo-issue3805
  $ cd repo-issue3805
  $ echo "base" > base
  $ hg ci -Am "base"
  adding base
  $ echo "foo" > foo
  $ hg ci -Am "A"
  adding foo
  $ hg clone . ../other-issue3805
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "bar" >> foo
  $ hg ci --amend
  $ cd ../other-issue3805
  $ hg log -G
  @  1:29f0c6921ddd (draft) [tip ] A
  |
  o  0:d20a80d4def3 (draft) [ ] base
  
  $ hg log -G -R ../repo-issue3805
  @  3:323a9c3ddd91 (draft) [tip ] A
  |
  o  0:d20a80d4def3 (draft) [ ] base
  
  $ hg incoming
  comparing with $TESTTMP/tmpe/repo-issue3805 (glob)
  searching for changes
  3:323a9c3ddd91 (draft) [tip ] A
  $ hg incoming --bundle ../issue3805.hg
  comparing with $TESTTMP/tmpe/repo-issue3805 (glob)
  searching for changes
  3:323a9c3ddd91 (draft) [tip ] A
  $ hg outgoing
  comparing with $TESTTMP/tmpe/repo-issue3805 (glob)
  searching for changes
  1:29f0c6921ddd (draft) [tip ] A

#if serve

  $ hg serve -R ../repo-issue3805 -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

  $ hg incoming http://localhost:$HGPORT
  comparing with http://localhost:$HGPORT/
  searching for changes
  2:323a9c3ddd91 (draft) [tip ] A
  $ hg outgoing http://localhost:$HGPORT
  comparing with http://localhost:$HGPORT/
  searching for changes
  1:29f0c6921ddd (draft) [tip ] A

  $ killdaemons.py

#endif

This test issue 3814

(nothing to push but locally hidden changeset)

  $ cd ..
  $ hg init repo-issue3814
  $ cd repo-issue3805
  $ hg push -r 323a9c3ddd91 ../repo-issue3814
  pushing to ../repo-issue3814
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  2 new obsolescence markers
  $ hg out ../repo-issue3814
  comparing with ../repo-issue3814
  searching for changes
  no changes found
  [1]

Test that a local tag blocks a changeset from being hidden

  $ hg tag -l visible -r 1 --hidden
  $ hg log -G
  @  3:323a9c3ddd91 (draft) [tip ] A
  |
  | x  1:29f0c6921ddd (draft) [visible ] A
  |/
  o  0:d20a80d4def3 (draft) [ ] base
  
Test that removing a local tag does not cause some commands to fail

  $ hg tag -l -r tip tiptag
  $ hg tags
  tiptag                             3:323a9c3ddd91
  tip                                3:323a9c3ddd91
  visible                            1:29f0c6921ddd
  $ hg --config extensions.strip= strip -r tip --no-backup
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg tags
  visible                            1:29f0c6921ddd
  tip                                1:29f0c6921ddd

Test bundle overlay onto hidden revision

  $ cd ..
  $ hg init repo-bundleoverlay
  $ cd repo-bundleoverlay
  $ echo "A" > foo
  $ hg ci -Am "A"
  adding foo
  $ echo "B" >> foo
  $ hg ci -m "B"
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "C" >> foo
  $ hg ci -m "C"
  created new head
  $ hg log -G
  @  2:c186d7714947 (draft) [tip ] C
  |
  | o  1:44526ebb0f98 (draft) [ ] B
  |/
  o  0:4b34ecfb0d56 (draft) [ ] A
  

  $ hg clone -r1 . ../other-bundleoverlay
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../other-bundleoverlay
  $ echo "B+" >> foo
  $ hg ci --amend -m "B+"
  $ hg log -G --hidden
  @  3:b7d587542d40 (draft) [tip ] B+
  |
  | x  2:eb95e9297e18 (draft) [ ] temporary amend commit for 44526ebb0f98
  | |
  | x  1:44526ebb0f98 (draft) [ ] B
  |/
  o  0:4b34ecfb0d56 (draft) [ ] A
  

  $ hg incoming ../repo-bundleoverlay --bundle ../bundleoverlay.hg
  comparing with ../repo-bundleoverlay
  searching for changes
  1:44526ebb0f98 (draft) [ ] B
  2:c186d7714947 (draft) [tip ] C
  $ hg log -G -R ../bundleoverlay.hg
  o  4:c186d7714947 (draft) [tip ] C
  |
  | @  3:b7d587542d40 (draft) [ ] B+
  |/
  o  0:4b34ecfb0d56 (draft) [ ] A
  

#if serve

Test issue 4506

  $ cd ..
  $ hg init repo-issue4506
  $ cd repo-issue4506
  $ echo "0" > foo
  $ hg add foo
  $ hg ci -m "content-0"

  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "1" > bar
  $ hg add bar
  $ hg ci -m "content-1"
  created new head
  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg graft 1
  grafting 1:1c9eddb02162 "content-1" (tip)

  $ hg debugobsolete `hg log -r1 -T'{node}'` `hg log -r2 -T'{node}'`

  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

  $ get-with-headers.py --headeronly localhost:$HGPORT 'rev/1'
  404 Not Found
  [1]
  $ get-with-headers.py --headeronly localhost:$HGPORT 'file/tip/bar'
  200 Script output follows
  $ get-with-headers.py --headeronly localhost:$HGPORT 'annotate/tip/bar'
  200 Script output follows

  $ killdaemons.py

#endif

Test heads computation on pending index changes with obsolescence markers
  $ cd ..
  $ cat >$TESTTMP/test_extension.py  << EOF
  > from mercurial import cmdutil
  > from mercurial.i18n import _
  > 
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command("amendtransient",[], _('hg amendtransient [rev]'))
  > def amend(ui, repo, *pats, **opts):
  >   def commitfunc(ui, repo, message, match, opts):
  >     return repo.commit(message, repo['.'].user(), repo['.'].date(), match)
  >   opts['message'] = 'Test'
  >   opts['logfile'] = None
  >   cmdutil.amend(ui, repo, commitfunc, repo['.'], {}, pats, opts)
  >   ui.write('%s\n' % repo.changelog.headrevs())
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > testextension=$TESTTMP/test_extension.py
  > EOF
  $ hg init repo-issue-nativerevs-pending-changes
  $ cd repo-issue-nativerevs-pending-changes
  $ mkcommit a
  $ mkcommit b
  $ hg up ".^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo aa > a
  $ hg amendtransient
  [1, 3]

Check that corrupted hidden cache does not crash

  $ printf "" > .hg/cache/hidden
  $ hg log -r . -T '{node}' --debug
  corrupted hidden cache
  8fd96dfc63e51ed5a8af1bec18eb4b19dbf83812 (no-eol)
  $ hg log -r . -T '{node}' --debug
  8fd96dfc63e51ed5a8af1bec18eb4b19dbf83812 (no-eol)

#if unix-permissions
Check that wrong hidden cache permission does not crash

  $ chmod 000 .hg/cache/hidden
  $ hg log -r . -T '{node}' --debug
  cannot read hidden cache
  error writing hidden changesets cache
  8fd96dfc63e51ed5a8af1bec18eb4b19dbf83812 (no-eol)
#endif

Test cache consistency for the visible filter
1) We want to make sure that the cached filtered revs are invalidated when
bookmarks change
  $ cd ..
  $ cat >$TESTTMP/test_extension.py  << EOF
  > import weakref
  > from mercurial import cmdutil, extensions, bookmarks, repoview
  > def _bookmarkchanged(orig, bkmstoreinst, *args, **kwargs):
  >  reporef = weakref.ref(bkmstoreinst._repo)
  >  def trhook(tr):
  >   repo = reporef()
  >   hidden1 = repoview.computehidden(repo)
  >   hidden = repoview.filterrevs(repo, 'visible')
  >   if sorted(hidden1) != sorted(hidden):
  >     print "cache inconsistency"
  >  bkmstoreinst._repo.currenttransaction().addpostclose('test_extension', trhook)
  >  orig(bkmstoreinst, *args, **kwargs)
  > def extsetup(ui):
  >   extensions.wrapfunction(bookmarks.bmstore, 'recordchange',
  >                           _bookmarkchanged)
  > EOF

  $ hg init repo-cache-inconsistency
  $ cd repo-issue-nativerevs-pending-changes
  $ mkcommit a
  a already tracked!
  $ mkcommit b
  $ hg id
  13bedc178fce tip
  $ echo "hello" > b
  $ hg commit --amend -m "message"
  $ hg book bookb -r 13bedc178fce --hidden
  $ hg log -r 13bedc178fce
  5:13bedc178fce (draft) [ bookb] add b
  $ hg book -d bookb
  $ hg log -r 13bedc178fce
  abort: hidden revision '13bedc178fce'!
  (use --hidden to access hidden revisions)
  [255]

Empty out the test extension, as it isn't compatible with later parts
of the test.
  $ echo > $TESTTMP/test_extension.py

Test ability to pull changeset with locally applying obsolescence markers
(issue4945)

  $ cd ..
  $ hg init issue4845
  $ cd issue4845

  $ echo foo > f0
  $ hg add f0
  $ hg ci -m '0'
  $ echo foo > f1
  $ hg add f1
  $ hg ci -m '1'
  $ echo foo > f2
  $ hg add f2
  $ hg ci -m '2'

  $ echo bar > f2
  $ hg commit --amend --config experimetnal.evolution=createmarkers
  $ hg log -G
  @  4:b0551702f918 (draft) [tip ] 2
  |
  o  1:e016b03fd86f (draft) [ ] 1
  |
  o  0:a78f55e5508c (draft) [ ] 0
  
  $ hg log -G --hidden
  @  4:b0551702f918 (draft) [tip ] 2
  |
  | x  3:f27abbcc1f77 (draft) [ ] temporary amend commit for e008cf283490
  | |
  | x  2:e008cf283490 (draft) [ ] 2
  |/
  o  1:e016b03fd86f (draft) [ ] 1
  |
  o  0:a78f55e5508c (draft) [ ] 0
  

  $ hg strip -r 1 --config extensions.strip=
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/tmpe/issue4845/.hg/strip-backup/e016b03fd86f-c41c6bcc-backup.hg (glob)
  $ hg log -G
  @  0:a78f55e5508c (draft) [tip ] 0
  
  $ hg log -G --hidden
  @  0:a78f55e5508c (draft) [tip ] 0
  

  $ hg pull .hg/strip-backup/*
  pulling from .hg/strip-backup/e016b03fd86f-c41c6bcc-backup.hg
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)
  $ hg log -G
  o  2:b0551702f918 (draft) [tip ] 2
  |
  o  1:e016b03fd86f (draft) [ ] 1
  |
  @  0:a78f55e5508c (draft) [ ] 0
  
  $ hg log -G --hidden
  o  2:b0551702f918 (draft) [tip ] 2
  |
  o  1:e016b03fd86f (draft) [ ] 1
  |
  @  0:a78f55e5508c (draft) [ ] 0
  
Test that 'hg debugobsolete --index --rev' can show indices of obsmarkers when
only a subset of those are displayed (because of --rev option)
  $ hg init doindexrev
  $ cd doindexrev
  $ echo a > a
  $ hg ci -Am a
  adding a
  $ hg ci --amend -m aa
  $ echo b > b
  $ hg ci -Am b
  adding b
  $ hg ci --amend -m bb
  $ echo c > c
  $ hg ci -Am c
  adding c
  $ hg ci --amend -m cc
  $ echo d > d
  $ hg ci -Am d
  adding d
  $ hg ci --amend -m dd
  $ hg debugobsolete --index --rev "3+7"
  1 6fdef60fcbabbd3d50e9b9cbc2a240724b91a5e1 d27fb9b066076fd921277a4b9e8b9cb48c95bc6a 0 \(.*\) {'user': 'test'} (re)
  3 4715cf767440ed891755448016c2b8cf70760c30 7ae79c5d60f049c7b0dd02f5f25b9d60aaf7b36d 0 \(.*\) {'user': 'test'} (re)

Test the --delete option of debugobsolete command
  $ hg debugobsolete --index
  0 cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b f9bd49731b0b175e42992a3c8fa6c678b2bc11f1 0 \(.*\) {'user': 'test'} (re)
  1 6fdef60fcbabbd3d50e9b9cbc2a240724b91a5e1 d27fb9b066076fd921277a4b9e8b9cb48c95bc6a 0 \(.*\) {'user': 'test'} (re)
  2 1ab51af8f9b41ef8c7f6f3312d4706d870b1fb74 29346082e4a9e27042b62d2da0e2de211c027621 0 \(.*\) {'user': 'test'} (re)
  3 4715cf767440ed891755448016c2b8cf70760c30 7ae79c5d60f049c7b0dd02f5f25b9d60aaf7b36d 0 \(.*\) {'user': 'test'} (re)
  $ hg debugobsolete --delete 1 --delete 3
  deleted 2 obsolescense markers
  $ hg debugobsolete
  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b f9bd49731b0b175e42992a3c8fa6c678b2bc11f1 0 \(.*\) {'user': 'test'} (re)
  1ab51af8f9b41ef8c7f6f3312d4706d870b1fb74 29346082e4a9e27042b62d2da0e2de211c027621 0 \(.*\) {'user': 'test'} (re)
  $ cd ..


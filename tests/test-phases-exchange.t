  $ setconfig extensions.treemanifest=!
#require killdaemons


  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > phasereport=$TESTDIR/testlib/ext-phase-report.py
  > EOF

  $ hgph() { hg log -G --template "{rev} {phase} {desc} - {node|short}\n" $*; }

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    message="$1"
  >    shift
  >    hg ci -m "$message" $*
  > }

  $ hg init alpha
  $ cd alpha
  $ mkcommit a-A
  test-debug-phase: new rev 0:  x -> 1
  $ mkcommit a-B
  test-debug-phase: new rev 1:  x -> 1
  $ mkcommit a-C
  test-debug-phase: new rev 2:  x -> 1
  $ mkcommit a-D
  test-debug-phase: new rev 3:  x -> 1
  $ hgph
  @  3 draft a-D - b555f63b6063
  |
  o  2 draft a-C - 54acac6f23ab
  |
  o  1 draft a-B - 548a3d25dbf0
  |
  o  0 draft a-A - 054250a37db4
  

  $ hg init ../beta
  $ hg push -r 1 ../beta
  pushing to ../beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  test-debug-phase: new rev 0:  x -> 0
  test-debug-phase: new rev 1:  x -> 0
  test-debug-phase: move rev 0: 1 -> 0
  test-debug-phase: move rev 1: 1 -> 0
  $ hgph
  @  3 draft a-D - b555f63b6063
  |
  o  2 draft a-C - 54acac6f23ab
  |
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

  $ cd ../beta
  $ hgph
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hg up -q
  $ mkcommit b-A
  test-debug-phase: new rev 2:  x -> 1
  $ hgph
  @  2 draft b-A - f54f1bb90ff3
  |
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hg pull ../alpha
  pulling from ../alpha
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  new changesets 54acac6f23ab:b555f63b6063
  test-debug-phase: new rev 3:  x -> 0
  test-debug-phase: new rev 4:  x -> 0
  $ hgph
  o  4 public a-D - b555f63b6063
  |
  o  3 public a-C - 54acac6f23ab
  |
  | @  2 draft b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

pull did not updated ../alpha state.
push from alpha to beta should update phase even if nothing is transferred

  $ cd ../alpha
  $ hgph # not updated by remote pull
  @  3 draft a-D - b555f63b6063
  |
  o  2 draft a-C - 54acac6f23ab
  |
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hg push -r 2 ../beta
  pushing to ../beta
  searching for changes
  no changes found
  test-debug-phase: move rev 2: 1 -> 0
  [1]
  $ hgph
  @  3 draft a-D - b555f63b6063
  |
  o  2 public a-C - 54acac6f23ab
  |
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hg push ../beta
  pushing to ../beta
  searching for changes
  no changes found
  test-debug-phase: move rev 3: 1 -> 0
  [1]
  $ hgph
  @  3 public a-D - b555f63b6063
  |
  o  2 public a-C - 54acac6f23ab
  |
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

update must update phase of common changeset too

  $ hg pull ../beta # getting b-A
  pulling from ../beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets f54f1bb90ff3
  test-debug-phase: new rev 4:  x -> 0

  $ cd ../beta
  $ hgph # not updated by remote pull
  o  4 public a-D - b555f63b6063
  |
  o  3 public a-C - 54acac6f23ab
  |
  | @  2 draft b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hg pull ../alpha
  pulling from ../alpha
  searching for changes
  no changes found
  test-debug-phase: move rev 2: 1 -> 0
  $ hgph
  o  4 public a-D - b555f63b6063
  |
  o  3 public a-C - 54acac6f23ab
  |
  | @  2 public b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Publish configuration option
----------------------------

Pull
````

changegroup are added without phase movement

  $ hg bundle -a ../base.bundle
  5 changesets found
  $ cd ..
  $ hg init mu
  $ cd mu
  $ cat > .hg/hgrc << EOF
  > [phases]
  > publish=0
  > EOF
  $ hg unbundle ../base.bundle
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 5 files (+1 heads)
  new changesets 054250a37db4:b555f63b6063
  test-debug-phase: new rev 0:  x -> 1
  test-debug-phase: new rev 1:  x -> 1
  test-debug-phase: new rev 2:  x -> 1
  test-debug-phase: new rev 3:  x -> 1
  test-debug-phase: new rev 4:  x -> 1
  $ hgph
  o  4 draft a-D - b555f63b6063
  |
  o  3 draft a-C - 54acac6f23ab
  |
  | o  2 draft b-A - f54f1bb90ff3
  |/
  o  1 draft a-B - 548a3d25dbf0
  |
  o  0 draft a-A - 054250a37db4
  
  $ cd ..

Pulling from publish=False to publish=False does not move boundary.

  $ hg init nu
  $ cd nu
  $ cat > .hg/hgrc << EOF
  > [phases]
  > publish=0
  > EOF
  $ hg pull ../mu -r 54acac6f23ab
  pulling from ../mu
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  new changesets 054250a37db4:54acac6f23ab
  test-debug-phase: new rev 0:  x -> 1
  test-debug-phase: new rev 1:  x -> 1
  test-debug-phase: new rev 2:  x -> 1
  $ hgph
  o  2 draft a-C - 54acac6f23ab
  |
  o  1 draft a-B - 548a3d25dbf0
  |
  o  0 draft a-A - 054250a37db4
  

Even for common

  $ hg pull ../mu -r f54f1bb90ff3
  pulling from ../mu
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets f54f1bb90ff3
  test-debug-phase: new rev 3:  x -> 1
  $ hgph
  o  3 draft b-A - f54f1bb90ff3
  |
  | o  2 draft a-C - 54acac6f23ab
  |/
  o  1 draft a-B - 548a3d25dbf0
  |
  o  0 draft a-A - 054250a37db4
  


Pulling from Publish=True to Publish=False move boundary in common set.
we are in nu

  $ hg pull ../alpha -r b555f63b6063
  pulling from ../alpha
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets b555f63b6063
  test-debug-phase: move rev 0: 1 -> 0
  test-debug-phase: move rev 1: 1 -> 0
  test-debug-phase: move rev 2: 1 -> 0
  test-debug-phase: new rev 4:  x -> 0
  $ hgph # f54f1bb90ff3 stay draft, not ancestor of -r
  o  4 public a-D - b555f63b6063
  |
  | o  3 draft b-A - f54f1bb90ff3
  | |
  o |  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

pulling from Publish=False to publish=False with some public

  $ hg up -q f54f1bb90ff3
  $ mkcommit n-A
  test-debug-phase: new rev 5:  x -> 1
  $ mkcommit n-B
  test-debug-phase: new rev 6:  x -> 1
  $ hgph
  @  6 draft n-B - 145e75495359
  |
  o  5 draft n-A - d6bcb4f74035
  |
  | o  4 public a-D - b555f63b6063
  | |
  o |  3 draft b-A - f54f1bb90ff3
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ cd ../mu
  $ hg pull ../nu
  pulling from ../nu
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  new changesets d6bcb4f74035:145e75495359
  test-debug-phase: move rev 0: 1 -> 0
  test-debug-phase: move rev 1: 1 -> 0
  test-debug-phase: move rev 3: 1 -> 0
  test-debug-phase: move rev 4: 1 -> 0
  test-debug-phase: new rev 5:  x -> 1
  test-debug-phase: new rev 6:  x -> 1
  $ hgph
  o  6 draft n-B - 145e75495359
  |
  o  5 draft n-A - d6bcb4f74035
  |
  | o  4 public a-D - b555f63b6063
  | |
  | o  3 public a-C - 54acac6f23ab
  | |
  o |  2 draft b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ cd ..

pulling into publish=True

  $ cd alpha
  $ hgph
  o  4 public b-A - f54f1bb90ff3
  |
  | @  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hg pull ../mu
  pulling from ../mu
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  new changesets d6bcb4f74035:145e75495359
  test-debug-phase: new rev 5:  x -> 1
  test-debug-phase: new rev 6:  x -> 1
  $ hgph
  o  6 draft n-B - 145e75495359
  |
  o  5 draft n-A - d6bcb4f74035
  |
  o  4 public b-A - f54f1bb90ff3
  |
  | @  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ cd ..

pulling back into original repo

  $ cd nu
  $ hg pull ../alpha
  pulling from ../alpha
  searching for changes
  no changes found
  test-debug-phase: move rev 3: 1 -> 0
  test-debug-phase: move rev 5: 1 -> 0
  test-debug-phase: move rev 6: 1 -> 0
  $ hgph
  @  6 public n-B - 145e75495359
  |
  o  5 public n-A - d6bcb4f74035
  |
  | o  4 public a-D - b555f63b6063
  | |
  o |  3 public b-A - f54f1bb90ff3
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Push
````

(inserted)

Test that phase are pushed even when they are nothing to pus
(this might be tested later bu are very convenient to not alter too much test)

Push back to alpha

  $ hg push ../alpha # from nu
  pushing to ../alpha
  searching for changes
  no changes found
  test-debug-phase: move rev 5: 1 -> 0
  test-debug-phase: move rev 6: 1 -> 0
  [1]
  $ cd ..
  $ cd alpha
  $ hgph
  o  6 public n-B - 145e75495359
  |
  o  5 public n-A - d6bcb4f74035
  |
  o  4 public b-A - f54f1bb90ff3
  |
  | @  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

(end insertion)


initial setup

  $ hg log -G # of alpha
  o  changeset:   6:145e75495359
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     n-B
  |
  o  changeset:   5:d6bcb4f74035
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     n-A
  |
  o  changeset:   4:f54f1bb90ff3
  |  parent:      1:548a3d25dbf0
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b-A
  |
  | @  changeset:   3:b555f63b6063
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     a-D
  | |
  | o  changeset:   2:54acac6f23ab
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     a-C
  |
  o  changeset:   1:548a3d25dbf0
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     a-B
  |
  o  changeset:   0:054250a37db4
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a-A
  
  $ mkcommit a-E
  test-debug-phase: new rev 7:  x -> 1
  $ mkcommit a-F
  test-debug-phase: new rev 8:  x -> 1
  $ mkcommit a-G
  test-debug-phase: new rev 9:  x -> 1
  $ hg up d6bcb4f74035 -q
  $ mkcommit a-H
  test-debug-phase: new rev 10:  x -> 1
  $ hgph
  @  10 draft a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 draft a-F - b740e3e5c05d
  | |
  | o  7 draft a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Pulling from bundle does not alter phases of changeset not present in the bundle

  $ hg bundle  --base 1 -r 6 -r 3 ../partial-bundle.hg
  5 changesets found
  $ hg pull ../partial-bundle.hg
  pulling from ../partial-bundle.hg
  searching for changes
  no changes found
  $ hgph
  @  10 draft a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 draft a-F - b740e3e5c05d
  | |
  | o  7 draft a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Pushing to Publish=False (unknown changeset)

  $ hg push ../mu -r b740e3e5c05d # a-F
  pushing to ../mu
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  test-debug-phase: new rev 7:  x -> 1
  test-debug-phase: new rev 8:  x -> 1
  $ hgph
  @  10 draft a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 draft a-F - b740e3e5c05d
  | |
  | o  7 draft a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

  $ cd ../mu
  $ hgph # again f54f1bb90ff3, d6bcb4f74035 and 145e75495359 stay draft,
  >      # not ancestor of -r
  o  8 draft a-F - b740e3e5c05d
  |
  o  7 draft a-E - e9f537e46dea
  |
  | o  6 draft n-B - 145e75495359
  | |
  | o  5 draft n-A - d6bcb4f74035
  | |
  o |  4 public a-D - b555f63b6063
  | |
  o |  3 public a-C - 54acac6f23ab
  | |
  | o  2 draft b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Pushing to Publish=True (unknown changeset)

  $ hg push ../beta -r b740e3e5c05d
  pushing to ../beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  test-debug-phase: new rev 5:  x -> 0
  test-debug-phase: new rev 6:  x -> 0
  test-debug-phase: move rev 7: 1 -> 0
  test-debug-phase: move rev 8: 1 -> 0
  $ hgph # again f54f1bb90ff3, d6bcb4f74035 and 145e75495359 stay draft,
  >      # not ancestor of -r
  o  8 public a-F - b740e3e5c05d
  |
  o  7 public a-E - e9f537e46dea
  |
  | o  6 draft n-B - 145e75495359
  | |
  | o  5 draft n-A - d6bcb4f74035
  | |
  o |  4 public a-D - b555f63b6063
  | |
  o |  3 public a-C - 54acac6f23ab
  | |
  | o  2 draft b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Pushing to Publish=True (common changeset)

  $ cd ../beta
  $ hg push ../alpha
  pushing to ../alpha
  searching for changes
  no changes found
  test-debug-phase: move rev 7: 1 -> 0
  test-debug-phase: move rev 8: 1 -> 0
  [1]
  $ hgph
  o  6 public a-F - b740e3e5c05d
  |
  o  5 public a-E - e9f537e46dea
  |
  o  4 public a-D - b555f63b6063
  |
  o  3 public a-C - 54acac6f23ab
  |
  | @  2 public b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ cd ../alpha
  $ hgph
  @  10 draft a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Pushing to Publish=False (common changeset that change phase + unknown one)

  $ hg push ../mu -r 967b449fbc94 -f
  pushing to ../mu
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  test-debug-phase: move rev 2: 1 -> 0
  test-debug-phase: move rev 5: 1 -> 0
  test-debug-phase: new rev 9:  x -> 1
  $ hgph
  @  10 draft a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ cd ../mu
  $ hgph # d6bcb4f74035 should have changed phase
  >      # 145e75495359 is still draft. not ancestor of -r
  o  9 draft a-H - 967b449fbc94
  |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 draft n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  | o  4 public a-D - b555f63b6063
  | |
  | o  3 public a-C - 54acac6f23ab
  | |
  o |  2 public b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  


Pushing to Publish=True (common changeset from publish=False)

(in mu)
  $ hg push ../alpha
  pushing to ../alpha
  searching for changes
  no changes found
  test-debug-phase: move rev 10: 1 -> 0
  test-debug-phase: move rev 6: 1 -> 0
  test-debug-phase: move rev 9: 1 -> 0
  [1]
  $ hgph
  o  9 public a-H - 967b449fbc94
  |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  | o  4 public a-D - b555f63b6063
  | |
  | o  3 public a-C - 54acac6f23ab
  | |
  o |  2 public b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hgph -R ../alpha # a-H should have been synced to 0
  @  10 public a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  


Bare push with next changeset and common changeset needing sync (issue3575)

(reset some stat on remote repo to avoid confusing other tests)

  $ hg -R ../alpha debugstrip --no-backup 967b449fbc94
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg phase --force --draft b740e3e5c05d 967b449fbc94
  test-debug-phase: move rev 8: 0 -> 1
  test-debug-phase: move rev 9: 0 -> 1
  $ hg push -fv ../alpha
  pushing to ../alpha
  searching for changes
  1 changesets found
  uncompressed size of bundle content:
       178 (changelog)
       165 (manifests)
       131  a-H
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  test-debug-phase: new rev 10:  x -> 0
  test-debug-phase: move rev 8: 1 -> 0
  test-debug-phase: move rev 9: 1 -> 0
  $ hgph
  o  9 public a-H - 967b449fbc94
  |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  | o  4 public a-D - b555f63b6063
  | |
  | o  3 public a-C - 54acac6f23ab
  | |
  o |  2 public b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

  $ hg -R ../alpha update 967b449fbc94 #for latter test consistency
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgph -R ../alpha
  @  10 public a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Discovery locally secret changeset on a remote repository:

- should make it non-secret

  $ cd ../alpha
  $ mkcommit A-secret --config phases.new-commit=2
  test-debug-phase: new rev 11:  x -> 2
  $ hgph
  @  11 secret A-secret - 435b5d83910c
  |
  o  10 public a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hg bundle --base 'parents(.)' -r . ../secret-bundle.hg
  1 changesets found
  $ hg -R ../mu unbundle ../secret-bundle.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 435b5d83910c
  test-debug-phase: new rev 10:  x -> 1
  $ hgph -R ../mu
  o  10 draft A-secret - 435b5d83910c
  |
  o  9 public a-H - 967b449fbc94
  |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  | o  4 public a-D - b555f63b6063
  | |
  | o  3 public a-C - 54acac6f23ab
  | |
  o |  2 public b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
  $ hg pull ../mu
  pulling from ../mu
  searching for changes
  no changes found
  test-debug-phase: move rev 11: 2 -> 1
  $ hgph
  @  11 draft A-secret - 435b5d83910c
  |
  o  10 public a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

pushing a locally public and draft changesets remotely secret should make them
appear on the remote side.

  $ hg -R ../mu phase --secret --force 967b449fbc94
  test-debug-phase: move rev 9: 0 -> 2
  test-debug-phase: move rev 10: 1 -> 2
  $ hg push -fr 435b5d83910c ../mu # because the push will create new visible head
  pushing to ../mu
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 2 files
  test-debug-phase: move rev 9: 2 -> 0
  test-debug-phase: move rev 10: 2 -> 1
  $ hgph -R ../mu
  o  10 draft A-secret - 435b5d83910c
  |
  o  9 public a-H - 967b449fbc94
  |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  | o  4 public a-D - b555f63b6063
  | |
  | o  3 public a-C - 54acac6f23ab
  | |
  o |  2 public b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

pull new changeset with common draft locally

  $ hg up -q 967b449fbc94 # create a new root for draft
  $ mkcommit 'alpha-more'
  test-debug-phase: new rev 12:  x -> 1
  $ hg push -fr . ../mu
  pushing to ../mu
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  test-debug-phase: new rev 11:  x -> 1
  $ cd ../mu
  $ hg phase --secret --force 1c5cfd894796
  test-debug-phase: move rev 11: 1 -> 2
  $ hg up -q 435b5d83910c
  $ mkcommit 'mu-more'
  test-debug-phase: new rev 12:  x -> 1
  $ cd ../alpha
  $ hg pull ../mu
  pulling from ../mu
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 5237fb433fc8
  test-debug-phase: new rev 13:  x -> 1
  $ hgph
  o  13 draft mu-more - 5237fb433fc8
  |
  | @  12 draft alpha-more - 1c5cfd894796
  | |
  o |  11 draft A-secret - 435b5d83910c
  |/
  o  10 public a-H - 967b449fbc94
  |
  | o  9 draft a-G - 3e27b6f1eee1
  | |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

Test that test are properly ignored on remote event when existing locally

  $ cd ..
  $ hg clone -qU -r b555f63b6063 -r f54f1bb90ff3 beta gamma
  test-debug-phase: new rev 0:  x -> 0
  test-debug-phase: new rev 1:  x -> 0
  test-debug-phase: new rev 2:  x -> 0
  test-debug-phase: new rev 3:  x -> 0
  test-debug-phase: new rev 4:  x -> 0

# pathological case are
#
# * secret remotely
# * known locally
# * repo have uncommon changeset

  $ hg -R beta phase --secret --force f54f1bb90ff3
  test-debug-phase: move rev 2: 0 -> 2
  $ hg -R gamma phase --draft --force f54f1bb90ff3
  test-debug-phase: move rev 2: 0 -> 1

  $ cd gamma
  $ hg pull ../beta
  pulling from ../beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  new changesets e9f537e46dea:b740e3e5c05d
  test-debug-phase: new rev 5:  x -> 0
  test-debug-phase: new rev 6:  x -> 0
  $ hg phase f54f1bb90ff3
  2: draft

same over the wire

  $ cd ../beta
  $ hg serve -p 0 --port-file $TESTTMP/.port -d --pid-file=../beta.pid -E ../beta-error.log
  $ HGPORT=`cat $TESTTMP/.port`
  $ cat ../beta.pid >> $DAEMON_PIDS
  $ cd ../gamma

  $ hg pull http://localhost:$HGPORT/ # bundle2+
  pulling from http://localhost:$HGPORT/ (glob)
  searching for changes
  no changes found
  $ hg phase f54f1bb90ff3
  2: draft

enforce bundle1

  $ hg pull http://localhost:$HGPORT/ --config devel.legacy.exchange=bundle1
  pulling from http://localhost:$HGPORT/ (glob)
  searching for changes
  no changes found
  $ hg phase f54f1bb90ff3
  2: draft

check that secret local on both side are not synced to public

  $ hg push -r b555f63b6063 http://localhost:$HGPORT/
  pushing to http://localhost:$HGPORT/ (glob)
  searching for changes
  no changes found
  [1]
  $ hg phase f54f1bb90ff3
  2: draft

put the changeset in the draft state again
(first test after this one expect to be able to copy)

  $ cd ..


Test Clone behavior

A. Clone without secret changeset

1.  cloning non-publishing repository
(Phase should be preserved)

# make sure there is no secret so we can use a copy clone

  $ hg -R mu phase --draft 'secret()'
  test-debug-phase: move rev 11: 2 -> 1

  $ hg clone -U mu Tau
  $ hgph -R Tau
  o  12 draft mu-more - 5237fb433fc8
  |
  | o  11 draft alpha-more - 1c5cfd894796
  | |
  o |  10 draft A-secret - 435b5d83910c
  |/
  o  9 public a-H - 967b449fbc94
  |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  | o  4 public a-D - b555f63b6063
  | |
  | o  3 public a-C - 54acac6f23ab
  | |
  o |  2 public b-A - f54f1bb90ff3
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  

2. cloning publishing repository

(everything should be public)

  $ hg clone -U alpha Upsilon
  $ hgph -R Upsilon
  o  13 public mu-more - 5237fb433fc8
  |
  | o  12 public alpha-more - 1c5cfd894796
  | |
  o |  11 public A-secret - 435b5d83910c
  |/
  o  10 public a-H - 967b449fbc94
  |
  | o  9 public a-G - 3e27b6f1eee1
  | |
  | o  8 public a-F - b740e3e5c05d
  | |
  | o  7 public a-E - e9f537e46dea
  | |
  +---o  6 public n-B - 145e75495359
  | |
  o |  5 public n-A - d6bcb4f74035
  | |
  o |  4 public b-A - f54f1bb90ff3
  | |
  | o  3 public a-D - b555f63b6063
  | |
  | o  2 public a-C - 54acac6f23ab
  |/
  o  1 public a-B - 548a3d25dbf0
  |
  o  0 public a-A - 054250a37db4
  
#if unix-permissions no-root

Pushing From an unlockable repo
--------------------------------
(issue3684)

Unability to lock the source repo should not prevent the push. It will prevent
the retrieval of remote phase during push. For example, pushing to a publishing
server won't turn changeset public.

1. Test that push is not prevented

  $ hg init Phi
  $ cd Upsilon
  $ chmod -R a-w .hg
  $ hg push ../Phi
  pushing to ../Phi
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 14 changesets with 14 changes to 14 files (+3 heads)
  test-debug-phase: new rev 0:  x -> 0
  test-debug-phase: new rev 1:  x -> 0
  test-debug-phase: new rev 2:  x -> 0
  test-debug-phase: new rev 3:  x -> 0
  test-debug-phase: new rev 4:  x -> 0
  test-debug-phase: new rev 5:  x -> 0
  test-debug-phase: new rev 6:  x -> 0
  test-debug-phase: new rev 7:  x -> 0
  test-debug-phase: new rev 8:  x -> 0
  test-debug-phase: new rev 9:  x -> 0
  test-debug-phase: new rev 10:  x -> 0
  test-debug-phase: new rev 11:  x -> 0
  test-debug-phase: new rev 12:  x -> 0
  test-debug-phase: new rev 13:  x -> 0
  $ chmod -R a+w .hg

2. Test that failed phases movement are reported

  $ hg phase --force --draft 3
  test-debug-phase: move rev 3: 0 -> 1
  test-debug-phase: move rev 7: 0 -> 1
  test-debug-phase: move rev 8: 0 -> 1
  test-debug-phase: move rev 9: 0 -> 1
  $ chmod -R a-w .hg
  $ hg push ../Phi
  pushing to ../Phi
  searching for changes
  no changes found
  cannot lock source repo, skipping local public phase update
  [1]
  $ chmod -R a+w .hg

  $ cd ..

#endif

Test that clone behaves like pull and doesn't publish changesets as plain push
does.  The conditional output accounts for changes in the conditional block
above.

#if unix-permissions no-root
  $ hg -R Upsilon phase -q --force --draft 2
  test-debug-phase: move rev 2: 0 -> 1
#else
  $ hg -R Upsilon phase -q --force --draft 2
  test-debug-phase: move rev 2: 0 -> 1
  test-debug-phase: move rev 3: 0 -> 1
  test-debug-phase: move rev 7: 0 -> 1
  test-debug-phase: move rev 8: 0 -> 1
  test-debug-phase: move rev 9: 0 -> 1
#endif

  $ hg clone -q Upsilon Pi -r 7
  test-debug-phase: new rev 0:  x -> 0
  test-debug-phase: new rev 1:  x -> 0
  test-debug-phase: new rev 2:  x -> 0
  test-debug-phase: new rev 3:  x -> 0
  test-debug-phase: new rev 4:  x -> 0
  $ hgph Upsilon -r 'min(draft())'
  o  2 draft a-C - 54acac6f23ab
  |
  ~

  $ hg -R Upsilon push Pi -r 7
  pushing to Pi
  searching for changes
  no changes found
  test-debug-phase: move rev 2: 1 -> 0
  test-debug-phase: move rev 3: 1 -> 0
  test-debug-phase: move rev 7: 1 -> 0
  [1]
  $ hgph Upsilon -r 'min(draft())'
  o  8 draft a-F - b740e3e5c05d
  |
  ~

  $ hg -R Upsilon push Pi -r 8
  pushing to Pi
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  test-debug-phase: new rev 5:  x -> 0
  test-debug-phase: move rev 8: 1 -> 0

  $ hgph Upsilon -r 'min(draft())'
  o  9 draft a-G - 3e27b6f1eee1
  |
  ~

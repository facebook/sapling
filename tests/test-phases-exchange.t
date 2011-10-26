  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > EOF
  $ alias hgph='hg log --template "{rev} {phase} {desc} - {node|short}\n"'

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

  $ hg init alpha
  $ cd alpha
  $ mkcommit a-A
  $ mkcommit a-B
  $ mkcommit a-C
  $ mkcommit a-D
  $ hgph
  3 1 a-D - b555f63b6063
  2 1 a-C - 54acac6f23ab
  1 1 a-B - 548a3d25dbf0
  0 1 a-A - 054250a37db4

  $ hg init ../beta
  $ hg push -r 1 ../beta
  pushing to ../beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  $ hgph
  3 1 a-D - b555f63b6063
  2 1 a-C - 54acac6f23ab
  1 0 a-B - 548a3d25dbf0
  0 0 a-A - 054250a37db4

  $ cd ../beta
  $ hgph
  1 0 a-B - 548a3d25dbf0
  0 0 a-A - 054250a37db4
  $ hg up -q
  $ mkcommit b-A
  $ hgph
  2 1 b-A - f54f1bb90ff3
  1 0 a-B - 548a3d25dbf0
  0 0 a-A - 054250a37db4
  $ hg pull ../alpha
  pulling from ../alpha
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hgph
  4 0 a-D - b555f63b6063
  3 0 a-C - 54acac6f23ab
  2 1 b-A - f54f1bb90ff3
  1 0 a-B - 548a3d25dbf0
  0 0 a-A - 054250a37db4

pull did not updated ../alpha state.
push from alpha to beta should update phase even if nothing is transfered

  $ cd ../alpha
  $ hgph # not updated by remote pull
  3 1 a-D - b555f63b6063
  2 1 a-C - 54acac6f23ab
  1 0 a-B - 548a3d25dbf0
  0 0 a-A - 054250a37db4
  $ hg push ../beta
  pushing to ../beta
  searching for changes
  no changes found
  $ hgph
  3 0 a-D - b555f63b6063
  2 0 a-C - 54acac6f23ab
  1 0 a-B - 548a3d25dbf0
  0 0 a-A - 054250a37db4

update must update phase of common changeset too

  $ hg pull ../beta # getting b-A
  pulling from ../beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ cd ../beta
  $ hgph # not updated by remote pull
  4 0 a-D - b555f63b6063
  3 0 a-C - 54acac6f23ab
  2 1 b-A - f54f1bb90ff3
  1 0 a-B - 548a3d25dbf0
  0 0 a-A - 054250a37db4
  $ hg pull ../alpha
  pulling from ../alpha
  searching for changes
  no changes found
  $ hgph
  4 0 a-D - b555f63b6063
  3 0 a-C - 54acac6f23ab
  2 0 b-A - f54f1bb90ff3
  1 0 a-B - 548a3d25dbf0
  0 0 a-A - 054250a37db4

Publish configuration option
----------------------------

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
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hgph
  4 1 a-D - b555f63b6063
  3 1 a-C - 54acac6f23ab
  2 1 b-A - f54f1bb90ff3
  1 1 a-B - 548a3d25dbf0
  0 1 a-A - 054250a37db4
  $ cd ..


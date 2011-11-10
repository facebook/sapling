  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > EOF
  $ alias hgph='hg log --template "{rev} {phase} {desc}\n"'

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
  3 1 a-D
  2 1 a-C
  1 1 a-B
  0 1 a-A

  $ hg init ../beta
  $ hg push -r 1 ../beta
  pushing to ../beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  $ hgph
  3 1 a-D
  2 1 a-C
  1 0 a-B
  0 0 a-A

  $ cd ../beta
  $ hgph
  1 0 a-B
  0 0 a-A
  $ hg up -q
  $ mkcommit b-A
  $ hgph
  2 1 b-A
  1 0 a-B
  0 0 a-A
  $ hg pull ../alpha
  pulling from ../alpha
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hgph
  4 0 a-D
  3 0 a-C
  2 1 b-A
  1 0 a-B
  0 0 a-A

pull did not updated ../alpha state.
push from alpha to beta should update phase even if nothing is transfered

  $ cd ../alpha
  $ hgph # not updated by remote pull
  3 1 a-D
  2 1 a-C
  1 0 a-B
  0 0 a-A
  $ hg push ../beta
  pushing to ../beta
  searching for changes
  no changes found
  $ hgph
  3 0 a-D
  2 0 a-C
  1 0 a-B
  0 0 a-A

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
  4 0 a-D
  3 0 a-C
  2 1 b-A
  1 0 a-B
  0 0 a-A
  $ hg pull ../alpha
  pulling from ../alpha
  searching for changes
  no changes found
  $ hgph
  4 0 a-D
  3 0 a-C
  2 0 b-A
  1 0 a-B
  0 0 a-A



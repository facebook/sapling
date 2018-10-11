
  $ mkcommit() {
  >  echo "$1" > "$1"
  >  hg add "$1"
  >  hg ci -m "$1"
  > }

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > fastpartialmatch=
  > histedit=
  > strip=
  > [experimental]
  > evolution=createmarkers
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF

  $ hg init repo
  $ cd repo
  $ mkcommit firstcommit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 000000000000
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg debugrebuildpartialindex
  $ hg debugcheckpartialindex
  $ mkcommit first
  $ hg debugcheckpartialindex
  $ hg prune -q .
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg debugcheckpartialindex

Try histedit
  $ mkcommit second
  $ mkcommit third
  $ mkcommit fourth
  $ hg log --graph
  @  changeset:   4:d5e85d22a345
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     fourth
  |
  o  changeset:   3:a5b4be173947
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     third
  |
  o  changeset:   2:be6305906393
     parent:      -1:000000000000
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     second
  
  $ hg histedit --commands - <<EOF
  > pick d5e85d22a345 3 fourth
  > pick a5b4be173947 2 third
  > pick be6305906393 1 second
  > EOF
  $ hg debugcheckpartialindex

Made commit, then amend it. Check partial index
  $ mkcommit toamend
  $ echo 1 > toamend
  $ hg commit --amend -m amended
  $ hg debugcheckpartialindex

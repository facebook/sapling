
  $ mkcommit() {
  >  echo "$1" > "$1"
  >  hg add "$1"
  >  hg ci -m "$1"
  > }

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastpartialmatch=$TESTDIR/../hgext3rd/fastpartialmatch.py
  > strip=
  > histedit=
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > [experimental]
  > evolution=createmarkers
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF

  $ hg init repo
  $ cd repo
  $ mkcommit firstcommit
  $ hg prune .
  advice: 'hg hide' provides a better UI for hiding commits
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 000000000000
  1 changesets pruned
  $ hg debugrebuildpartialindex
  $ hg debugcheckpartialindex
  $ mkcommit first
  $ hg debugcheckpartialindex
  $ hg prune -q .
  advice: 'hg hide' provides a better UI for hiding commits
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

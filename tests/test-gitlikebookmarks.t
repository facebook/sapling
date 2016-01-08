  $ $PYTHON -c 'import evolve' || exit 80
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > gitlikebookmarks=$TESTDIR/../gitlikebookmarks.py
  > rebase=
  > evolve=
  > inhibit=
  > directaccess=
  > [experimental]
  > evolution=createmarkers
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    [ -z $2 ] || echo "Differential Revision: https://phabricator.fb.com/D$2" >> msg
  >    hg ci -l msg
  > }

  $ hg init repo
  $ cd repo
  $ mkcommit _a
  $ mkcommit _b
  $ hg up "desc(_a)"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit _c
  created new head
  $ hg book first_bookmark
  $ hg book second_bookmark
  $ mkcommit _d
  $ hg log -G
  @  changeset:   3:3d974c2713ca
  |  bookmark:    second_bookmark
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add _d
  |
  o  changeset:   2:1db8f42448cc
  |  bookmark:    first_bookmark
  |  parent:      0:135f39f4bd78
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add _c
  |
  | o  changeset:   1:37445b16603b
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add _b
  |
  o  changeset:   0:135f39f4bd78
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add _a
  
  $ hg bookmarks
     first_bookmark            2:1db8f42448cc
   * second_bookmark           3:3d974c2713ca
  $ hg rebase -s "desc(_c)" -d "desc(_b)" -x
  rebasing 2:1db8f42448cc "add _c" (first_bookmark)
  rebasing 3:3d974c2713ca "add _d" (tip second_bookmark)
  $ hg log -G
  @  changeset:   5:15f1a0e15dd8
  |  bookmark:    second_bookmark
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add _d
  |
  o  changeset:   4:da71b5c6dbab
  |  parent:      1:37445b16603b
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add _c
  |
  | o  changeset:   2:1db8f42448cc
  | |  bookmark:    first_bookmark
  | |  parent:      0:135f39f4bd78
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add _c
  | |
  o |  changeset:   1:37445b16603b
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add _b
  |
  o  changeset:   0:135f39f4bd78
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add _a
  
  $ hg bookmarks
     first_bookmark            2:1db8f42448cc
   * second_bookmark           5:15f1a0e15dd8

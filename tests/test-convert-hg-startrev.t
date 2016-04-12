
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert =
  > [convert]
  > hg.saverev = yes
  > EOF

  $ glog()
  > {
  >     hg -R "$1" log -G --template '{rev} "{desc}" files: {files}\n'
  > }

  $ hg init source
  $ cd source

  $ echo a > a
  $ echo b > b
  $ echo f > f
  $ hg ci -d '0 0' -qAm '0: add a b f'
  $ echo c > c
  $ hg move f d
  $ hg ci -d '1 0' -qAm '1: add c, move f to d'
  $ hg copy a e
  $ echo b >> b
  $ hg ci -d '2 0' -qAm '2: copy e from a, change b'
  $ hg up -C 0
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo a >> a
  $ hg ci -d '3 0' -qAm '3: change a'
  $ hg merge
  merging a and e to e
  3 files updated, 1 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -d '4 0' -qAm '4: merge 2 and 3'
  $ echo a >> a
  $ hg ci -d '5 0' -qAm '5: change a'
  $ cd ..

Convert from null revision

  $ hg convert --config convert.hg.startrev=null source full
  initializing destination full repository
  scanning source...
  sorting...
  converting...
  5 0: add a b f
  4 1: add c, move f to d
  3 2: copy e from a, change b
  2 3: change a
  1 4: merge 2 and 3
  0 5: change a

  $ glog full
  o  5 "5: change a" files: a
  |
  o    4 "4: merge 2 and 3" files: e f
  |\
  | o  3 "3: change a" files: a
  | |
  o |  2 "2: copy e from a, change b" files: b e
  | |
  o |  1 "1: add c, move f to d" files: c d f
  |/
  o  0 "0: add a b f" files: a b f
  
  $ rm -Rf full

Convert from zero revision

  $ hg convert --config convert.hg.startrev=0 source full
  initializing destination full repository
  scanning source...
  sorting...
  converting...
  5 0: add a b f
  4 1: add c, move f to d
  3 2: copy e from a, change b
  2 3: change a
  1 4: merge 2 and 3
  0 5: change a

  $ glog full
  o  5 "5: change a" files: a
  |
  o    4 "4: merge 2 and 3" files: e f
  |\
  | o  3 "3: change a" files: a
  | |
  o |  2 "2: copy e from a, change b" files: b e
  | |
  o |  1 "1: add c, move f to d" files: c d f
  |/
  o  0 "0: add a b f" files: a b f
  
Convert from merge parent

  $ hg convert --config convert.hg.startrev=1 source conv1
  initializing destination conv1 repository
  scanning source...
  sorting...
  converting...
  3 1: add c, move f to d
  2 2: copy e from a, change b
  1 4: merge 2 and 3
  0 5: change a

  $ glog conv1
  o  3 "5: change a" files: a
  |
  o  2 "4: merge 2 and 3" files: a e
  |
  o  1 "2: copy e from a, change b" files: b e
  |
  o  0 "1: add c, move f to d" files: a b c d
  
  $ cd conv1
  $ hg up -q

Check copy preservation

  $ hg st -C --change 2 e
  M e
  $ hg st -C --change 1 e
  A e
    a
  $ hg st -C --change 0 a
  A a

(It seems like a bug in log that the following doesn't show rev 1.)

  $ hg log --follow --copies e
  changeset:   2:82bbac3d2cf4
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     4: merge 2 and 3
  
  changeset:   0:23c3be426dce
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     1: add c, move f to d
  
Check copy removal on missing parent

  $ hg log --follow --copies d
  changeset:   0:23c3be426dce
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     1: add c, move f to d
  
  $ hg cat -r tip a b
  a
  a
  a
  b
  b
  $ hg -q verify
  $ cd ..

Convert from merge

  $ hg convert --config convert.hg.startrev=4 source conv4
  initializing destination conv4 repository
  scanning source...
  sorting...
  converting...
  1 4: merge 2 and 3
  0 5: change a
  $ glog conv4
  o  1 "5: change a" files: a
  |
  o  0 "4: merge 2 and 3" files: a b c d e
  
  $ cd conv4
  $ hg up -C
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg cat -r tip a b
  a
  a
  a
  b
  b
  $ hg -q verify
  $ cd ..

Convert from revset in convert.hg.revs

  $ hg convert --config convert.hg.revs='3:4+0' source revsetrepo
  initializing destination revsetrepo repository
  scanning source...
  sorting...
  converting...
  2 0: add a b f
  1 3: change a
  0 4: merge 2 and 3

  $ glog revsetrepo
  o  2 "4: merge 2 and 3" files: b c d e f
  |
  o  1 "3: change a" files: a
  |
  o  0 "0: add a b f" files: a b f
  
Convert from specified revs

  $ hg convert --rev 3 --rev 2 source multiplerevs
  initializing destination multiplerevs repository
  scanning source...
  sorting...
  converting...
  3 0: add a b f
  2 1: add c, move f to d
  1 2: copy e from a, change b
  0 3: change a
  $ glog multiplerevs
  o  3 "3: change a" files: a
  |
  | o  2 "2: copy e from a, change b" files: b e
  | |
  | o  1 "1: add c, move f to d" files: c d f
  |/
  o  0 "0: add a b f" files: a b f
  
Convert in multiple steps that doesn't overlap - the link to the parent is
preserved anyway

  $ hg convert --config convert.hg.revs=::1 source multistep
  initializing destination multistep repository
  scanning source...
  sorting...
  converting...
  1 0: add a b f
  0 1: add c, move f to d
  $ hg convert --config convert.hg.revs=2 source multistep
  scanning source...
  sorting...
  converting...
  0 2: copy e from a, change b
  $ glog multistep
  o  2 "2: copy e from a, change b" files: b e
  |
  o  1 "1: add c, move f to d" files: c d f
  |
  o  0 "0: add a b f" files: a b f
  

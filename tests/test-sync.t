  $ . "$TESTDIR/library.sh"

# Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ initserver master masterrepo
  $ cd master
  $ hg log
  $ hg pull -q ../client

  $ cd ..

# Verify new masters see the same commit

  $ initserver master2 masterrepo
  $ cd master2
  $ hg log
  changeset:   0:b292c1e3311f
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  

# Push new commit to one master, verify it shows up in the other

  $ cd ../client
  $ echo y > y
  $ hg commit -qAm y
  $ hg push -q ssh://user@dummy/master
  $ cd ../master2
  $ hg log -l 1
  changeset:   1:d34c38483be9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
# Push a bookmark to one master, verify in the other

  $ cd ../client
  $ hg book mybook
  $ hg push ssh://user@dummy/master -B mybook
  pushing to ssh://user@dummy/master
  searching for changes
  no changes found
  exporting bookmark mybook
  [1]
  $ cd ../master2
  $ hg book
     mybook                    1:d34c38483be9

# Pull commit and bookmark to one master, verify in the other

  $ cd ../client
  $ hg up mybook
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark mybook)
  $ echo z > z
  $ hg commit -qAm z
  $ cd ../master
  $ hg pull -q ssh://user@dummy/client
  $ cd ../master2
  $ hg log -l 1
  changeset:   2:d47967ce72a5
  bookmark:    mybook
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     z
  
# Verify that --forcesync works

  $ cd ../master
  $ echo '[hooks]' >> $HGRCPATH
  $ echo 'presyncdb=$TESTTMP/hook.sh' >> $HGRCPATH
  $ echo 'sleep 1' > $TESTTMP/hook.sh
  $ chmod a+x $TESTTMP/hook.sh
  $ cd ../client
  $ echo a > a
  $ hg commit -qAm a
  $ hg push -q ssh://user@dummy/master2
  $ cd ../master
  $ hg log -l 1 --template '{rev} {desc}\n' &
  $ hg log -l 1 --template '{rev} {desc}\n' --forcesync
  waiting for lock on working directory of $TESTTMP/master held by * (glob)
  3 a
  got lock after ? seconds (glob)
  3 a

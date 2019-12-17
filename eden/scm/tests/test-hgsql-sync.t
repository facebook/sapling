#chg-compatible

#chg-compatible

#testcases case-innodb case-rocksdb

#if case-rocksdb
  $ DBENGINE=rocksdb
#else
  $ DBENGINE=innodb
#endif

  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!
  $ setconfig hgsql.verbose=1

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
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  [hgsql] held lock for * seconds (read 5 rows; write 5 rows) (glob)

  $ cd ..

# Verify new masters see the same commit

  $ initserver master2 masterrepo
  $ cd master2
  $ hg log
  [hgsql] getting 1 commits from database
  changeset:   0:b292c1e3311f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  

# Push new commit to one master, verify it shows up in the other

  $ cd ../client
  $ echo y > y

# (also test for a file containing a single null byte)
  $ printf '\0' > nullbyte
  $ f --hexdump nullbyte
  nullbyte:
  0000: 00                                              |.|

  $ hg commit -qAm y
  $ hg push -q ssh://user@dummy/master
  $ cd ../master2
  $ hg log -l 1
  [hgsql] getting 1 commits from database
  changeset:   1:b62091368546
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
  $ hg cat -r 1 nullbyte | f --hexdump -
  
  0000: 00                                              |.|

# Push a bookmark to one master, verify in the other

  $ cd ../client
  $ hg book mybook
  $ hg push ssh://user@dummy/master -B mybook
  pushing to ssh://user@dummy/master
  searching for changes
  no changes found
  remote: [hgsql] got lock after * seconds (read 1 rows) (glob)
  remote: [hgsql] held lock for * seconds (read 5 rows; write 1 rows) (glob)
  exporting bookmark mybook
  [1]
  $ cd ../master2
  $ hg book
  [hgsql] getting 0 commits from database
     mybook                    1:b62091368546

# Pull commit and bookmark to one master, verify in the other

  $ cd ../client
  $ hg up mybook
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo z > z
  $ hg commit -qAm z
  $ cd ../master
  $ hg pull -q ssh://user@dummy/client
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  [hgsql] held lock for * seconds (read 8 rows; write 9 rows) (glob)
  $ cd ../master2
  $ hg log -l 1
  [hgsql] getting 1 commits from database
  changeset:   2:f3a7cb746fa9
  bookmark:    mybook
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     z
  
# Delete a bookmark in one, verify in the other

  $ hg book book1
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  [hgsql] held lock for * seconds (read 6 rows; write 1 rows) (glob)
  $ cd ../master
  $ hg book
  [hgsql] getting 0 commits from database
     book1                     -1:000000000000
     mybook                    2:f3a7cb746fa9
  $ hg book -d book1
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  [hgsql] held lock for * seconds (read 7 rows; write 1 rows) (glob)
  $ cd ../master2
  $ hg book
  [hgsql] getting 0 commits from database
     mybook                    2:f3a7cb746fa9

# Verify that --forcesync works

  $ cd ../
  $ cp $HGRCPATH backup.hgrc
  $ cd master
  $ echo '[hooks]' >> $HGRCPATH
  $ echo 'presyncdb=$TESTTMP/hook.sh' >> $HGRCPATH
  $ echo 'sleep 2' > $TESTTMP/hook.sh
  $ chmod a+x $TESTTMP/hook.sh
  $ cd ../client
  $ echo a > a
  $ hg commit -qAm a
  $ hg push -q ssh://user@dummy/master2
  $ cd ../master
  $ hg log -l 1 --template '{rev} {desc}\n' &
  $ sleep 1
  [hgsql] getting 1 commits from database
  $ hg log -l 1 --template '{rev} {desc}\n' --forcesync
  waiting for lock on working directory of $TESTTMP/master held by * (glob)
  (hint: run 'hg debugprocesstree *' to see related processes) (glob)
  3 a
  got lock after * seconds (glob)
  3 a
  $ cd ..
  $ cp backup.hgrc $HGRCPATH

# Update one bookmark but not the other
  $ cat >> $TESTTMP/inspectsql.py <<EOF
  > import os, sys
  > from edenscm.mercurial import demandimport, extensions
  > with demandimport.deactivated():
  >     import mysql.connector
  > watchstrings = os.environ.get("INSPECTSQL")
  > if watchstrings:
  >     watchstrings = watchstrings.split(',')
  > def printsql(orig, *args, **kwargs):
  >     if not watchstrings or any(s for s in watchstrings if s in args[1]):
  >         print >> sys.stderr, args[1] % args[2]
  >     return orig(*args, **kwargs)
  > extensions.wrapfunction(mysql.connector.cursor.MySQLCursor, "execute", printsql)
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > inspectsql=$TESTTMP/inspectsql.py
  > EOF
  $ cd master
  $ INSPECTSQL=DELETE,INSERT hg book mybook2
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  INSERT INTO revision_references(repo, namespace, name, value) VALUES (masterrepo, 'bookmarks', mybook2, 0000000000000000000000000000000000000000)
  INSERT INTO revision_references(repo, namespace, name, value) VALUES(masterrepo, 'tip', 'tip', 3) ON DUPLICATE KEY UPDATE value=3
  [hgsql] held lock for * seconds (read 6 rows; write 1 rows) (glob)
  $ cd ..
  $ cp backup.hgrc $HGRCPATH

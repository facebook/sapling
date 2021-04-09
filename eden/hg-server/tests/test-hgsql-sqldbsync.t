#require py2
#chg-compatible

#testcases case-innodb case-rocksdb

#if case-rocksdb
  $ DBENGINE=rocksdb
#else
  $ DBENGINE=innodb
#endif

  $ . "$TESTDIR/hgsql/library.sh"
  $ disable treemanifest
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
  commit:      b292c1e3311f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  

# Push new commit to one master, verify it shows up in the other

  $ cd ../client
  $ echo y > y

  $ hg commit -qAm y
  $ hg push -q ssh://user@dummy/master
  $ cd ../master2
  $ hg log -r tip --forcesync
  [hgsql] getting 1 commits from database
  commit:      d34c38483be9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
  $ hg log -r tip
  commit:      d34c38483be9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  

# Now strip the latest commit, sync from master (that should fail), but sync from replica 
# should succeed
  $ cd ../master
  $ hg sqlstrip --i-know-what-i-am-doing 1
  *** YOU ARE ABOUT TO DELETE HISTORY (MANDATORY 5 SECOND WAIT) ***
  stripping locally
  stripping from the database
  deleting old references
  deleting revision data
  $ cd ../master2
  $ hg log -r tip --forcesync 2>&1 | grep CorruptionException
  CorruptionException: tip doesn't match after sync (self: 1, fetchend: 0)
  $ DB="$(hg config hgsql.database --config hgsql.initialsync=False)"
  $ hg log -r tip --syncfromreplica --config hgsql.replicadatabase="$DB"
  commit:      d34c38483be9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  

  $ hg log -r tip --forcesync --syncfromreplica --config hgsql.replicadatabase="$DB"
  commit:      d34c38483be9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  

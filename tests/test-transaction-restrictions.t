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

# Verify commit fails in a db repo

  $ echo y > y
  $ hg commit -qAm y
  transaction abort!
  rollback completed
  abort: invalid repo change - only hg push and pull are allowed
  [255]

  $ hg log -l 1
  changeset:   0:b292c1e3311f
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  

# Verify strip fails in a db repo

  $ hg strip --config extensions.strip= -r tip
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/b292c1e3311f-backup.hg
  transaction abort!
  rollback completed
  strip failed, full bundle stored in '$TESTTMP/master/.hg/strip-backup/b292c1e3311f-backup.hg'
  abort: invalid repo change - only hg push and pull are allowed
  [255]

  $ hg log -l 1
  changeset:   0:b292c1e3311f
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  

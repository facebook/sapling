#chg-compatible

no-check-code

  $ disable treemanifest
  $ . "$TESTDIR/hgsql/library.sh"

# Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ initserver master masterrepo
  $ cat >> master/.hg/hgrc <<EOF
  > [hgsql]
  > profileoutput=$TESTTMP/
  > EOF
  $ hg -R master log
  $ hg -R master pull -q client

  $ initserver master2 masterrepo
  $ hg -R master2 log --template '{node}\n'
  b292c1e3311fd0f13ae83b409caae4a6d1fb348c

# Verify that reads are not blocked by the repo syncing

  $ cd client
  $ echo y > y
  $ hg commit -qAm y
  $ hg push -q ssh://user@dummy/master
  $ cd ..

  $ cd master2
  $ printf "[hooks]\npresyncdb.sleep = sleep 5\n" >> .hg/hgrc
  $ hg log -l 2 --template "first:{node}\n" --debug &
  $ sleep 3
  syncing with mysql
  getting 1 commits from database
  running hook presyncdb.sleep: sleep 5
  $ hg log -l 2 --template "second:{node}\n" --debug
  locker is still running (full unique id: '*') (glob)
  skipping database sync because another process is already syncing
  second:b292c1e3311fd0f13ae83b409caae4a6d1fb348c
  $ sleep 5
  first:d34c38483be9d08f205eaae60c380a29b48e0189
  first:b292c1e3311fd0f13ae83b409caae4a6d1fb348c
  $ sed -i '/hooks/d' .hg/hgrc
  $ sed -i '/sleep/d' .hg/hgrc

# Check hgsql.synclimit

  $ hg log -r . -T '.\n' --debug --config hgsql.synclimit=100000
  skipping database sync due to rate limit
  .
  $ cd ..

# Verify simultaneous pushes to different heads succeeds

  $ printf "[hooks]\npre-changegroup.sleep = sleep 2\n" >> master/.hg/hgrc
  $ initclient client2
  $ hg pull -q -R client2 ssh://user@dummy/master

  $ cd client
  $ hg up -q 1
  $ echo z1 > z1
  $ hg commit -qAm z1
  $ cd ../client2
  $ hg up -q 1
  $ echo z2 > z2
  $ hg commit -qAm z2
  $ cd ..

  $ hg push -R client -q ssh://user@dummy/master &
  $ sleep 0.2
  $ hg push -R client2 -q -f ssh://user@dummy/master2
  $ hg log -R master -G --template '{desc}\n'
  o  z2
  │
  │ o  z1
  ├─╯
  o  y
  │
  o  x
  
  $ sed -i '/hooks/d' master/.hg/hgrc
  $ sed -i '/sleep/d' master/.hg/hgrc

  $ . "$TESTDIR/hgsql/library.sh"

# Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ initserver master masterrepo

# Test with stat profiler
  $ cat >> master/.hg/hgrc <<EOF
  > [hgsql]
  > profiler=stat
  > profileoutput=$TESTTMP/
  > EOF

  $ cd client
  $ hg push -q ssh://user@dummy/master
  $ cat $TESTTMP/hgsql-profile* | grep "Total Elapsed Time"
  Total Elapsed Time: * (glob)
  $ rm -f $TESTTMP/hgsql-profile*

  $ cd ..

# Test with ls profiler
  $ cat >> master/.hg/hgrc <<EOF
  > [hgsql]
  > profiler=ls
  > profileoutput=$TESTTMP/
  > EOF

  $ cd client
  $ echo x >> x
  $ hg commit -qAm x
  $ hg push -q ../master
  $ cat $TESTTMP/hgsql-profile* | grep "Total Elapsed Time"
  Total Elapsed Time: * (glob)
  $ rm -f $TESTTMP/hgsql-profile*

  $ cd ..

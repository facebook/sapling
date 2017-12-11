Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ cat > $TESTTMP/pretxnchangegroup.sh << EOF
  > #!/bin/sh
  > env | egrep "^HG_USERVAR_(DEBUG|BYPASS_REVIEW)" | sort
  > exit 0
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [hooks]
  > pretxnchangegroup = sh $TESTTMP/pretxnchangegroup.sh
  > [experimental]
  > bundle2-exp = true
  > EOF

  $ hg init repo
  $ hg clone -q repo child
  $ cd child

Test pushing vars to repo with pushvars.server not set

  $ echo b > a
  $ hg commit -Aqm a
  $ hg push --pushvars "DEBUG=1" --pushvars "BYPASS_REVIEW=true"
  pushing to $TESTTMP/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Setting pushvars.sever = true and then pushing.

  $ echo [push] >> $HGRCPATH
  $ echo "pushvars.server = true" >> $HGRCPATH
  $ echo b >> a
  $ hg commit -Aqm a
  $ hg push --pushvars "DEBUG=1" --pushvars "BYPASS_REVIEW=true"
  pushing to $TESTTMP/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  HG_USERVAR_BYPASS_REVIEW=true
  HG_USERVAR_DEBUG=1

Test pushing var with empty right-hand side

  $ echo b >> a
  $ hg commit -Aqm a
  $ hg push --pushvars "DEBUG="
  pushing to $TESTTMP/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  HG_USERVAR_DEBUG=

Test pushing bad vars

  $ echo b >> a
  $ hg commit -Aqm b
  $ hg push --pushvars "DEBUG"
  pushing to $TESTTMP/repo
  searching for changes
  abort: unable to parse variable 'DEBUG', should follow 'KEY=VALUE' or 'KEY=' format
  [255]

  $ extpath=$(dirname $TESTDIR)
  $ cat > $TESTTMP/pretxnchangegroup.sh << EOF
  > #!/bin/bash
  > env | grep -E "^HG_USERVAR_DEBUG"
  > env | grep -E "^HG_USERVAR_BYPASS_REVIEW"
  > exit 0
  > EOF
  $ chmod +x $TESTTMP/pretxnchangegroup.sh
  $ cp $extpath/pushvars.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > pushvars=$TESTTMP/pushvars.py
  > [hooks]
  > pretxnchangegroup = $TESTTMP/pretxnchangegroup.sh
  > [experimental]
  > bundle2-exp = true
  > EOF

  $ hg init repo
  $ hg clone -q repo child
  $ cd child

Test pushing vars to repo

  $ echo b > a
  $ hg commit -Aqm a
  $ hg push --pushvars "DEBUG=1" --pushvars "BYPASS_REVIEW=true"
  pushing to $TESTTMP/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  HG_USERVAR_DEBUG=1
  HG_USERVAR_BYPASS_REVIEW=true

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
  abort: passed in variable needs to be of form var= or var=val. Instead, this was given "DEBUG"
  [255]

  $ . helpers-usechg.sh

Setup

  $ cat > $TESTTMP/pretxnchangegroup.sh << EOF
  > #!/bin/bash
  > env | egrep "^HG_USERVAR_(DEBUG|BYPASS_REVIEW)" | sort
  > exit 0
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [hooks]
  > pretxnchangegroup = bash $TESTTMP/pretxnchangegroup.sh
  > EOF

  $ hg init repo
  $ hg clone -q repo child
  $ cd child

Test pushing vars to repo with pushvars.server explicitly disabled

  $ cd ../repo
  $ setconfig push.pushvars.server=False
  $ cd ../child
  $ echo b > a
  $ hg commit -Aqm a
  $ hg push --pushvars "DEBUG=1" --pushvars "BYPASS_REVIEW=true" --config push.pushvars.server=False
  pushing to $TESTTMP/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Setting pushvars.sever = true and then pushing.

  $ cd ../repo
  $ setconfig push.pushvars.server=True
  $ cd ../child
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

Test Python hooks

  $ cat >> $TESTTMP/pyhook.py << EOF
  > def hook(ui, repo, hooktype, **kwargs):
  >     for k, v in sorted(kwargs.items()):
  >         if "USERVAR" in k:
  >             ui.write("Got pushvar: %s=%s\n" % (k, v))
  > EOF

  $ cp $HGRCPATH $TESTTMP/hgrc.bak
  $ cat >> $HGRCPATH << EOF
  > [hooks]
  > pretxnchangegroup.pyhook = python:$TESTTMP/pyhook.py:hook
  > EOF

  $ hg push --pushvars "A=1" --pushvars "B=2"
  pushing to $TESTTMP/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  Got pushvar: USERVAR_A=1
  Got pushvar: USERVAR_B=2
  $ cp $TESTTMP/hgrc.bak $HGRCPATH

Test pushvars for enforcing push reasons
  $ cat >> .hg/hgrc <<EOF
  > [push]
  > requirereason=True
  > requirereasonmsg="Because I said so"
  > EOF
  $ echo c >> a
  $ hg commit -Aqm c
  $ hg push
  pushing to $TESTTMP/repo
  abort: "Because I said so"
  (use `--pushvars PUSH_REASON='because ...'`)
  [255]
  $ hg push --pushvars PUSH_REASON="I want to"
  pushing to $TESTTMP/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg blackbox --pattern '{"legacy_log": {"service": "pushreason"}}'
  * [legacy][pushreason] bypassing push block with reason: I want to (glob)
  $ cp $TESTTMP/hgrc.bak $HGRCPATH

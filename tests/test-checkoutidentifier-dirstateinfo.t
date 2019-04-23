  $ enable amend
  $ setconfig extensions.extralog=$TESTDIR/extralog.py
  $ setconfig extensions.staleidentifiers=$TESTDIR/stableidentifiers.py
  $ setconfig extralog.events=dirstate_info extralog.keywords=true

  $ newrepo

  $ echo base > base
  $ hg commit -Am base
  dirstate_info (precheckoutidentifier= prewdirparent1= prewdirparent2=)
  adding base
  dirstate_info (postcheckoutidentifier=0000000000000000 postwdirparent1=d20a80d4def38df63a4b330b7fb688f3d4cae1e3 postwdirparent2=)

  $ hg debugcheckoutidentifier
  dirstate_info (precheckoutidentifier=0000000000000000 prewdirparent1=d20a80d4def38df63a4b330b7fb688f3d4cae1e3 prewdirparent2=)
  0000000000000000
  dirstate_info (postcheckoutidentifier=0000000000000000 postwdirparent1=d20a80d4def38df63a4b330b7fb688f3d4cae1e3 postwdirparent2=)

  $ echo 1 > 1
  $ hg commit -Am 1
  dirstate_info (precheckoutidentifier=0000000000000000 prewdirparent1=d20a80d4def38df63a4b330b7fb688f3d4cae1e3 prewdirparent2=)
  adding 1
  dirstate_info (postcheckoutidentifier=0000000000000001 postwdirparent1=f0161ad23099c690115006c21e96f780f5d740b6 postwdirparent2=)

  $ hg debugcheckoutidentifier
  dirstate_info (precheckoutidentifier=0000000000000001 prewdirparent1=f0161ad23099c690115006c21e96f780f5d740b6 prewdirparent2=)
  0000000000000001
  dirstate_info (postcheckoutidentifier=0000000000000001 postwdirparent1=f0161ad23099c690115006c21e96f780f5d740b6 postwdirparent2=)

An extension which makes the log command slow
  $ cat > $TESTTMP/slowlog.py <<EOF
  > from edenscm.mercurial import commands, extensions, util
  > import time
  > def log(orig, ui, repo, *args, **kwargs):
  >     ui.flush()
  >     ret = orig(ui, repo, *args, **kwargs)
  >     time.sleep(2)
  >     return ret
  > def uisetup(ui):
  >     extensions.wrapcommand(commands.table, "log", log)
  > EOF

Test the race between a slow log process and a command that checks out a new commit.
The log process's post-run information shouldn't have changed.
  $ hg log -r . -T "{node}\n" --config extensions.slowlog=$TESTTMP/slowlog.py > $TESTTMP/log.out &
  $ sleep 1
  $ hg prev
  dirstate_info (precheckoutidentifier=0000000000000001 prewdirparent1=f0161ad23099c690115006c21e96f780f5d740b6 prewdirparent2=)
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [d20a80] base
  dirstate_info (postcheckoutidentifier=0000000000000002 postwdirparent1=d20a80d4def38df63a4b330b7fb688f3d4cae1e3 postwdirparent2=)
  $ wait
  $ cat $TESTTMP/log.out
  dirstate_info (precheckoutidentifier=0000000000000001 prewdirparent1=f0161ad23099c690115006c21e96f780f5d740b6 prewdirparent2=)
  f0161ad23099c690115006c21e96f780f5d740b6
  dirstate_info (postcheckoutidentifier=0000000000000001 postwdirparent1=f0161ad23099c690115006c21e96f780f5d740b6 postwdirparent2=)

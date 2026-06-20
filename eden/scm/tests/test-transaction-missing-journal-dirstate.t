#require no-eden

  $ eagerepo

  $ cat > $TESTTMP/ext.py <<'EOF'
  > import os
  > import time
  > 
  > from sapling import registrar
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > @command("debugholdtx", [], "READY GO")
  > def hold(ui, repo, ready, go):
  >     with repo.wlock(), repo.lock():
  >         tx = repo.transaction("hold")
  >         try:
  >             with open(ready, "w"):
  >                 pass
  >             while not os.path.exists(go):
  >                 time.sleep(0.05)
  >         finally:
  >             tx.release()
  > 
  > @command("debuglockfreetx", [], "")
  > def lockfree(ui, repo):
  >     with repo.transaction("lockfree", lockfree=True):
  >         pass
  > EOF

Lock-free transaction close must not move another transaction's journals.

  $ newrepo repo
  $ sl --config extensions.ext=$TESTTMP/ext.py debugholdtx $TESTTMP/ready $TESTTMP/go > $TESTTMP/out 2>&1 &
  $ while [ ! -f $TESTTMP/ready ]; do sleep 0.05; done
  $ sl --config extensions.ext=$TESTTMP/ext.py debuglockfreetx
  $ touch $TESTTMP/go
  $ wait
  $ cat $TESTTMP/out

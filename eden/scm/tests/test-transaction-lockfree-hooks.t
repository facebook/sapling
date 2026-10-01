#require no-eden

  $ configure dummyssh mutation-norecord
  $ enable commitcloud

  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server
  $ setconfig devel.collapse-traceback=true

  $ newserver server
  $ touch base
  $ sl commit -Aqm base
  $ sl bookmark master
  $ sl debugmakepublic .
  $ cd ..

  $ sl clone ssh://user@dummy/server client -q
  $ cd client
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ sl cloud join -q

An external transaction hook works with lock-free cloud sync.

  $ touch draft
  $ sl commit -Aqm draft
  $ sl --config hooks.txnclose.repro=true cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  ...
  commitcloud: commits synchronized
  finished in * (glob)

The upload succeeded and does not need to be retried.

  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd $TESTTMP
  $ eagerepo

  $ cat > $TESTTMP/ext.py <<'EOF'
  > import os
  > 
  > from sapling import registrar
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > @command("debuglockfreebookmark", [], "BOOKMARK")
  > def lockfreebookmark(ui, repo, bookmark):
  >     with repo.transaction("lockfree", lockfree=True) as tx:
  >         repo._bookmarks.applychanges(repo, tx, [(bookmark, repo["."].node())])
  > 
  > @command("debugcheckpendingbookmark", [], "")
  > def checkpendingbookmark(ui, repo):
  >     assert os.environ.get("HG_PENDING_METALOG")
  >     assert not os.environ.get("HG_PENDING")
  >     assert not repo.svfs.exists("bookmarks.pending")
  >     assert "pending" in repo._bookmarks
  >     ui.write("pending\n")
  > 
  > @command("debuglockfreewithdirtydirstate", [], "FILE")
  > def lockfreewithdirtydirstate(ui, repo, file):
  >     repo.dirstate.needcheck(file)
  >     with repo.transaction("lockfree", lockfree=True):
  >         pass
  > EOF

  $ newrepo hooks
  $ touch A
  $ sl commit -Aqm A

Lock-free transactions do not expose preexisting dirstate cache changes to an
external hook.

  $ cat > $TESTTMP/checknodirstatepending.sh <<'EOF'
  > test -z "$HG_PENDING"
  > test ! -e .sl/dirstate.pending
  > EOF
  $ sl --config hooks.pretxnclose="sh $TESTTMP/checknodirstatepending.sh" --config extensions.ext=$TESTTMP/ext.py debuglockfreewithdirtydirstate A

Lock-free hooks read generated bookmarks from the pending metalog without a
legacy bookmarks.pending file.

  $ setconfig extensions.ext=$TESTTMP/ext.py
  $ sl --config hooks.pretxnclose="sl debugcheckpendingbookmark" debuglockfreebookmark pending
  pending

  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client
  $ cd client
  $ echo a >> a
  $ hg commit -Aqm a

Create an empty commit with a misconstructed memctx in the same transaction as a normal commit
  $ cat >> $TESTTMP/repro.py <<EOF
  > from edenscm.mercurial import context, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > @command("^repro", [], "")
  > def repro(ui, repo, *pats, **opts):
  >     def getfile(repo, memctx, path):
  >         if "path" == "a":
  >             return "d"
  > 
  >         return None
  > 
  >     with repo.wlock(), repo.lock(), repo.transaction('tr'):
  >         p1 = context.memctx(
  >             repo,  # repository
  >             (repo['.'].node(), None),  # parents
  >             "valid commit",  # commit message
  >             ["a"],  # files affected by this change
  >             getfile,  # fn - see above
  >             user="author",  # commit author
  >         ).commit()
  > 
  >         context.memctx(
  >             repo,  # repository
  >             (repo[p1].node(), None),  # parents
  >             "empty commit",  # commit message
  >             ["fake"],  # files affected by this change
  >             getfile,  # fn - see above
  >             user="author",  # commit author
  >         ).commit()
  > EOF
  $ hg repro --config extensions.repro="$TESTTMP/repro.py" 2>&1 | grep SystemError
  SystemError: Rust panic

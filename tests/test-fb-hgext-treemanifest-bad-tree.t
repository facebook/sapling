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
  $ hg repro --config extensions.repro="$TESTTMP/repro.py" --traceback
  $ hg log -G --all -T '{node} {manifest}'
  o  eae78c8dead4f2c972a7c3475f57b477577dc335 57faf8a737ae7faf490582941a82319ba6529dca
  |
  o  dc8d2106d7e2d5973fa6cf4cf519ee1b07eafed6 57faf8a737ae7faf490582941a82319ba6529dca
  |
  @  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  
#BUGBUG: Both manifest nodes from above should be present below
  $ hg debughistorypack .hg/store/packs/manifests/*.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  a0c8bcbbb45c  000000000000  000000000000  cb9a9f314b8b  

#require no-windows

Test:
1. Process X is handling a pushrebase request.
2. While running prepushrebase hooks, the local repo and the database were updated.
3. Process X enters the critical section and thinks the local repo is
up-to-date while some internal states might be not.

  $ shorttraceback
  $ . "$TESTDIR/hgsql/library.sh"
  $ enable treemanifest remotefilelog remotenames pushrebase
  $ setconfig hgsql.initialsync=false treemanifest.treeonly=1 treemanifest.sendtrees=1 remotefilelog.reponame=x remotefilelog.cachepath=$TESTTMP/cache ui.ssh="python $TESTDIR/dummyssh" pushrebase.verbose=1 experimental.bundle2lazylocking=True

  $ newrepo state1
  $ echo remotefilelog >> .hg/requires
  $ hg debugdrawdag << 'EOS'
  > A
  > EOS

  $ newrepo state2
  $ echo remotefilelog >> .hg/requires
  $ hg debugdrawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ newrepo state3
  $ echo remotefilelog >> .hg/requires
  $ hg debugdrawdag << 'EOS'
  > B C
  > |/
  > A
  > EOS

  $ cd $TESTTMP
  $ initserver serverrepo master

Update the server repo and the database to state1.

  $ cd $TESTTMP/serverrepo
  $ setconfig treemanifest.server=1
  $ hg pull -r tip $TESTTMP/state1 -q
  $ hg bookmark -r tip master

Prepare the prepushrebase hook to update the server repo and the database.

  $ cat > $TESTTMP/update-to-state2.sh <<EOF
  > # Bypass pushrebase logic that enforces a bundle repo
  > unset HG_HOOK_BUNDLEPATH
  > # Update the server repo and the database to state2
  > hg pull --cwd $TESTTMP/serverrepo -R $TESTTMP/serverrepo -r tip $TESTTMP/state2
  > EOF

Another prepushrebase hook just to warm up in-memory repo states (changelog and
manifest).

  $ cat > $TESTTMP/prepushrebase.py <<EOF
  > def warmup(ui, repo, *args, **kwds):
  >     # Just have some side-effect loading the changelog and manifest
  >     data = repo['tip']['A'].data()
  >     ui.write_err('prepushrebase hook called. A = %r\n' % data)
  > EOF

Setup prepushrebase hooks.

  $ cat >> .hg/hgrc << EOF
  > [hgsql]
  > verbose=1
  > [hooks]
  > prepushrebase.step1=python:$TESTTMP/prepushrebase.py:warmup
  > prepushrebase.step2=bash $TESTTMP/update-to-state2.sh
  > EOF

Do the push!

  $ cd $TESTTMP/state3
  $ hg push -r C --to master ssh://user@dummy/serverrepo
  pushing rev dc0947a82db8 to destination ssh://user@dummy/serverrepo bookmark master
  searching for changes
  remote: prepushrebase hook called. A = 'A'
  remote: [hgsql] got lock after * seconds (read 1 rows) (glob)
  remote: pulling from $TESTTMP/state2
  remote: searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: new changesets 112478962961
  remote: [hgsql] held lock for * seconds (read 8 rows; write 7 rows) (glob)
  remote: checking conflicts with 426bada5c675
  remote: pushing 1 changeset:
  remote:     dc0947a82db8  C
  remote: [hgsql] got lock after * seconds (read 1 rows) (glob)
  remote: rebasing stack from 426bada5c675 onto 426bada5c675
  remote: [hgsql] held lock for * seconds (read 8 rows; write 8 rows) (glob)
  updating bookmark master


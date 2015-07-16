  $ python "$TESTDIR/conduithttp.py" -p 8000 --pid conduit.pid
  $ cat conduit.pid >> $DAEMON_PIDS
  $ hg init initial
  $ cd initial
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "fbconduit = $TESTDIR/../fbconduit.py" >> .hg/hgrc
  $ echo "[fbconduit]" >> .hg/hgrc
  $ echo "reponame = dummy" >> .hg/hgrc
  $ echo "host = localhost:8000" >> .hg/hgrc
  $ echo "path = /intern/conduit/" >> .hg/hgrc
  $ echo "protocol = http" >> .hg/hgrc
  $ touch file
  $ hg add file
  $ hg ci -m "initial commit"
  $ commitid=`hg log -T "{label('custom.fullrev',node)}"`
  $ hg phase -p $commitid
  $ curl -s -X PUT http://localhost:8000/dummy/hg/dummy/git/$commitid/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -T '{gitnode}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -T '{mirrornode("git")}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -T '{mirrornode("dummy", "git")}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.
  $ curl -s -X PUT http://localhost:8000/dummy/git/dummy/hg/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/$commitid
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  initial commit

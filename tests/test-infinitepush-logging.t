  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..
  $ hg clone ssh://user@dummy/repo client -q

  $ cat >> $TESTTMP/uilog.py <<EOF
  > from mercurial import extensions
  > from mercurial import ui as uimod
  > def uisetup(ui):
  >     extensions.wrapfunction(uimod.ui, 'log', mylog)
  > def mylog(orig, self, service, *msg, **opts):
  >     if service in ['infinitepush']:
  >         kwstr = ", ".join("%s=%s" % (k, v) for k, v in
  >                           sorted(opts.iteritems()))
  >         msgstr = msg[0] % msg[1:]
  >         self.warn('%s: %s (%s)\n' % (service, msgstr, kwstr))
  >     return orig(self, service, *msg, **opts)
  > EOF
  $ cat >> repo/.hg/hgrc <<EOF
  > [extensions]
  > uilog=$TESTTMP/uilog.py
  > EOF

  $ cd client
  $ mkcommit commit
  $ hg push -r . --to scratch/scratchpush --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: infinitepush: b2x:infinitepush \(eventtype=start, hostname=.+, requestid=\d+, user=test\) (re)
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  remote: infinitepush: bundlestore \(eventtype=start, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: bundlestore \(elapsed=.+, eventtype=success, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: index \(eventtype=start, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: index \(elapsed=.+, eventtype=success, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: b2x:infinitepush \(elapsed=.+, eventtype=success, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  $ cd ..

Check that logging works for b2x:infinitepushscratchbookmarks part
  $ cd client
  $ hg pushbackup
  searching for changes
  remote: infinitepush: b2x:infinitepush \(eventtype=start, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  remote: infinitepush: index \(eventtype=start, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: index \(elapsed=.+, eventtype=success, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: b2x:infinitepush \(elapsed=.+, eventtype=success, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: b2x:infinitepushscratchbookmarks \(eventtype=start, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: b2x:infinitepushscratchbookmarks \(elapsed=.+, eventtype=success, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  $ cd ..

Make upload to bundlestore fail
  $ cat >> repo/.hg/hgrc <<EOF
  > [scratchbranch]
  > storepath=/dev/null
  > EOF
  $ cd client
  $ mkcommit failpushcommit
  $ hg push -r . --to scratch/scratchpush 2> /dev/null
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: infinitepush: b2x:infinitepush \(eventtype=start, hostname=.+, requestid=\d+, user=test\) (re)
  remote: pushing 2 commits:
  remote:     7e6a6fd9c7c8  commit
  remote:     bba29d9d577a  failpushcommit
  remote: infinitepush: bundlestore \(eventtype=start, hostname=.+, requestid=\d+, user=test\) (re)
  remote: infinitepush: bundlestore \(elapsed=[-+0-9.e]+, errormsg=\[Errno 20\] Not a directory: '/dev/null/\d+', eventtype=failure, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: infinitepush: b2x:infinitepush \(elapsed=[-+0-9.e]+, errormsg=\[Errno 20\] Not a directory: '/dev/null/\d+', eventtype=failure, hostname=[-.0-9a-z]+, reponame=babar, requestid=\d+, user=test\) (re)
  remote: abort: Not a directory: '/dev/null/\d+' (re)
  [255]

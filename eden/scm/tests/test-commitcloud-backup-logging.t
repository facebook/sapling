#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..
  $ hg clone ssh://user@dummy/repo client -q

  $ cat >> $TESTTMP/uilog.py <<EOF
  > from edenscm.mercurial import extensions
  > from edenscm.mercurial import ui as uimod
  > def uisetup(ui):
  >     extensions.wrapfunction(uimod.ui, 'log', mylog)
  > def mylog(orig, self, service, *msg, **opts):
  >     if service in ['infinitepush']:
  >         kwstr = ", ".join("%s=%s" % (k, v) for k, v in
  >                           sorted(opts.items()))
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
  remote: infinitepush: b2x:infinitepush \(eventtype=start, hostname=.+, requestid=\d+, user=\w+\) (re)
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  remote: infinitepush: bundlestore \(bundlesize=654, eventtype=start, hostname=.+, reponame=babar, requestid=\d+, user=\w+\) (re)
  remote: infinitepush: bundlestore \(bundlesize=654, elapsedms=.+, eventtype=success, hostname=.+, reponame=babar, requestid=\d+, user=\w+\) (re)
  remote: infinitepush: index \(eventtype=start, hostname=.+, newheadscount=1, reponame=babar, requestid=\d+, user=\w+\) (re)
  remote: infinitepush: index \(elapsedms=.+, eventtype=success, hostname=.+, newheadscount=1, reponame=babar, requestid=\d+, user=\w+\) (re)
  remote: infinitepush: b2x:infinitepush \(elapsedms=.+, eventtype=success, hostname=.+, reponame=babar, requestid=\d+, user=\w+\) (re)
  $ cd ..

Make upload to bundlestore fail
  $ cat >> repo/.hg/hgrc <<EOF
  > [scratchbranch]
  > storepath=/dev/null
  > EOF
  $ cd client
  $ mkcommit failpushcommit
  $ hg push -r . --to scratch/scratchpush 2>err
  [255]
  $ grep '^remote: ' err
  remote: infinitepush: b2x:infinitepush \(eventtype=start, hostname=.+, requestid=\d+, user=\w+\) (re)
  remote: pushing 2 commits:
  remote:     7e6a6fd9c7c8  commit
  remote:     bba29d9d577a  failpushcommit
  remote: infinitepush: bundlestore \(bundlesize=1247, eventtype=start, hostname=.+, requestid=\d+, user=\w+\) (re)
  remote: infinitepush: bundlestore \(bundlesize=1247, elapsedms=[-+0-9.e]+, errormsg=\[Errno 20\] \$ENOTDIR\$: '/dev/null/[0-9a-f]+', eventtype=failure, hostname=.+, reponame=babar, requestid=\d+, user=\w+\) (re)
  remote: infinitepush: b2x:infinitepush \(elapsedms=[-+0-9.e]+, errormsg=\[Errno 20\] \$ENOTDIR\$: '/dev/null/[0-9a-f]+', eventtype=failure, hostname=.+, reponame=babar, requestid=\d+, user=\w+\) (re)
  remote: abort: \$ENOTDIR\$: /dev/null/[0-9a-f]+ (re)

  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

# Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ initserver master masterrepo

  $ cat >> $TESTTMP/uilog.py <<EOF
  > from edenscm.mercurial import extensions
  > from edenscm.mercurial import ui as uimod
  > def uisetup(ui):
  >     extensions.wrapfunction(uimod.ui, 'log', mylog)
  > def mylog(orig, self, service, *msg, **opts):
  >     if service in ['sqllock']:
  >         kwstr = ", ".join("%s=%s" % (k, v) for k, v in
  >                           sorted(opts.iteritems()))
  >         msgstr = msg[0] % msg[1:]
  >         self.warn('%s: %s (%s)\n' % (service, msgstr, kwstr))
  >     return orig(self, service, *msg, **opts)
  > EOF
  $ cat >> master/.hg/hgrc <<EOF
  > [extensions]
  > uilog=$TESTTMP/uilog.py
  > EOF

# Verify timeouts are logged
  $ cat >> $TESTTMP/forcetimeout.py <<EOF
  > from edenscm.mercurial import error, extensions
  > def uisetup(ui):
  >     hgsql = extensions.find('hgsql')
  >     extensions.wrapfunction(hgsql.sqlcontext, '__enter__', fakeenter)
  > def fakeenter(orig, self):
  >     if self.dbwritable:
  >         extensions.wrapfunction(self.repo.__class__, '_sqllock', lockthrow)
  >     return orig(self)
  > def lockthrow(*args, **kwargs):
  >     raise error.Abort("fake timeout")
  > EOF

  $ cp master/.hg/hgrc $TESTTMP/orighgrc
  $ cat >> master/.hg/hgrc <<EOF
  > [extensions]
  > forcetimeout=$TESTTMP/forcetimeout.py
  > EOF
  $ cd client
  $ hg push ssh://user@dummy/master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: sqllock: failed to get sql lock after * seconds (glob)
  remote:  (elapsed=*, repository=$TESTTMP/master, success=false, valuetype=lockwait) (glob)
  remote: abort: fake timeout
  abort: not a Mercurial bundle
  [255]
  $ cd ..
  $ cp $TESTTMP/orighgrc master/.hg/hgrc

# Verify sqllock times are logged
  $ cd client
  $ hg push ssh://user@dummy/master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: sqllock: waited for sql lock for * seconds (read 1 rows) (glob)
  remote:  (elapsed=*, repository=$TESTTMP/master, success=true, valuetype=lockwait) (glob)
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: sqllock: held sql lock for * seconds (read 5 rows; write 5 rows) (glob)
  remote:  (elapsed=*, readrows=5, repository=$TESTTMP/master, valuetype=lockheld, writerows=5) (glob)

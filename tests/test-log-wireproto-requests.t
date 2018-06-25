  $ cat >> $TESTTMP/uilog.py <<EOF
  > from mercurial import extensions
  > from mercurial import ui as uimod
  > def uisetup(ui):
  >     extensions.wrapfunction(uimod.ui, 'log', mylog)
  > def mylog(orig, self, service, *msg, **opts):
  >     if service in ['wireproto_requests']:
  >         kw = []
  >         for k, v in sorted(opts.iteritems()):
  >           if k =='args':
  >             v = eval(v)
  >             for arg in v:
  >               if isinstance(arg, dict):
  >                 v = sorted(list(arg.iteritems()))
  >             v = str(v)
  >           kw.append("%s=%s" % (k, v)) 
  >         kwstr = ", ".join(kw)
  >         msgstr = msg[0] % msg[1:]
  >         self.warn('%s: %s (%s)\n' % (service, msgstr, kwstr))
  >     return orig(self, service, *msg, **opts)
  > EOF

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > uilog=$TESTTMP/uilog.py
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF

  $ hg init repo
  $ hg clone ssh://user@dummy/repo repo-clone -q
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [wireproto]
  > logrequests=batch,branchmap,getbundle,hello,listkeys,lookup,between,unbundle
  > EOF
  $ echo a > a && hg add a && hg ci -m a
  $ cd ../repo-clone
  $ hg pull 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=[], command=batch, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=[('bookmarks', '1'), ('bundlecaps', 'HG20,$USUAL_BUNDLE2_CAPS$'), ('cg', '1'), ('common', '0000000000000000000000000000000000000000'), ('heads', 'cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b'), ('listkeys', 'bookmarks'), ('phases', '1')], command=getbundle, duration=*, responselen=*) (glob)
  $ cd ../repo
  $ echo b > b && hg add b && hg ci -m b
  $ cd ../repo-clone
  $ hg pull 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=[], command=batch, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=[('bookmarks', '1'), ('bundlecaps', 'HG20,$USUAL_BUNDLE2_CAPS$'), ('cg', '1'), ('common', 'cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b'), ('heads', 'd2ae7f538514cd87c17547b0de4cea71fe1af9fb'), ('listkeys', 'bookmarks'), ('phases', '1')], command=getbundle, duration=*, responselen=*) (glob)
  $ echo c > c && hg add c && hg ci -m c
  $ hg push --force 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=[], command=batch, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['phases'], command=listkeys, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['bookmarks'], command=listkeys, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['666f726365'], command=unbundle, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['phases'], command=listkeys, duration=*, responselen=*) (glob)
  $ hg pull -r ololo 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, responselen=*) (glob)
  remote: wireproto_requests:  (args=['ololo'], command=lookup, duration=*, responselen=*) (glob)

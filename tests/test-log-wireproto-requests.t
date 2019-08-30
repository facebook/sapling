  $ . "$TESTDIR/helpers-wireprotologging.sh"
  $ setconfig extensions.treemanifest=!
  $ CACHEDIR=`pwd`/hgcache

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > remotefilelog=
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > [remotefilelog]
  > cachepath=$CACHEDIR
  > EOF

  $ hg init repo
  $ capturewireprotologs
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ cd ..
  $ hg clone ssh://user@dummy/repo --shallow repo-clone -q
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [wireproto]
  > logrequests=batch,branchmap,getbundle,hello,listkeys,lookup,between,unbundle
  > loggetfiles=true
  > EOF
  $ echo a > a && hg add a && hg ci -m a
  $ cd ../repo-clone
  $ hg pull 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=[], command=batch, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=[('bookmarks', '1'), ('bundlecaps', 'HG20,$USUAL_BUNDLE2_CAPS$%0Aremotefilelog%3DTrue,remotefilelog'), ('cg', '1'), ('common', '0000000000000000000000000000000000000000'), ('heads', 'cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b'), ('listkeys', 'bookmarks'), ('phases', '1')], command=getbundle, duration=*, reponame=unknown, responselen=*) (glob)
  $ cd ../repo
  $ echo b > b && hg add b && hg ci -m b
  $ echo c > c && hg add c && hg ci -m c
  $ cd ../repo-clone
  $ hg pull 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=[], command=batch, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=[('bookmarks', '1'), ('bundlecaps', 'HG20,$USUAL_BUNDLE2_CAPS$%0Aremotefilelog%3DTrue,remotefilelog'), ('cg', '1'), ('common', 'cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b'), ('heads', '177f92b773850b59254aa5e923436f921b55483b'), ('listkeys', 'bookmarks'), ('phases', '1')], command=getbundle, duration=*, reponame=unknown, responselen=*) (glob)
  $ hg up tip -q

Looks like `ui.warn()` after getfiles might not make it's way to client hg. Let's read from file
  $ grep 'getfiles' $TESTTMP/loggedrequests
  wireproto_requests:  (args=[*], command=getfiles, duration=*, reponame=unknown, responselen=*) (glob)
  $ echo cc > c && hg ci -m c
  $ hg push --force 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=[], command=batch, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['phases'], command=listkeys, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['bookmarks'], command=listkeys, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['666f726365'], command=unbundle, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['phases'], command=listkeys, duration=*, reponame=unknown, responselen=*) (glob)
  $ hg pull -r ololo 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, reponame=unknown, responselen=*) (glob)
  remote: wireproto_requests:  (args=['ololo'], command=lookup, duration=*, reponame=unknown, responselen=*) (glob)

Enable clienttelemetry and change reponame
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > clienttelemetry=
  > [common]
  > reponame=repo
  > EOF
  $ hg pull 2>&1 | grep wireproto_requests
  remote: wireproto_requests:  (args=[], command=hello, duration=*, reponame=repo, responselen=*) (glob)
  remote: wireproto_requests:  (args=['0000000000000000000000000000000000000000-0000000000000000000000000000000000000000'], command=between, duration=*, reponame=repo, responselen=*) (glob)
  remote: wireproto_requests:  (args=[], client_fullcommand=pull, client_hostname=*, command=batch, duration=*, reponame=repo, responselen=*) (glob)
  remote: wireproto_requests:  (args=[('bookmarks', '1'), ('bundlecaps', 'HG20,$USUAL_BUNDLE2_CAPS$%0Aremotefilelog%3DTrue,remotefilelog'), ('cg', '0'), ('common', 'cc27a19b3db0a292460298a71c413840f27f6a37'), ('heads', 'cc27a19b3db0a292460298a71c413840f27f6a37'), ('listkeys', 'bookmarks'), ('phases', '1')], client_fullcommand=pull, client_hostname=*, command=getbundle, duration=*, reponame=repo, responselen=*) (glob)
  $ cd ../repo
  $ echo xxx > xxx && hg add xxx && hg ci -m xxx
  $ cd -
  $TESTTMP/repo-clone
  $ hg pull -q
  $ hg up tip -q
  $ grep 'getfiles' $TESTTMP/loggedrequests
  wireproto_requests:  (args=[*], command=getfiles, duration=*, reponame=unknown, responselen=*) (glob)
  wireproto_requests:  (args=[*], client_fullcommand=up tip -q, client_hostname=*, command=getfiles, duration=*, reponame=repo, responselen=*) (glob)

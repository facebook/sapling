#require serve

  $ hg init test
  $ cd test
  $ echo foo>foo
  $ mkdir foo.d foo.d/bAr.hg.d foo.d/baR.d.hg
  $ echo foo>foo.d/foo
  $ echo bar>foo.d/bAr.hg.d/BaR
  $ echo bar>foo.d/baR.d.hg/bAR
  $ hg commit -A -m 1
  adding foo
  adding foo.d/bAr.hg.d/BaR
  adding foo.d/baR.d.hg/bAR
  adding foo.d/foo
  $ hg serve -p $HGPORT -d --pid-file=../hg1.pid -E ../error.log
  $ hg --config server.uncompressed=False serve -p $HGPORT1 -d --pid-file=../hg2.pid

Test server address cannot be reused

#if windows
  $ hg serve -p $HGPORT1 2>&1
  abort: cannot start server at ':$HGPORT1': * (glob)
  [255]
#else
  $ hg serve -p $HGPORT1 2>&1
  abort: cannot start server at ':$HGPORT1': Address already in use
  [255]
#endif
  $ cd ..
  $ cat hg1.pid hg2.pid >> $DAEMON_PIDS

clone via stream

  $ hg clone --uncompressed http://localhost:$HGPORT/ copy 2>&1
  streaming all changes
  6 files to transfer, 606 bytes of data
  transferred * bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg verify -R copy
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 1 changesets, 4 total revisions

try to clone via stream, should use pull instead

  $ hg clone --uncompressed http://localhost:$HGPORT1/ copy2
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved

clone via pull

  $ hg clone http://localhost:$HGPORT1/ copy-pull
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg verify -R copy-pull
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 1 changesets, 4 total revisions
  $ cd test
  $ echo bar > bar
  $ hg commit -A -d '1 0' -m 2
  adding bar
  $ cd ..

clone over http with --update

  $ hg clone http://localhost:$HGPORT1/ updated --update 0
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 5 changes to 5 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r . -R updated
  changeset:   0:8b6053c928fe
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ rm -rf updated

incoming via HTTP

  $ hg clone http://localhost:$HGPORT1/ --rev 0 partial
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd partial
  $ touch LOCAL
  $ hg ci -qAm LOCAL
  $ hg incoming http://localhost:$HGPORT1/ --template '{desc}\n'
  comparing with http://localhost:$HGPORT1/
  searching for changes
  2
  $ cd ..

pull

  $ cd copy-pull
  $ echo '[hooks]' >> .hg/hgrc
  $ echo "changegroup = python \"$TESTDIR/printenv.py\" changegroup" >> .hg/hgrc
  $ hg pull
  pulling from http://localhost:$HGPORT1/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  changegroup hook: HG_NODE=5fed3813f7f5e1824344fdc9cf8f63bb662c292d HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=http://localhost:$HGPORT1/ (glob)
  (run 'hg update' to get a working copy)
  $ cd ..

clone from invalid URL

  $ hg clone http://localhost:$HGPORT/bad
  abort: HTTP Error 404: Not Found
  [255]

test http authentication
+ use the same server to test server side streaming preference

  $ cd test
  $ cat << EOT > userpass.py
  > import base64
  > from mercurial.hgweb import common
  > def perform_authentication(hgweb, req, op):
  >     auth = req.env.get('HTTP_AUTHORIZATION')
  >     if not auth:
  >         raise common.ErrorResponse(common.HTTP_UNAUTHORIZED, 'who',
  >                 [('WWW-Authenticate', 'Basic Realm="mercurial"')])
  >     if base64.b64decode(auth.split()[1]).split(':', 1) != ['user', 'pass']:
  >         raise common.ErrorResponse(common.HTTP_FORBIDDEN, 'no')
  > def extsetup():
  >     common.permhooks.insert(0, perform_authentication)
  > EOT
  $ hg --config extensions.x=userpass.py serve -p $HGPORT2 -d --pid-file=pid \
  >    --config server.preferuncompressed=True \
  >    --config web.push_ssl=False --config web.allow_push=* -A ../access.log
  $ cat pid >> $DAEMON_PIDS

  $ cat << EOF > get_pass.py
  > import getpass
  > def newgetpass(arg):
  >   return "pass"
  > getpass.getpass = newgetpass
  > EOF

  $ hg id http://localhost:$HGPORT2/
  abort: http authorization required for http://localhost:$HGPORT2/
  [255]
  $ hg id http://localhost:$HGPORT2/
  abort: http authorization required for http://localhost:$HGPORT2/
  [255]
  $ hg id --config ui.interactive=true --config extensions.getpass=get_pass.py http://user@localhost:$HGPORT2/
  http authorization required for http://localhost:$HGPORT2/
  realm: mercurial
  user: user
  password: 5fed3813f7f5
  $ hg id http://user:pass@localhost:$HGPORT2/
  5fed3813f7f5
  $ echo '[auth]' >> .hg/hgrc
  $ echo 'l.schemes=http' >> .hg/hgrc
  $ echo 'l.prefix=lo' >> .hg/hgrc
  $ echo 'l.username=user' >> .hg/hgrc
  $ echo 'l.password=pass' >> .hg/hgrc
  $ hg id http://localhost:$HGPORT2/
  5fed3813f7f5
  $ hg id http://localhost:$HGPORT2/
  5fed3813f7f5
  $ hg id http://user@localhost:$HGPORT2/
  5fed3813f7f5
  $ hg clone http://user:pass@localhost:$HGPORT2/ dest 2>&1
  streaming all changes
  7 files to transfer, 916 bytes of data
  transferred * bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
--pull should override server's preferuncompressed
  $ hg clone --pull http://user:pass@localhost:$HGPORT2/ dest-pull 2>&1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 5 changes to 5 files
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg id http://user2@localhost:$HGPORT2/
  abort: http authorization required for http://localhost:$HGPORT2/
  [255]
  $ hg id http://user:pass2@localhost:$HGPORT2/
  abort: HTTP Error 403: no
  [255]

  $ hg -R dest tag -r tip top
  $ hg -R dest push http://user:pass@localhost:$HGPORT2/
  pushing to http://user:***@localhost:$HGPORT2/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ hg rollback -q

  $ cut -c38- ../access.log
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=branchmap HTTP/1.1" 200 -
  "GET /?cmd=stream_out HTTP/1.1" 401 -
  "GET /?cmd=stream_out HTTP/1.1" 200 -
  "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D5fed3813f7f5e1824344fdc9cf8f63bb662c292d
  "GET /?cmd=getbundle HTTP/1.1" 200 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=0&common=5fed3813f7f5e1824344fdc9cf8f63bb662c292d&heads=5fed3813f7f5e1824344fdc9cf8f63bb662c292d&listkeys=phase%2Cbookmarks
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=phases
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D
  "GET /?cmd=getbundle HTTP/1.1" 401 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=5fed3813f7f5e1824344fdc9cf8f63bb662c292d&listkeys=phase%2Cbookmarks
  "GET /?cmd=getbundle HTTP/1.1" 200 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=5fed3813f7f5e1824344fdc9cf8f63bb662c292d&listkeys=phase%2Cbookmarks
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=phases
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=listkeys HTTP/1.1" 403 - x-hgarg-1:namespace=namespaces
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D7f4e523d01f2cc3765ac8934da3d14db775ff872
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=phases
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=phases
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks
  "GET /?cmd=branchmap HTTP/1.1" 200 -
  "GET /?cmd=branchmap HTTP/1.1" 200 -
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks
  "POST /?cmd=unbundle HTTP/1.1" 200 - x-hgarg-1:heads=666f726365
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=phases

  $ cd ..

clone of serve with repo in root and unserved subrepo (issue2970)

  $ hg --cwd test init sub
  $ echo empty > test/sub/empty
  $ hg --cwd test/sub add empty
  $ hg --cwd test/sub commit -qm 'add empty'
  $ hg --cwd test/sub tag -r 0 something
  $ echo sub = sub > test/.hgsub
  $ hg --cwd test add .hgsub
  $ hg --cwd test commit -qm 'add subrepo'
  $ hg clone http://localhost:$HGPORT noslash-clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 7 changes to 7 files
  updating to branch default
  abort: HTTP Error 404: Not Found
  [255]
  $ hg clone http://localhost:$HGPORT/ slash-clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 7 changes to 7 files
  updating to branch default
  abort: HTTP Error 404: Not Found
  [255]

check error log

  $ cat error.log

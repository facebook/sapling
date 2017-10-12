#require killdaemons serve

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
  $ hg serve --config server.uncompressed=False -p $HGPORT1 -d --pid-file=../hg2.pid

Test server address cannot be reused

#if windows
  $ hg serve -p $HGPORT1 2>&1
  abort: cannot start server at 'localhost:$HGPORT1': * (glob)
  [255]
#else
  $ hg serve -p $HGPORT1 2>&1
  abort: cannot start server at 'localhost:$HGPORT1': Address already in use
  [255]
#endif
  $ cd ..
  $ cat hg1.pid hg2.pid >> $DAEMON_PIDS

clone via stream

  $ hg clone --stream http://localhost:$HGPORT/ copy 2>&1
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

  $ hg clone --stream http://localhost:$HGPORT1/ copy2
  warning: stream clone requested but server has them disabled
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  new changesets 8b6053c928fe
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved

try to clone via stream but missing requirements, so should use pull instead

  $ cat > $TESTTMP/removesupportedformat.py << EOF
  > from mercurial import localrepo
  > def extsetup(ui):
  >     localrepo.localrepository.supportedformats.remove('generaldelta')
  > EOF

  $ hg clone --config extensions.rsf=$TESTTMP/removesupportedformat.py --stream http://localhost:$HGPORT/ copy3
  warning: stream clone requested but client is missing requirements: generaldelta
  (see https://www.mercurial-scm.org/wiki/MissingRequirement for more information)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  new changesets 8b6053c928fe
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved

clone via pull

  $ hg clone http://localhost:$HGPORT1/ copy-pull
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  new changesets 8b6053c928fe
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
  new changesets 8b6053c928fe:5fed3813f7f5
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
  new changesets 8b6053c928fe
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
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > changegroup = sh -c "printenv.py changegroup"
  > EOF
  $ hg pull
  pulling from http://localhost:$HGPORT1/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 5fed3813f7f5
  changegroup hook: HG_HOOKNAME=changegroup HG_HOOKTYPE=changegroup HG_NODE=5fed3813f7f5e1824344fdc9cf8f63bb662c292d HG_NODE_LAST=5fed3813f7f5e1824344fdc9cf8f63bb662c292d HG_SOURCE=pull HG_TXNID=TXN:$ID$ HG_URL=http://localhost:$HGPORT1/
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
  $ hg serve --config extensions.x=userpass.py -p $HGPORT2 -d --pid-file=pid \
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
  new changesets 8b6053c928fe:5fed3813f7f5
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

  $ sed 's/.*] "/"/' < ../access.log
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=branchmap HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=stream_out HTTP/1.1" 401 - x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=stream_out HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D5fed3813f7f5e1824344fdc9cf8f63bb662c292d x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=getbundle HTTP/1.1" 200 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Aphases%253Dheads%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=0&common=5fed3813f7f5e1824344fdc9cf8f63bb662c292d&heads=5fed3813f7f5e1824344fdc9cf8f63bb662c292d&listkeys=bookmarks&phases=1 x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=getbundle HTTP/1.1" 401 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Aphases%253Dheads%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=5fed3813f7f5e1824344fdc9cf8f63bb662c292d&listkeys=bookmarks&phases=1 x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=getbundle HTTP/1.1" 200 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Aphases%253Dheads%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=5fed3813f7f5e1824344fdc9cf8f63bb662c292d&listkeys=bookmarks&phases=1 x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=lookup HTTP/1.1" 200 - x-hgarg-1:key=tip x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 403 - x-hgarg-1:namespace=namespaces x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=capabilities HTTP/1.1" 200 -
  "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D7f4e523d01f2cc3765ac8934da3d14db775ff872 x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 401 - x-hgarg-1:namespace=phases x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=phases x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=branchmap HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=branchmap HTTP/1.1" 200 - x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=bookmarks x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)
  "POST /?cmd=unbundle HTTP/1.1" 200 - x-hgarg-1:heads=666f726365* (glob)
  "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=phases x-hgproto-1:0.1 0.2 comp=*zlib,none,bzip2 (glob)

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
  new changesets 8b6053c928fe:56f9bc90cce6
  updating to branch default
  abort: HTTP Error 404: Not Found
  [255]
  $ hg clone http://localhost:$HGPORT/ slash-clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 7 changes to 7 files
  new changesets 8b6053c928fe:56f9bc90cce6
  updating to branch default
  abort: HTTP Error 404: Not Found
  [255]

check error log

  $ cat error.log

check abort error reporting while pulling/cloning

  $ $RUNTESTDIR/killdaemons.py
  $ hg serve -R test -p $HGPORT -d --pid-file=hg3.pid -E error.log --config extensions.crash=${TESTDIR}/crashgetbundler.py
  $ cat hg3.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/ abort-clone
  requesting all changes
  remote: abort: this is an exercise
  abort: pull failed on remote
  [255]
  $ cat error.log

disable pull-based clones

  $ hg serve -R test -p $HGPORT1 -d --pid-file=hg4.pid -E error.log --config server.disablefullbundle=True
  $ cat hg4.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT1/ disable-pull-clone
  requesting all changes
  remote: abort: server has pull-based clones disabled
  abort: pull failed on remote
  (remove --pull if specified or upgrade Mercurial)
  [255]

... but keep stream clones working

  $ hg clone --stream --noupdate http://localhost:$HGPORT1/ test-stream-clone
  streaming all changes
  * files to transfer, * of data (glob)
  transferred * in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cat error.log

... and also keep partial clones and pulls working
  $ hg clone http://localhost:$HGPORT1 --rev 0 test-partial-clone
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  new changesets 8b6053c928fe
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg pull -R test-partial-clone
  pulling from http://localhost:$HGPORT1/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 3 changes to 3 files
  new changesets 5fed3813f7f5:56f9bc90cce6
  (run 'hg update' to get a working copy)

corrupt cookies file should yield a warning

  $ cat > $TESTTMP/cookies.txt << EOF
  > bad format
  > EOF

  $ hg --config auth.cookiefile=$TESTTMP/cookies.txt id http://localhost:$HGPORT/
  (error loading cookie file $TESTTMP/cookies.txt: '*/cookies.txt' does not look like a Netscape format cookies file; continuing without cookies) (glob)
  56f9bc90cce6

  $ killdaemons.py

Create dummy authentication handler that looks for cookies. It doesn't do anything
useful. It just raises an HTTP 500 with details about the Cookie request header.
We raise HTTP 500 because its message is printed in the abort message.

  $ cat > cookieauth.py << EOF
  > from mercurial import util
  > from mercurial.hgweb import common
  > def perform_authentication(hgweb, req, op):
  >     cookie = req.env.get('HTTP_COOKIE')
  >     if not cookie:
  >         raise common.ErrorResponse(common.HTTP_SERVER_ERROR, 'no-cookie')
  >     raise common.ErrorResponse(common.HTTP_SERVER_ERROR, 'Cookie: %s' % cookie)
  > def extsetup():
  >     common.permhooks.insert(0, perform_authentication)
  > EOF

  $ hg serve --config extensions.cookieauth=cookieauth.py -R test -p $HGPORT -d --pid-file=pid
  $ cat pid > $DAEMON_PIDS

Request without cookie sent should fail due to lack of cookie

  $ hg id http://localhost:$HGPORT
  abort: HTTP Error 500: no-cookie
  [255]

Populate a cookies file

  $ cat > cookies.txt << EOF
  > # HTTP Cookie File
  > # Expiration is 2030-01-01 at midnight
  > .example.com	TRUE	/	FALSE	1893456000	hgkey	examplevalue
  > EOF

Should not send a cookie for another domain

  $ hg --config auth.cookiefile=cookies.txt id http://localhost:$HGPORT/
  abort: HTTP Error 500: no-cookie
  [255]

Add a cookie entry for our test server and verify it is sent

  $ cat >> cookies.txt << EOF
  > localhost.local	FALSE	/	FALSE	1893456000	hgkey	localhostvalue
  > EOF

  $ hg --config auth.cookiefile=cookies.txt id http://localhost:$HGPORT/
  abort: HTTP Error 500: Cookie: hgkey=localhostvalue
  [255]

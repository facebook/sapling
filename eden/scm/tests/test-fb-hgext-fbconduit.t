#chg-compatible

  $ . "$TESTDIR/hgsql/library.sh"

Start up translation service.

  $ hg debugpython -- "$TESTDIR/conduithttp.py" --port-file conduit.port --pid conduit.pid
  $ cat conduit.pid >> $DAEMON_PIDS
  $ CONDUIT_PORT=`cat conduit.port`
  $ cat > ~/.arcrc <<EOF
  > {
  >   "hosts": {
  >     "https://phabricator.intern.facebook.com/api/": {
  >       "user": "testuser",
  >       "oauth": "testtoken"
  >     }
  >  }
  > }
  > EOF

Basic functionality.

  $ hg init basic
  $ cd basic
  $ echo {} > .arcconfig
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fbscmquery=
  > [fbscmquery]
  > reponame = basic
  > host = localhost:$CONDUIT_PORT
  > path = /intern/conduit/
  > protocol = http
  > [phabricator]
  > arcrc_host = https://phabricator.intern.facebook.com/api/
  > graphql_host = http://localhost:$CONDUIT_PORT
  > default_timeout = 60
  > graphql_app_id = 1234
  > graphql_app_token = TOKEN123
  > EOF
  $ touch file
  $ hg add file
  $ hg ci -m "initial commit"
  $ commitid=`hg log -T "{label('custom.fullrev',node)}"`
  $ hg phase -p $commitid
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/basic/hg/basic/git/$commitid/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -T '{gitnode}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -T '{mirrornode("git")}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -T '{mirrornode("basic", "git")}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/basic/git/basic/hg/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/$commitid
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  initial commit

Make sure that we fail gracefully if the translation server returns an
HTTP error code.

  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/fail_next/whoops
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: whoops
  $ cd ..

Test with one backing repos specified.

  $ hg init single_backingrepo
  $ echo {} > .arcconfig
  $ cd single_backingrepo
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "fbscmquery=" >> .hg/hgrc
  $ echo "[fbscmquery]" >> .hg/hgrc
  $ echo "reponame = single" >> .hg/hgrc
  $ echo "backingrepos = single_src" >> .hg/hgrc
  $ echo "host = localhost:$CONDUIT_PORT" >> .hg/hgrc
  $ echo "path = /intern/conduit/" >> .hg/hgrc
  $ echo "protocol = http" >> .hg/hgrc
  $ echo "[phabricator]" >> .hg/hgrc
  $ echo "arcrc_host = https://phabricator.intern.facebook.com/api/" >> .hg/hgrc
  $ echo "graphql_host = http://localhost:$CONDUIT_PORT" >> .hg/hgrc
  > echo "default_timeout = 60" >> .hg/hgrc
  > echo "graphql_app_id = 1234" >> .hg/hgrc
  > echo "graphql_app_token = TOKEN123" >> .hg/hgrc
  $ touch file
  $ hg add file
  $ hg ci -m "initial commit"
  $ commitid=`hg log -T "{label('custom.fullrev',node)}"`
  $ hg phase -p $commitid
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/single/hg/single_src/git/$commitid/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -T '{gitnode}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/single_src/git/single/hg/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/$commitid
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  initial commit
  $ cd ..

Test with multiple backing repos specified.

  $ hg init backingrepos
  $ echo {} > .arcconfig
  $ cd backingrepos
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "fbscmquery=" >> .hg/hgrc
  $ echo "[fbscmquery]" >> .hg/hgrc
  $ echo "reponame = multiple" >> .hg/hgrc
  $ echo "backingrepos = src_a src_b src_c" >> .hg/hgrc
  $ echo "host = localhost:$CONDUIT_PORT" >> .hg/hgrc
  $ echo "path = /intern/conduit/" >> .hg/hgrc
  $ echo "protocol = http" >> .hg/hgrc
  $ echo "[phabricator]" >> .hg/hgrc
  $ echo "arcrc_host = https://phabricator.intern.facebook.com/api/" >> .hg/hgrc
  $ echo "graphql_host = http://localhost:$CONDUIT_PORT" >> .hg/hgrc
  > echo "default_timeout = 60" >> .hg/hgrc
  > echo "graphql_app_id = 1234" >> .hg/hgrc
  > echo "graphql_app_token = TOKEN123" >> .hg/hgrc
  $ touch file_a
  $ hg add file_a
  $ hg ci -m "commit 1"
  $ touch file_b
  $ hg add file_b
  $ hg ci -m "commit 2"
  $ touch file_c
  $ hg add file_c
  $ hg ci -m "commit 3"
  $ commit_a_id=`hg log -T "{label('custom.fullrev',node)}" -r ".^^"`
  $ commit_b_id=`hg log -T "{label('custom.fullrev',node)}" -r ".^"`
  $ commit_c_id=`hg log -T "{label('custom.fullrev',node)}" -r .`
  $ hg phase -p $commit_a_id
  $ hg phase -p $commit_b_id
  $ hg phase -p $commit_c_id
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/multiple/hg/src_a/git/$commit_a_id/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/multiple/hg/src_b/git/$commit_b_id/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/multiple/hg/src_c/git/$commit_c_id/cccccccccccccccccccccccccccccccccccccccc
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/multiple/hg/src_b/git/$commit_c_id/dddddddddddddddddddddddddddddddddddddddd
  $ hg log -T '{gitnode}\n' -r ".^^"
  src_a: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg log -T '{gitnode}\n' -r ".^"
  src_b: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ hg log -T '{gitnode}\n' -r .
  src_b: dddddddddddddddddddddddddddddddddddddddd; src_c: cccccccccccccccccccccccccccccccccccccccc
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/src_a/git/multiple/hg/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/$commit_a_id
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/src_b/git/multiple/hg/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/$commit_b_id
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/src_c/git/multiple/hg/cccccccccccccccccccccccccccccccccccccccc/$commit_c_id
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/src_b/git/multiple/hg/dddddddddddddddddddddddddddddddddddddddd/$commit_c_id
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  commit 1
  $ hg log -r 'gitnode("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")' -T '{desc}\n'
  commit 2
  $ hg log -r 'gitnode("cccccccccccccccccccccccccccccccccccccccc")' -T '{desc}\n'
  commit 3
  $ hg log -r 'gitnode("dddddddddddddddddddddddddddddddddddddddd")' -T '{desc}\n'
  commit 3
  $ hg log -r gaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa -T '{desc}\n'
  commit 1
  $ hg log -r gbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb -T '{desc}\n'
  commit 2
  $ hg log -r gcccccccccccccccccccccccccccccccccccccccc -T '{desc}\n'
  commit 3
  $ hg log -r gdddddddddddddddddddddddddddddddddddddddd -T '{desc}\n'
  commit 3
  $ cd ..

Test with a bad server port, where we get connection refused errors.

  $ hg init errortest
  $ echo {} > .arcconfig
  $ cd errortest
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "fbscmquery=" >> .hg/hgrc
  $ echo "[fbscmquery]" >> .hg/hgrc
  $ echo "reponame = errortest" >> .hg/hgrc
  $ echo "host = localhost:9" >> .hg/hgrc
  $ echo "path = /intern/conduit/" >> .hg/hgrc
  $ echo "protocol = http" >> .hg/hgrc
  $ touch file
  $ hg add file
  $ hg ci -m "initial commit"
  $ commitid=`hg log -T "{label('custom.fullrev',node)}"`
  $ hg phase -p $commitid
  $ hg log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: * (glob)
  $ cd ..

Make sure the template keywords are documented correctly

  $ cd basic
  $ hg help templates | grep gitnode
      gitnode       Return the git revision corresponding to a given hg rev
  $ cd ..

Make sure that locally found commits actually work
  $ cd basic
  $ hg up rFBS4772e01e369e598da6a916e3f4fc83dd8944bf23
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

Make sure that globalrevs work

  $ cd ..
  $ initserver mockwwwrepo mockwwwrepo
  $ cd mockwwwrepo
  $ enable fbscmquery globalrevs pushrebase
  $ setconfig \
  > hgsql.enabled=True \
  > fbscmquery.reponame=basic \
  > fbscmquery.host=localhost:$CONDUIT_PORT \
  > fbscmquery.path=/intern/conduit/ \
  > fbscmquery.protocol=http \
  > globalrevs.onlypushrebase=False \
  > globalrevs.startrev=5000 \
  > globalrevs.svnrevinteroperation=True \
  > phabricator.arcrc_host=https://phabricator.intern.facebook.com/api/ \
  > phabricator.graphql_host=http://localhost:$CONDUIT_PORT \
  > phabricator.default_timeout=60 \
  > phabricator.graphql_app_id=1234 \
  > phabricator.graphql_app_token=TOKEN123
  > echo {} > .arcconfig

  $ hg initglobalrev 5000 --i-know-what-i-am-doing

  $ touch x
  $ hg commit -Aqm "added a file"

  $ hg up rWWW4999
  abort: unknown revision 'rWWW4999'!
  (if rWWW4999 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

  $ hg up rWWWHGaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  abort: unknown revision 'rWWWHGaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'!
  (if rWWWHGaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

  $ hg up -q rWWW5000
  $ hg up -q rWWWHGb5dd6b876215cbea8d0cd6c093bf6c0326bb40ab


Make sure that the `globalrevs.scmquerylookup` configuration works as expected.

- Set the configurations to ensure we are using the ScmQuery lookup for
globalrevs.

  $ setconfig globalrevs.scmquerylookup=True phabricator.graphql_host=https://nonesuch.intern.facebook.com

- Test that if the ScmQuery lookup throws an exception, we are still able to
fallback to the slow lookup path.

  $ hg up -q m5000

- Fix the conduit configurations so that we can mock ScmQuery lookups.

  $ setconfig phabricator.graphql_host=http://localhost:$CONDUIT_PORT

- Test that the lookup fails because ScmQuery returns no hash corresponding to
the globalrev 5000.

  $ hg up -q m5000
  abort: unknown revision 'm5000'!
  (if m5000 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

- Setup the `globalrev->hash` mapping for commit with globalrev 5000.

  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/basic/globalrev/basic/hg/5000/b5dd6b876215cbea8d0cd6c093bf6c0326bb40ab

- Test that the lookup succeeds now.

  $ hg up -q m5000

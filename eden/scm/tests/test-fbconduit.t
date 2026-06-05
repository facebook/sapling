#chg-compatible
#inprocess-hg-incompatible
#require no-eden no-windows

  $ cat >> $HGRCPATH <<EOF
  > [globalrevs]
  > scmquerylookup=True
  > edenapilookup=False
  > EOF

Start up translation service.
 
  $ sl debugpython -- "$TESTDIR/conduithttp.py" --port-file conduit.port --pid conduit.pid
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
  $ setconfig phabricator.use-unix-socket=False

Basic functionality.

  $ sl init basic
  $ cd basic
  $ echo {} > .arcconfig
  $ enable fbcodereview
  $ cat >> .sl/config <<EOF
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
  $ sl add file
  $ sl ci -m "initial commit"
  $ commitid=`sl log -T "{label('custom.fullrev',node)}"`
  $ sl debugmakepublic $commitid
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/basic/hg/basic/git/$commitid/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sl log -T '{gitnode}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sl log -T '{mirrornode("git")}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sl log -T '{mirrornode("basic", "git")}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sl log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/basic/git/basic/hg/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/$commitid
  $ sl log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  initial commit

Make sure that we fail gracefully if the translation server returns an
HTTP error code.

  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/fail_next/whoops
  $ sl log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: whoops
  $ cd ..

Test with one backing repos specified.

  $ sl init single_backingrepo
  $ echo {} > .arcconfig
  $ cd single_backingrepo
  $ echo "[extensions]" >> .sl/config
  $ echo "fbcodereview=" >> .sl/config
  $ echo "[fbscmquery]" >> .sl/config
  $ echo "reponame = single" >> .sl/config
  $ echo "backingrepos = single_src" >> .sl/config
  $ echo "host = localhost:$CONDUIT_PORT" >> .sl/config
  $ echo "path = /intern/conduit/" >> .sl/config
  $ echo "protocol = http" >> .sl/config
  $ echo "[phabricator]" >> .sl/config
  $ echo "arcrc_host = https://phabricator.intern.facebook.com/api/" >> .sl/config
  $ echo "graphql_host = http://localhost:$CONDUIT_PORT" >> .sl/config
  > echo "default_timeout = 60" >> .sl/config
  > echo "graphql_app_id = 1234" >> .sl/config
  > echo "graphql_app_token = TOKEN123" >> .sl/config
  $ touch file
  $ sl add file
  $ sl ci -m "initial commit"
  $ commitid=`sl log -T "{label('custom.fullrev',node)}"`
  $ sl debugmakepublic $commitid
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/single/hg/single_src/git/$commitid/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sl log -T '{gitnode}\n'
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sl log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/single_src/git/single/hg/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/$commitid
  $ sl log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  initial commit
  $ cd ..

Test with multiple backing repos specified.

  $ sl init backingrepos
  $ echo {} > .arcconfig
  $ cd backingrepos
  $ echo "[extensions]" >> .sl/config
  $ echo "fbcodereview=" >> .sl/config
  $ echo "[fbscmquery]" >> .sl/config
  $ echo "reponame = multiple" >> .sl/config
  $ echo "backingrepos = src_a src_b src_c" >> .sl/config
  $ echo "host = localhost:$CONDUIT_PORT" >> .sl/config
  $ echo "path = /intern/conduit/" >> .sl/config
  $ echo "protocol = http" >> .sl/config
  $ echo "[phabricator]" >> .sl/config
  $ echo "arcrc_host = https://phabricator.intern.facebook.com/api/" >> .sl/config
  $ echo "graphql_host = http://localhost:$CONDUIT_PORT" >> .sl/config
  > echo "default_timeout = 60" >> .sl/config
  > echo "graphql_app_id = 1234" >> .sl/config
  > echo "graphql_app_token = TOKEN123" >> .sl/config
  $ touch file_a
  $ sl add file_a
  $ sl ci -m "commit 1"
  $ touch file_b
  $ sl add file_b
  $ sl ci -m "commit 2"
  $ touch file_c
  $ sl add file_c
  $ sl ci -m "commit 3"
  $ commit_a_id=`sl log -T "{label('custom.fullrev',node)}" -r ".^^"`
  $ commit_b_id=`sl log -T "{label('custom.fullrev',node)}" -r ".^"`
  $ commit_c_id=`sl log -T "{label('custom.fullrev',node)}" -r .`
  $ sl debugmakepublic $commit_a_id
  $ sl debugmakepublic $commit_b_id
  $ sl debugmakepublic $commit_c_id
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/multiple/hg/src_a/git/$commit_a_id/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/multiple/hg/src_b/git/$commit_b_id/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/multiple/hg/src_c/git/$commit_c_id/cccccccccccccccccccccccccccccccccccccccc
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/multiple/hg/src_b/git/$commit_c_id/dddddddddddddddddddddddddddddddddddddddd
  $ sl log -T '{gitnode}\n' -r ".^^"
  src_a: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sl log -T '{gitnode}\n' -r ".^"
  src_b: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ sl log -T '{gitnode}\n' -r .
  src_b: dddddddddddddddddddddddddddddddddddddddd; src_c: cccccccccccccccccccccccccccccccccccccccc
  $ sl log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/src_a/git/multiple/hg/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/$commit_a_id
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/src_b/git/multiple/hg/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/$commit_b_id
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/src_c/git/multiple/hg/cccccccccccccccccccccccccccccccccccccccc/$commit_c_id
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/src_b/git/multiple/hg/dddddddddddddddddddddddddddddddddddddddd/$commit_c_id
  $ sl log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")' -T '{desc}\n'
  commit 1
  $ sl log -r 'gitnode("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")' -T '{desc}\n'
  commit 2
  $ sl log -r 'gitnode("cccccccccccccccccccccccccccccccccccccccc")' -T '{desc}\n'
  commit 3
  $ sl log -r 'gitnode("dddddddddddddddddddddddddddddddddddddddd")' -T '{desc}\n'
  commit 3
  $ sl log -r gaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa -T '{desc}\n'
  commit 1
  $ sl log -r gbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb -T '{desc}\n'
  commit 2
  $ sl log -r gcccccccccccccccccccccccccccccccccccccccc -T '{desc}\n'
  commit 3
  $ sl log -r gdddddddddddddddddddddddddddddddddddddddd -T '{desc}\n'
  commit 3
  $ cd ..

Test with a bad server port, where we get connection refused errors.

  $ sl init errortest
  $ echo {} > .arcconfig
  $ cd errortest
  $ echo "[extensions]" >> .sl/config
  $ echo "fbcodereview=" >> .sl/config
  $ echo "[fbscmquery]" >> .sl/config
  $ echo "reponame = errortest" >> .sl/config
  $ echo "host = localhost:9" >> .sl/config
  $ echo "path = /intern/conduit/" >> .sl/config
  $ echo "protocol = http" >> .sl/config
  $ touch file
  $ sl add file
  $ sl ci -m "initial commit"
  $ commitid=`sl log -T "{label('custom.fullrev',node)}"`
  $ sl debugmakepublic $commitid
  $ sl log -r 'gitnode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")'
  Could not translate revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: * (glob)
  $ cd ..

Make sure the template keywords are documented correctly

  $ cd basic
  $ sl help templates | grep gitnode
      gitnode       Return the git revision corresponding to a given sl rev
  $ cd ..

Make sure that locally found commits actually work
  $ cd basic
  $ sl up rFBS4772e01e369e598da6a916e3f4fc83dd8944bf23
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

Make sure that globalrevs work

  $ cd
  $ sl init mockwwwrepo
  $ cd mockwwwrepo
  $ enable fbcodereview globalrevs pushrebase
  $ setconfig \
  > globalrevs.server=True \
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

  $ sl debuginitglobalrev 5000

  $ touch x
  $ sl commit -Aqm "added a file"
  $ sl debugmakepublic -r .

  $ sl up rWWW4999
  abort: unknown revision 'rWWW4999'!
  [255]

  $ sl up rWWWHGaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  abort: unknown revision 'rWWWHGaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'!
  [255]

  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/basic/GLOBAL_REV/basic/hg/5000/b5dd6b876215cbea8d0cd6c093bf6c0326bb40ab
  $ sl up -q rWWW5000
  $ sl up -q rWWWHGb5dd6b876215cbea8d0cd6c093bf6c0326bb40ab


Make sure that the `globalrevs.scmquerylookup` configuration works as expected.

- Set the configurations to ensure we are using the ScmQuery lookup for
globalrevs.

  $ curl -s -X DELETE http://localhost:$CONDUIT_PORT/basic/GLOBAL_REV/basic/hg/5000/b5dd6b876215cbea8d0cd6c093bf6c0326bb40ab
  $ sl up -q null
  $ setconfig phabricator.graphql_host=https://nonesuch.intern.facebook.com

- Test that if the ScmQuery lookup throws an exception, we are still able to
fallback to the slow lookup path.

  $ sl up -q m5000 2>&1 | grep 'failed to lookup globalrev 5000 from scmquery' > /dev/null
  $ sl log -r . -T '{globalrev}\n'
  5000

- Fix the conduit configurations so that we can mock ScmQuery lookups.

  $ setconfig phabricator.graphql_host=http://localhost:$CONDUIT_PORT
  $ sl up -q null

- Test that the lookup fails because ScmQuery returns no hash corresponding to
the globalrev 5000.

  $ sl up -q m5000
  abort: unknown revision 'm5000'!
  [255]

- Setup the `globalrev->hash` mapping for commit with globalrev 5000.

  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/basic/GLOBAL_REV/basic/hg/5000/b5dd6b876215cbea8d0cd6c093bf6c0326bb40ab

- Test that the lookup succeeds now.

  $ sl up -q m5000

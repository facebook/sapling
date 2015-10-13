Set up a server

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > clonebundles =
  > EOF

  $ touch foo
  $ hg -q commit -A -m 'add foo'
  $ touch bar
  $ hg -q commit -A -m 'add bar'

  $ hg serve -d -p $HGPORT --pid-file hg.pid --accesslog access.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..

Feature disabled by default
(client should not request manifest)

  $ hg clone -U http://localhost:$HGPORT feature-disabled
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

  $ cat server/access.log
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D (glob)
  * - - [*] "GET /?cmd=getbundle HTTP/1.1" 200 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=aaff8d2ffbbf07a46dd1f05d8ae7877e3f56e2a2&listkeys=phase%2Cbookmarks (glob)
  * - - [*] "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=phases (glob)

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > clonebundles = true
  > EOF

Missing manifest should not result in server lookup

  $ hg --verbose clone -U http://localhost:$HGPORT no-manifest
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

  $ tail -4 server/access.log
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D (glob)
  * - - [*] "GET /?cmd=getbundle HTTP/1.1" 200 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=aaff8d2ffbbf07a46dd1f05d8ae7877e3f56e2a2&listkeys=phase%2Cbookmarks (glob)
  * - - [*] "GET /?cmd=listkeys HTTP/1.1" 200 - x-hgarg-1:namespace=phases (glob)

Empty manifest file results in retrieval
(the extension only checks if the manifest file exists)

  $ touch server/.hg/clonebundles.manifest
  $ hg --verbose clone -U http://localhost:$HGPORT empty-manifest
  no clone bundles available on remote; falling back to regular clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

Manifest file with invalid URL aborts

  $ echo 'http://does.not.exist/bundle.hg' > server/.hg/clonebundles.manifest
  $ hg clone http://localhost:$HGPORT 404-url
  applying clone bundle from http://does.not.exist/bundle.hg
  error fetching bundle: [Errno -2] Name or service not known
  abort: error applying bundle
  (consider contacting the server operator if this error persists)
  [255]

Server is not running aborts

  $ echo "http://localhost:$HGPORT1/bundle.hg" > server/.hg/clonebundles.manifest
  $ hg clone http://localhost:$HGPORT server-not-runner
  applying clone bundle from http://localhost:$HGPORT1/bundle.hg
  error fetching bundle: [Errno 111] Connection refused
  abort: error applying bundle
  (consider contacting the server operator if this error persists)
  [255]

Server returns 404

  $ python $TESTDIR/dumbhttp.py -p $HGPORT1 --pid http.pid
  $ cat http.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT running-404
  applying clone bundle from http://localhost:$HGPORT1/bundle.hg
  HTTP error fetching bundle: HTTP Error 404: File not found
  abort: error applying bundle
  (consider contacting the server operator if this error persists)
  [255]

We can override failure to fall back to regular clone

  $ hg --config ui.clonebundlefallback=true clone -U http://localhost:$HGPORT 404-fallback
  applying clone bundle from http://localhost:$HGPORT1/bundle.hg
  HTTP error fetching bundle: HTTP Error 404: File not found
  falling back to normal clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

Bundle with partial content works

  $ hg -R server bundle --type gzip --base null -r 53245c60e682 partial.hg
  1 changesets found

  $ echo "http://localhost:$HGPORT1/partial.hg" > server/.hg/clonebundles.manifest
  $ hg clone -U http://localhost:$HGPORT partial-bundle
  applying clone bundle from http://localhost:$HGPORT1/partial.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  finished applying clone bundle
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Bundle with full content works

  $ hg -R server bundle --type gzip-v2 --base null -r tip full.hg
  2 changesets found

  $ echo "http://localhost:$HGPORT1/full.hg" > server/.hg/clonebundles.manifest
  $ hg clone -U http://localhost:$HGPORT full-bundle
  applying clone bundle from http://localhost:$HGPORT1/full.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

Set up a server

  $ cat >> $HGRCPATH << EOF
  > [format]
  > usegeneraldelta=yes
  > EOF
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

Missing manifest should not result in server lookup

  $ hg --verbose clone -U http://localhost:$HGPORT no-manifest
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

  $ cat server/access.log
  * - - [*] "GET /?cmd=capabilities HTTP/1.1" 200 - (glob)
  * - - [*] "GET /?cmd=batch HTTP/1.1" 200 - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D (glob)
  * - - [*] "GET /?cmd=getbundle HTTP/1.1" 200 - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=aaff8d2ffbbf07a46dd1f05d8ae7877e3f56e2a2&listkeys=phases%2Cbookmarks (glob)

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
  error fetching bundle: (.* not known|getaddrinfo failed|No address associated with hostname) (re)
  abort: error applying bundle
  (if this error persists, consider contacting the server operator or disable clone bundles via "--config ui.clonebundles=false")
  [255]

Server is not running aborts

  $ echo "http://localhost:$HGPORT1/bundle.hg" > server/.hg/clonebundles.manifest
  $ hg clone http://localhost:$HGPORT server-not-runner
  applying clone bundle from http://localhost:$HGPORT1/bundle.hg
  error fetching bundle: * refused* (glob)
  abort: error applying bundle
  (if this error persists, consider contacting the server operator or disable clone bundles via "--config ui.clonebundles=false")
  [255]

Server returns 404

  $ python $TESTDIR/dumbhttp.py -p $HGPORT1 --pid http.pid
  $ cat http.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT running-404
  applying clone bundle from http://localhost:$HGPORT1/bundle.hg
  HTTP error fetching bundle: HTTP Error 404: File not found
  abort: error applying bundle
  (if this error persists, consider contacting the server operator or disable clone bundles via "--config ui.clonebundles=false")
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

  $ hg -R server bundle --type gzip-v1 --base null -r 53245c60e682 partial.hg
  1 changesets found

We verify exact bundle content as an extra check against accidental future
changes. If this output changes, we could break old clients.

  $ f --size --hexdump partial.hg
  partial.hg: size=207
  0000: 48 47 31 30 47 5a 78 9c 63 60 60 98 17 ac 12 93 |HG10GZx.c``.....|
  0010: f0 ac a9 23 45 70 cb bf 0d 5f 59 4e 4a 7f 79 21 |...#Ep..._YNJ.y!|
  0020: 9b cc 40 24 20 a0 d7 ce 2c d1 38 25 cd 24 25 d5 |..@$ ...,.8%.$%.|
  0030: d8 c2 22 cd 38 d9 24 cd 22 d5 c8 22 cd 24 cd 32 |..".8.$."..".$.2|
  0040: d1 c2 d0 c4 c8 d2 32 d1 38 39 29 c9 34 cd d4 80 |......2.89).4...|
  0050: ab 24 b5 b8 84 cb 40 c1 80 2b 2d 3f 9f 8b 2b 31 |.$....@..+-?..+1|
  0060: 25 45 01 c8 80 9a d2 9b 65 fb e5 9e 45 bf 8d 7f |%E......e...E...|
  0070: 9f c6 97 9f 2b 44 34 67 d9 ec 8e 0f a0 92 0b 75 |....+D4g.......u|
  0080: 41 d6 24 59 18 a4 a4 9a a6 18 1a 5b 98 9b 5a 98 |A.$Y.......[..Z.|
  0090: 9a 18 26 9b a6 19 98 1a 99 99 26 a6 18 9a 98 24 |..&.......&....$|
  00a0: 26 59 a6 25 5a 98 a5 18 a6 24 71 41 35 b1 43 dc |&Y.%Z....$qA5.C.|
  00b0: 16 b2 83 f7 e9 45 8b d2 56 c7 a3 1f 82 52 d7 8a |.....E..V....R..|
  00c0: 78 ed fc d5 76 f1 36 35 dc 05 00 36 ed 5e c7    |x...v.65...6.^.|

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

Incremental pull doesn't fetch bundle

  $ hg clone -r 53245c60e682 -U http://localhost:$HGPORT partial-clone
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

  $ cd partial-clone
  $ hg pull
  pulling from http://localhost:$HGPORT/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ cd ..

Bundle with full content works

  $ hg -R server bundle --type gzip-v2 --base null -r tip full.hg
  2 changesets found

Again, we perform an extra check against bundle content changes. If this content
changes, clone bundles produced by new Mercurial versions may not be readable
by old clients.

  $ f --size --hexdump full.hg
  full.hg: size=406
  0000: 48 47 32 30 00 00 00 0e 43 6f 6d 70 72 65 73 73 |HG20....Compress|
  0010: 69 6f 6e 3d 47 5a 78 9c 63 60 60 90 e5 76 f6 70 |ion=GZx.c``..v.p|
  0020: f4 73 77 75 0f f2 0f 0d 60 00 02 46 06 76 a6 b2 |.swu....`..F.v..|
  0030: d4 a2 e2 cc fc 3c 03 23 06 06 e6 65 40 b1 4d c1 |.....<.#...e@.M.|
  0040: 2a 31 09 cf 9a 3a 52 04 b7 fc db f0 95 e5 a4 f4 |*1...:R.........|
  0050: 97 17 b2 c9 0c 14 00 02 e6 d9 99 25 1a a7 a4 99 |...........%....|
  0060: a4 a4 1a 5b 58 a4 19 27 9b a4 59 a4 1a 59 a4 99 |...[X..'..Y..Y..|
  0070: a4 59 26 5a 18 9a 18 59 5a 26 1a 27 27 25 99 a6 |.Y&Z...YZ&.''%..|
  0080: 99 1a 70 95 a4 16 97 70 19 28 18 70 a5 e5 e7 73 |..p....p.(.p...s|
  0090: 71 25 a6 a4 28 00 19 40 13 0e ac fa df ab ff 7b |q%..(..@.......{|
  00a0: 3f fb 92 dc 8b 1f 62 bb 9e b7 d7 d9 87 3d 5a 44 |?.....b......=ZD|
  00b0: ac 2f b0 a9 c3 66 1e 54 b9 26 08 a7 1a 1b 1a a7 |./...f.T.&......|
  00c0: 25 1b 9a 1b 99 19 9a 5a 18 9b a6 18 19 00 dd 67 |%......Z.......g|
  00d0: 61 61 98 06 f4 80 49 4a 8a 65 52 92 41 9a 81 81 |aa....IJ.eR.A...|
  00e0: a5 11 17 50 31 30 58 19 cc 80 98 25 29 b1 08 c4 |...P10X....%)...|
  00f0: 37 07 79 19 88 d9 41 ee 07 8a 41 cd 5d 98 65 fb |7.y...A...A.].e.|
  0100: e5 9e 45 bf 8d 7f 9f c6 97 9f 2b 44 34 67 d9 ec |..E.......+D4g..|
  0110: 8e 0f a0 61 a8 eb 82 82 2e c9 c2 20 25 d5 34 c5 |...a....... %.4.|
  0120: d0 d8 c2 dc d4 c2 d4 c4 30 d9 34 cd c0 d4 c8 cc |........0.4.....|
  0130: 34 31 c5 d0 c4 24 31 c9 32 2d d1 c2 2c c5 30 25 |41...$1.2-..,.0%|
  0140: 09 e4 ee 85 8f 85 ff 88 ab 89 36 c7 2a c4 47 34 |..........6.*.G4|
  0150: fe f8 ec 7b 73 37 3f c3 24 62 1d 8d 4d 1d 9e 40 |...{s7?.$b..M..@|
  0160: 06 3b 10 14 36 a4 38 10 04 d8 21 01 9a b1 83 f7 |.;..6.8...!.....|
  0170: e9 45 8b d2 56 c7 a3 1f 82 52 d7 8a 78 ed fc d5 |.E..V....R..x...|
  0180: 76 f1 36 25 81 89 c7 ad ec 90 34 48 75 2b 89 49 |v.6%......4Hu+.I|
  0190: bf 00 d6 97 f0 8d                               |......|

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

Feature works over SSH

  $ hg clone -U -e "python \"$TESTDIR/dummyssh\"" ssh://user@dummy/server ssh-full-clone
  applying clone bundle from http://localhost:$HGPORT1/full.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

Entry with unknown BUNDLESPEC is filtered and not used

  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://bad.entry1 BUNDLESPEC=UNKNOWN
  > http://bad.entry2 BUNDLESPEC=xz-v1
  > http://bad.entry3 BUNDLESPEC=none-v100
  > http://localhost:$HGPORT1/full.hg BUNDLESPEC=gzip-v2
  > EOF

  $ hg clone -U http://localhost:$HGPORT filter-unknown-type
  applying clone bundle from http://localhost:$HGPORT1/full.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

Automatic fallback when all entries are filtered

  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://bad.entry BUNDLESPEC=UNKNOWN
  > EOF

  $ hg clone -U http://localhost:$HGPORT filter-all
  no compatible clone bundles available on server; falling back to regular clone
  (you may want to report this to the server operator)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

URLs requiring SNI are filtered in Python <2.7.9

  $ cp full.hg sni.hg
  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://localhost:$HGPORT1/sni.hg REQUIRESNI=true
  > http://localhost:$HGPORT1/full.hg
  > EOF

#if sslcontext
Python 2.7.9+ support SNI

  $ hg clone -U http://localhost:$HGPORT sni-supported
  applying clone bundle from http://localhost:$HGPORT1/sni.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found
#else
Python <2.7.9 will filter SNI URLs

  $ hg clone -U http://localhost:$HGPORT sni-unsupported
  applying clone bundle from http://localhost:$HGPORT1/full.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found
#endif

Stream clone bundles are supported

  $ hg -R server debugcreatestreamclonebundle packed.hg
  writing 613 bytes for 4 files
  bundle requirements: generaldelta, revlogv1

No bundle spec should work

  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://localhost:$HGPORT1/packed.hg
  > EOF

  $ hg clone -U http://localhost:$HGPORT stream-clone-no-spec
  applying clone bundle from http://localhost:$HGPORT1/packed.hg
  4 files to transfer, 613 bytes of data
  transferred 613 bytes in *.* seconds (*) (glob)
  finished applying clone bundle
  searching for changes
  no changes found

Bundle spec without parameters should work

  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://localhost:$HGPORT1/packed.hg BUNDLESPEC=none-packed1
  > EOF

  $ hg clone -U http://localhost:$HGPORT stream-clone-vanilla-spec
  applying clone bundle from http://localhost:$HGPORT1/packed.hg
  4 files to transfer, 613 bytes of data
  transferred 613 bytes in *.* seconds (*) (glob)
  finished applying clone bundle
  searching for changes
  no changes found

Bundle spec with format requirements should work

  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://localhost:$HGPORT1/packed.hg BUNDLESPEC=none-packed1;requirements%3Drevlogv1
  > EOF

  $ hg clone -U http://localhost:$HGPORT stream-clone-supported-requirements
  applying clone bundle from http://localhost:$HGPORT1/packed.hg
  4 files to transfer, 613 bytes of data
  transferred 613 bytes in *.* seconds (*) (glob)
  finished applying clone bundle
  searching for changes
  no changes found

Stream bundle spec with unknown requirements should be filtered out

  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://localhost:$HGPORT1/packed.hg BUNDLESPEC=none-packed1;requirements%3Drevlogv42
  > EOF

  $ hg clone -U http://localhost:$HGPORT stream-clone-unsupported-requirements
  no compatible clone bundles available on server; falling back to regular clone
  (you may want to report this to the server operator)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files

Set up manifest for testing preferences
(Remember, the TYPE does not have to match reality - the URL is
important)

  $ cp full.hg gz-a.hg
  $ cp full.hg gz-b.hg
  $ cp full.hg bz2-a.hg
  $ cp full.hg bz2-b.hg
  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://localhost:$HGPORT1/gz-a.hg BUNDLESPEC=gzip-v2 extra=a
  > http://localhost:$HGPORT1/bz2-a.hg BUNDLESPEC=bzip2-v2 extra=a
  > http://localhost:$HGPORT1/gz-b.hg BUNDLESPEC=gzip-v2 extra=b
  > http://localhost:$HGPORT1/bz2-b.hg BUNDLESPEC=bzip2-v2 extra=b
  > EOF

Preferring an undefined attribute will take first entry

  $ hg --config ui.clonebundleprefers=foo=bar clone -U http://localhost:$HGPORT prefer-foo
  applying clone bundle from http://localhost:$HGPORT1/gz-a.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

Preferring bz2 type will download first entry of that type

  $ hg --config ui.clonebundleprefers=COMPRESSION=bzip2 clone -U http://localhost:$HGPORT prefer-bz
  applying clone bundle from http://localhost:$HGPORT1/bz2-a.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

Preferring multiple values of an option works

  $ hg --config ui.clonebundleprefers=COMPRESSION=unknown,COMPRESSION=bzip2 clone -U http://localhost:$HGPORT prefer-multiple-bz
  applying clone bundle from http://localhost:$HGPORT1/bz2-a.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

Sorting multiple values should get us back to original first entry

  $ hg --config ui.clonebundleprefers=BUNDLESPEC=unknown,BUNDLESPEC=gzip-v2,BUNDLESPEC=bzip2-v2 clone -U http://localhost:$HGPORT prefer-multiple-gz
  applying clone bundle from http://localhost:$HGPORT1/gz-a.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

Preferring multiple attributes has correct order

  $ hg --config ui.clonebundleprefers=extra=b,BUNDLESPEC=bzip2-v2 clone -U http://localhost:$HGPORT prefer-separate-attributes
  applying clone bundle from http://localhost:$HGPORT1/bz2-b.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

Test where attribute is missing from some entries

  $ cat > server/.hg/clonebundles.manifest << EOF
  > http://localhost:$HGPORT1/gz-a.hg BUNDLESPEC=gzip-v2
  > http://localhost:$HGPORT1/bz2-a.hg BUNDLESPEC=bzip2-v2
  > http://localhost:$HGPORT1/gz-b.hg BUNDLESPEC=gzip-v2 extra=b
  > http://localhost:$HGPORT1/bz2-b.hg BUNDLESPEC=bzip2-v2 extra=b
  > EOF

  $ hg --config ui.clonebundleprefers=extra=b clone -U http://localhost:$HGPORT prefer-partially-defined-attribute
  applying clone bundle from http://localhost:$HGPORT1/gz-b.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  finished applying clone bundle
  searching for changes
  no changes found

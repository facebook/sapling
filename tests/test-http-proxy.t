#require serve
  $ cat << EOF >> $HGRCPATH
  > [experimental]
  > # drop me once bundle2 is the default,
  > # added to get test change early.
  > bundle2-exp = True
  > EOF

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -Ama -d '1123456789 0'
  adding a
  $ hg --config server.uncompressed=True serve -p $HGPORT -d --pid-file=hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..
  $ "$TESTDIR/tinyproxy.py" $HGPORT1 localhost >proxy.log 2>&1 </dev/null &
  $ while [ ! -f proxy.pid ]; do sleep 0; done
  $ cat proxy.pid >> $DAEMON_PIDS

url for proxy, stream

  $ http_proxy=http://localhost:$HGPORT1/ hg --config http_proxy.always=True clone --uncompressed http://localhost:$HGPORT/ b
  streaming all changes
  3 files to transfer, 303 bytes of data
  transferred * bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd b
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  $ cd ..

url for proxy, pull

  $ http_proxy=http://localhost:$HGPORT1/ hg --config http_proxy.always=True clone http://localhost:$HGPORT/ b-pull
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd b-pull
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  $ cd ..

host:port for proxy

  $ http_proxy=localhost:$HGPORT1 hg clone --config http_proxy.always=True http://localhost:$HGPORT/ c
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

proxy url with user name and password

  $ http_proxy=http://user:passwd@localhost:$HGPORT1 hg clone --config http_proxy.always=True http://localhost:$HGPORT/ d
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

url with user name and password

  $ http_proxy=http://user:passwd@localhost:$HGPORT1 hg clone --config http_proxy.always=True http://user:passwd@localhost:$HGPORT/ e
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

bad host:port for proxy

  $ http_proxy=localhost:$HGPORT2 hg clone --config http_proxy.always=True http://localhost:$HGPORT/ f
  abort: error: Connection refused
  [255]

do not use the proxy if it is in the no list

  $ http_proxy=localhost:$HGPORT1 hg clone --config http_proxy.no=localhost http://localhost:$HGPORT/ g
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat proxy.log
  * - - [*] "GET http://localhost:$HGPORT/?cmd=capabilities HTTP/1.1" - - (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=branchmap HTTP/1.1" - - (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=stream_out HTTP/1.1" - - (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=batch HTTP/1.1" - - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D83180e7845de420a1bb46896fd5fe05294f8d629 (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=getbundle HTTP/1.1" - - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=0&common=83180e7845de420a1bb46896fd5fe05294f8d629&heads=83180e7845de420a1bb46896fd5fe05294f8d629&listkeys=phase%2Cbookmarks (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=listkeys HTTP/1.1" - - x-hgarg-1:namespace=phases (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=capabilities HTTP/1.1" - - (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=batch HTTP/1.1" - - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=getbundle HTTP/1.1" - - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=83180e7845de420a1bb46896fd5fe05294f8d629&listkeys=phase%2Cbookmarks (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=listkeys HTTP/1.1" - - x-hgarg-1:namespace=phases (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=capabilities HTTP/1.1" - - (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=batch HTTP/1.1" - - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=getbundle HTTP/1.1" - - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=83180e7845de420a1bb46896fd5fe05294f8d629&listkeys=phase%2Cbookmarks (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=listkeys HTTP/1.1" - - x-hgarg-1:namespace=phases (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=capabilities HTTP/1.1" - - (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=batch HTTP/1.1" - - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=getbundle HTTP/1.1" - - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=83180e7845de420a1bb46896fd5fe05294f8d629&listkeys=phase%2Cbookmarks (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=listkeys HTTP/1.1" - - x-hgarg-1:namespace=phases (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=capabilities HTTP/1.1" - - (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=batch HTTP/1.1" - - x-hgarg-1:cmds=heads+%3Bknown+nodes%3D (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=getbundle HTTP/1.1" - - x-hgarg-1:bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=83180e7845de420a1bb46896fd5fe05294f8d629&listkeys=phase%2Cbookmarks (glob)
  * - - [*] "GET http://localhost:$HGPORT/?cmd=listkeys HTTP/1.1" - - x-hgarg-1:namespace=phases (glob)

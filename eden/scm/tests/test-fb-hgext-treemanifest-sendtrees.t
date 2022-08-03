#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > pushrebase=
  > remotenames=
  > [treemanifest]
  > sendtrees=True
  > EOF

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

Make local commits on the server
  $ mkdir subdir
  $ echo x > subdir/x
  $ hg commit -qAm 'add subdir/x'
  $ hg book master

The following will turn on sendtrees mode for a hybrid client and verify it
sends them during a push and during bundle operations.

Create flat manifest clients
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client1 -q
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
  $ hgcloneshallow ssh://user@dummy/master client2 -q

Transition to hybrid flat+tree client
  $ cat >> client1/.hg/hgrc <<EOF
  > [extensions]
  > amend=
  > [treemanifest]
  > demanddownload=True
  > EOF
  $ cat >> client2/.hg/hgrc <<EOF
  > [extensions]
  > amend=
  > [treemanifest]
  > demanddownload=True
  > EOF

Make a draft commit
  $ cd client1
  $ echo f >> subdir/x
  $ hg commit -qm "hybrid commit"
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.
Test bundling/unbundling
  $ hg bundle -r . --base '.^' ../treebundle.hg --debug 2>&1 | grep treegroup
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload

  $ cd ../client2
  $ hg unbundle ../treebundle.hg --debug 2>&1 | grep treegroup
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.
Test pushing
  $ hg push -r tip --to master --debug 2>&1 2>&1 | grep rebasepackpart
  bundle2-output-part: "b2x:rebasepackpart" (params: 3 mandatory) streamed payload

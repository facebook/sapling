
  $ setconfig scmstore.cas-mode=on

  $ setconfig push.edenapi=true
  $ setconfig scmstore.tree-metadata-mode=always

  $ newserver server

  $ newclientrepo client server
  $ drawdag <<EOS
  > A # A/dir/foo = foo
  >   # A/dir/bar = bar
  > EOS
  $ hg push -qr $A --to main --create

Reset local repo stores
  $ newclientrepo client2 server
  $ hg pull -qr $A

First fetch aux data for root dir (needed to for subsequent fetch).
  $ hg debugscmstore --mode tree -r $A "" >/dev/null

--store-model uses the storemodel trait (which is what EdenFS uses)
FIXME: missing aux data
  $ hg debugscmstore --mode tree -r $A "dir" --store-model
  Tree 'dir' entries
    (PathComponentBuf("bar"), HgId("a324b8bf63f7d56de9d36f8747e3b68a72a4d968"), File(Regular))
    (PathComponentBuf("foo"), HgId("49d8cbb15ce257920447006b46978b7af980a979"), File(Regular))
  Tree 'dir' file aux

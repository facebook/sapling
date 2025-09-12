#testcases cas no-cas

#if cas
  $ setconfig scmstore.cas-mode=on
#else
  $ setconfig scmstore.cas-mode=off
#endif

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
  $ hg debugscmstore --mode tree -r $A "dir" --store-model
  Tree 'dir' entries
    (PathComponent("bar"), HgId("a324b8bf63f7d56de9d36f8747e3b68a72a4d968"), File(Regular))
    (PathComponent("foo"), HgId("49d8cbb15ce257920447006b46978b7af980a979"), File(Regular))
  Tree 'dir' file aux
    (HgId("49d8cbb15ce257920447006b46978b7af980a979"), FileAuxData { total_size: 3, sha1: Sha1("0beec7b5ea3f0fdbc95d0dd47f3c5bc275da8a33"), blake3: Blake3("29b9cb87c1ac3fd85f385e97eed4ba6b0bc64435cc1985815d2bef6cde472233"), file_header_metadata: Some(b"") })
    (HgId("a324b8bf63f7d56de9d36f8747e3b68a72a4d968"), FileAuxData { total_size: 3, sha1: Sha1("62cdb7020ff920e5aa642c3d4066950dd1f01f4d"), blake3: Blake3("0f6cb4c2a4c391b24fdbb4023e5d8c9ad7fbadaecfdc69cc801cf9d08dffd32a"), file_header_metadata: Some(b"") })

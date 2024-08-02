  $ setconfig push.edenapi=true

First sanity check eagerepo works as CAS store
  $ newclientrepo
  $ echo "A" | drawdag

Remote doesn't know about commit yet.
  $ hg debugcas -r $A A
  path A, node 005d992c5dcf32993668f7cede29d296c494a5d9, digest CasDigest { hash: Blake3("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"), size: 1 }, not found in CAS

  $ hg push -qr $A --to main --create

Now remote knows about our data.
  $ hg debugcas -r $A A
  path A, node 005d992c5dcf32993668f7cede29d296c494a5d9, digest CasDigest { hash: Blake3("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"), size: 1 }, contents:
  A


#debugruntest-compatible

#require eden

  $ eagerepo
  $ setconfig clone.use-rust=true

  $ newrepo server
  $ echo foo > included
  $ echo foo > excluded
  $ echo path:included > sparse
  $ touch eden-sparse
  $ hg commit -Aqm a

  $ cd
  $ hg clone -q --eden test:server client --config clone.eden-sparse-filter=eden-sparse
  $ cd client

Allow adhoc use of sparse commands to debug sparse profiles:
  $ hg debugsparsematch -q --sparse-profile=sparse excluded --config extensions.sparse=

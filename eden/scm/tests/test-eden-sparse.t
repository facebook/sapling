
#require eden

  $ eagerepo
  $ enable edensparse
  $ setconfig clone.use-rust=true

  $ newrepo server
  $ echo foo > included
  $ echo foo > excluded
  $ echo path:included > sparse
  $ touch eden-sparse
  $ hg commit -Aqm a
  $ hg book master

  $ cd
  $ hg clone -q --eden test:server client --config clone.eden-sparse-filter=eden-sparse
  $ cd client

Allow adhoc use of sparse commands to debug sparse profiles:
  $ hg debugsparsematch -q --sparse-profile=sparse excluded --config extensions.sparse=

Test diff command against a commit that updated files excluded by the sparse profile

  $ cd
  $ newrepo server-diff
  $ echo aaa > a.txt
  $ hg commit -Aqm a
  $ echo bbb > b.txt
  $ hg commit -Aqm b
  $ echo ccc > a.txt
  $ echo ccc > b.txt
  $ hg commit -Aqm c
  $ cat >> eden-sparse << EOF
  > [include]
  > *
  > [exclude]
  > b.txt
  > EOF
  $ hg commit -Aqm d
  $ hg book master

  $ cd
  $ hg clone -q --eden test:server-diff client-diff --config clone.eden-sparse-filter=eden-sparse
  $ cd client-diff
  $ hg diff -r 'desc(b)' --stat
   a.txt       |  2 +-
   b.txt       |  2 +-
   eden-sparse |  4 ++++
   3 files changed, 6 insertions(+), 2 deletions(-)
  $ hg diff -r 'desc(b)' --stat --sparse
   a.txt       |  2 +-
   eden-sparse |  4 ++++
   2 files changed, 5 insertions(+), 1 deletions(-)

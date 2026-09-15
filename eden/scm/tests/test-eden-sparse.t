
#require eden

  $ eagerepo
  $ enable edensparse
  $ setconfig clone.use-rust=true

  $ newrepo server
  $ echo foo > included
  $ echo foo > excluded
  $ echo path:included > sparse
  $ touch eden-sparse
  $ sl commit -Aqm a
  $ sl book master

  $ cd
  $ sl clone -q --eden test:server client --config clone.eden-sparse-filter=eden-sparse
  $ cd client

Allow adhoc use of sparse commands to debug sparse profiles:
  $ sl debugsparsematch -q --sparse-profile=sparse excluded --config extensions.sparse=

Reading a revision still works when sparse is loaded but edensparse is not. The
repo keeps the edensparse requirement while neither reposetup supplies
sparsematch(), so cat must not assume the method exists:
  $ sl cat -r . included --config extensions.sparse= --config extensions.edensparse=!
  foo

Test diff command against a commit that updated files excluded by the sparse profile

  $ cd
  $ newrepo server-diff
  $ echo aaa > a.txt
  $ sl commit -Aqm a
  $ echo bbb > b.txt
  $ sl commit -Aqm b
  $ echo ccc > a.txt
  $ echo ccc > b.txt
  $ sl commit -Aqm c
  $ cat >> eden-sparse << EOF
  > [include]
  > *
  > [exclude]
  > b.txt
  > EOF
  $ sl commit -Aqm d
  $ sl book master

  $ cd
  $ sl clone -q --eden test:server-diff client-diff --config clone.eden-sparse-filter=eden-sparse
  $ cd client-diff
  $ sl diff -r 'desc(b)' --stat
   a.txt       |  2 +-
   b.txt       |  2 +-
   eden-sparse |  4 ++++
   3 files changed, 6 insertions(+), 2 deletions(-)
  $ sl diff -r 'desc(b)' --stat --sparse
   a.txt       |  2 +-
   eden-sparse |  4 ++++
   2 files changed, 5 insertions(+), 1 deletions(-)

Don't crash with extensions in this state
  $ sl diff -r 'desc(b)' --stat --config extensions.sparse= --config extensions.edensparse=!
  abort: *$ENOENT$* (glob)
  [255]

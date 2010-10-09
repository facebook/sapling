
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > notify=
  > 
  > [hooks]
  > changegroup.notify = python:hgext.notify.hook
  > 
  > [notify]
  > sources = push
  > diffstat = False
  > maxsubject = 10
  > 
  > [usersubs]
  > foo@bar = *
  > 
  > [reposubs]
  > * = baz
  > EOF
  $ hg init a

clone

  $ hg --traceback clone a b
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a > b/a

commit

  $ hg --traceback --cwd b commit -Ama
  adding a
  $ echo a >> b/a

commit

  $ hg --traceback --cwd b commit -Amb

push

  $ hg --traceback --cwd b push ../a 2>&1 |
  >     python -c 'import sys,re; print re.sub("\n\t", " ", sys.stdin.read()),'
  pushing to ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Date: * (glob)
  Subject: * (glob)
  From: test
  X-Hg-Notification: changeset cb9a9f314b8b
  Message-Id: <*> (glob)
  To: baz, foo@bar
  
  changeset cb9a9f314b8b in $TESTTMP/a
  details: $TESTTMP/a?cmd=changeset;node=cb9a9f314b8b
  summary: a
  
  changeset ba677d0156c1 in $TESTTMP/a
  details: $TESTTMP/a?cmd=changeset;node=ba677d0156c1
  summary: b
  
  diffs (6 lines):
  
  diff -r 000000000000 -r ba677d0156c1 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +a
  +a


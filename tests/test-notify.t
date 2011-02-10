
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > notify=
  > 
  > [hooks]
  > incoming.notify = python:hgext.notify.hook
  > 
  > [notify]
  > sources = pull
  > diffstat = False
  > 
  > [usersubs]
  > foo@bar = *
  > 
  > [reposubs]
  > * = baz
  > EOF
  $ hg help notify
  notify extension - hooks for sending email notifications at commit/push time
  
  Subscriptions can be managed through a hgrc file. Default mode is to print
  messages to stdout, for testing and configuring.
  
  To use, configure the notify extension and enable it in hgrc like this:
  
    [extensions]
    notify =
  
    [hooks]
    # one email for each incoming changeset
    incoming.notify = python:hgext.notify.hook
    # batch emails when many changesets incoming at one time
    changegroup.notify = python:hgext.notify.hook
  
    [notify]
    # config items go here
  
  Required configuration items:
  
    config = /path/to/file # file containing subscriptions
  
  Optional configuration items:
  
    test = True            # print messages to stdout for testing
    strip = 3              # number of slashes to strip for url paths
    domain = example.com   # domain to use if committer missing domain
    style = ...            # style file to use when formatting email
    template = ...         # template to use when formatting email
    incoming = ...         # template to use when run as incoming hook
    changegroup = ...      # template when run as changegroup hook
    maxdiff = 300          # max lines of diffs to include (0=none, -1=all)
    maxsubject = 67        # truncate subject line longer than this
    diffstat = True        # add a diffstat before the diff content
    sources = serve        # notify if source of incoming changes in this list
                           # (serve == ssh or http, push, pull, bundle)
    merge = False          # send notification for merges (default True)
    [email]
    from = user@host.com   # email address to send as if none given
    [web]
    baseurl = http://hgserver/... # root of hg web site for browsing commits
  
  The notify config file has same format as a regular hgrc file. It has two
  sections so you can express subscriptions in whatever way is handier for you.
  
    [usersubs]
    # key is subscriber email, value is ","-separated list of glob patterns
    user@host = pattern
  
    [reposubs]
    # key is glob pattern, value is ","-separated list of subscriber emails
    pattern = user@host
  
  Glob patterns are matched against path to repository root.
  
  If you like, you can put notify config file in repository that users can push
  changes to, they can manage their own subscriptions.
  
  no commands defined
  $ hg init a
  $ echo a > a/a

commit

  $ hg --cwd a commit -Ama -d '0 0'
  adding a


clone

  $ hg --traceback clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a >> a/a

commit

  $ hg --traceback --cwd a commit -Amb -d '1 0'

on Mac OS X 10.5 the tmp path is very long so would get stripped in the subject line

  $ cat <<EOF >> $HGRCPATH
  > [notify]
  > maxsubject = 200
  > EOF

the python call below wraps continuation lines, which appear on Mac OS X 10.5 because
of the very long subject line
pull (minimal config)

  $ hg --traceback --cwd b pull ../a | \
  >   python -c 'import sys,re; print re.sub("\n[\t ]", " ", sys.stdin.read()),'
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Date: * (glob)
  Subject: changeset in $TESTTMP/b: b
  From: test
  X-Hg-Notification: changeset 0647d048b600
  Message-Id: <*> (glob)
  To: baz, foo@bar
  
  changeset 0647d048b600 in $TESTTMP/b
  details: $TESTTMP/b?cmd=changeset;node=0647d048b600
  description: b
  
  diffs (6 lines):
  
  diff -r cb9a9f314b8b -r 0647d048b600 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -1,1 +1,2 @@ a
  +a
  (run 'hg update' to get a working copy)
  $ cat <<EOF >> $HGRCPATH
  > [notify]
  > config = `pwd`/.notify.conf
  > domain = test.com
  > strip = 42
  > template = Subject: {desc|firstline|strip}\nFrom: {author}\nX-Test: foo\n\nchangeset {node|short} in {webroot}\ndescription:\n\t{desc|tabindent|strip}
  > 
  > [web]
  > baseurl = http://test/
  > EOF

fail for config file is missing

  $ hg --cwd b rollback
  repository tip rolled back to revision 0 (undo pull)
  working directory now based on revision 0
  $ hg --cwd b pull ../a 2>&1 | grep 'error.*\.notify\.conf' > /dev/null && echo pull failed
  pull failed
  $ touch ".notify.conf"

pull

  $ hg --cwd b rollback
  repository tip rolled back to revision 0 (undo pull)
  working directory now based on revision 0
  $ hg --traceback --cwd b pull ../a  | \
  >   python -c 'import sys,re; print re.sub("\n\t", " ", sys.stdin.read()),'
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  X-Test: foo
  Date: * (glob)
  Subject: b
  From: test@test.com
  X-Hg-Notification: changeset 0647d048b600
  Message-Id: <*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 0647d048b600 in b
  description: b
  diffs (6 lines):
  
  diff -r cb9a9f314b8b -r 0647d048b600 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -1,1 +1,2 @@
   a
  +a
  (run 'hg update' to get a working copy)

  $ cat << EOF >> $HGRCPATH
  > [hooks]
  > incoming.notify = python:hgext.notify.hook
  > 
  > [notify]
  > sources = pull
  > diffstat = True
  > EOF

pull

  $ hg --cwd b rollback
  repository tip rolled back to revision 0 (undo pull)
  working directory now based on revision 0
  $ hg --traceback --cwd b pull ../a | \
  >   python -c 'import sys,re; print re.sub("\n\t", " ", sys.stdin.read()),'
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  X-Test: foo
  Date: * (glob)
  Subject: b
  From: test@test.com
  X-Hg-Notification: changeset 0647d048b600
  Message-Id: <*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 0647d048b600 in b
  description: b
  diffstat:
  
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  diffs (6 lines):
  
  diff -r cb9a9f314b8b -r 0647d048b600 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -1,1 +1,2 @@
   a
  +a
  (run 'hg update' to get a working copy)

test merge

  $ cd a
  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a >> a
  $ hg ci -Am adda2 -d '2 0'
  created new head
  $ hg merge
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge -d '3 0'
  $ cd ..
  $ hg --traceback --cwd b pull ../a | \
  >   python -c 'import sys,re; print re.sub("\n\t", " ", sys.stdin.read()),'
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  X-Test: foo
  Date: * (glob)
  Subject: adda2
  From: test@test.com
  X-Hg-Notification: changeset 0a184ce6067f
  Message-Id: <*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 0a184ce6067f in b
  description: adda2
  diffstat:
  
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  diffs (6 lines):
  
  diff -r cb9a9f314b8b -r 0a184ce6067f a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:02 1970 +0000
  @@ -1,1 +1,2 @@
   a
  +a
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  X-Test: foo
  Date: * (glob)
  Subject: merge
  From: test@test.com
  X-Hg-Notification: changeset 22c88b85aa27
  Message-Id: <*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 22c88b85aa27 in b
  description: merge
  (run 'hg update' to get a working copy)

truncate multi-byte subject

  $ cat <<EOF >> $HGRCPATH
  > [notify]
  > maxsubject = 4
  > EOF
  $ echo a >> a/a
  $ hg --cwd a --encoding utf-8 commit -A -d '0 0' \
  >   -m `python -c 'print "\xc3\xa0\xc3\xa1\xc3\xa2\xc3\xa3\xc3\xa4"'`
  $ hg --traceback --cwd b --encoding utf-8 pull ../a | \
  >   python -c 'import sys,re; print re.sub("\n\t", " ", sys.stdin.read()),'
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 8bit
  X-Test: foo
  Date: * (glob)
  Subject: \xc3\xa0... (esc)
  From: test@test.com
  X-Hg-Notification: changeset 4a47f01c1356
  Message-Id: <*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 4a47f01c1356 in b
  description: \xc3\xa0\xc3\xa1\xc3\xa2\xc3\xa3\xc3\xa4 (esc)
  diffstat:
  
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  diffs (7 lines):
  
  diff -r 22c88b85aa27 -r 4a47f01c1356 a
  --- a/a	Thu Jan 01 00:00:03 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   a
   a
  +a
  (run 'hg update' to get a working copy)

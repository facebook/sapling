
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
  notify extension - hooks for sending email push notifications
  
  This extension let you run hooks sending email notifications when changesets
  are being pushed, from the sending or receiving side.
  
  First, enable the extension as explained in "hg help extensions", and register
  the hook you want to run. "incoming" and "outgoing" hooks are run by the
  changesets receiver while the "outgoing" one is for the sender:
  
    [hooks]
    # one email for each incoming changeset
    incoming.notify = python:hgext.notify.hook
    # one email for all incoming changesets
    changegroup.notify = python:hgext.notify.hook
  
    # one email for all outgoing changesets
    outgoing.notify = python:hgext.notify.hook
  
  Now the hooks are running, subscribers must be assigned to repositories. Use
  the "[usersubs]" section to map repositories to a given email or the
  "[reposubs]" section to map emails to a single repository:
  
    [usersubs]
    # key is subscriber email, value is a comma-separated list of glob
    # patterns
    user@host = pattern
  
    [reposubs]
    # key is glob pattern, value is a comma-separated list of subscriber
    # emails
    pattern = user@host
  
  Glob patterns are matched against absolute path to repository root. The
  subscriptions can be defined in their own file and referenced with:
  
    [notify]
    config = /path/to/subscriptionsfile
  
  Alternatively, they can be added to Mercurial configuration files by setting
  the previous entry to an empty value.
  
  At this point, notifications should be generated but will not be sent until
  you set the "notify.test" entry to "False".
  
  Notifications content can be tweaked with the following configuration entries:
  
  notify.test
    If "True", print messages to stdout instead of sending them. Default: True.
  
  notify.sources
    Space separated list of change sources. Notifications are sent only if it
    includes the incoming or outgoing changes source. Incoming sources can be
    "serve" for changes coming from http or ssh, "pull" for pulled changes,
    "unbundle" for changes added by "hg unbundle" or "push" for changes being
    pushed locally. Outgoing sources are the same except for "unbundle" which is
    replaced by "bundle". Default: serve.
  
  notify.strip
    Number of leading slashes to strip from url paths. By default, notifications
    references repositories with their absolute path. "notify.strip" let you
    turn them into relative paths. For example, "notify.strip=3" will change
    "/long/path/repository" into "repository". Default: 0.
  
  notify.domain
    If subscribers emails or the from email have no domain set, complete them
    with this value.
  
  notify.style
    Style file to use when formatting emails.
  
  notify.template
    Template to use when formatting emails.
  
  notify.incoming
    Template to use when run as incoming hook, override "notify.template".
  
  notify.outgoing
    Template to use when run as outgoing hook, override "notify.template".
  
  notify.changegroup
    Template to use when running as changegroup hook, override
    "notify.template".
  
  notify.maxdiff
    Maximum number of diff lines to include in notification email. Set to 0 to
    disable the diff, -1 to include all of it. Default: 300.
  
  notify.maxsubject
    Maximum number of characters in emails subject line. Default: 67.
  
  notify.diffstat
    Set to True to include a diffstat before diff content. Default: True.
  
  notify.merge
    If True, send notifications for merge changesets. Default: True.
  
  If set, the following entries will also be used to customize the
  notifications:
  
  email.from
    Email "From" address to use if none can be found in generated email content.
  
  web.baseurl
    Root repository browsing URL to combine with repository paths when making
    references. See also "notify.strip".
  
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
  $ hg --cwd b pull ../a 2>&1 | grep 'error.*\.notify\.conf' > /dev/null && echo pull failed
  pull failed
  $ touch ".notify.conf"

pull

  $ hg --cwd b rollback
  repository tip rolled back to revision 0 (undo pull)
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
  X-Hg-Notification: changeset 6a0cf76b2701
  Message-Id: <*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 6a0cf76b2701 in b
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
  X-Hg-Notification: changeset 7ea05ad269dc
  Message-Id: <*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 7ea05ad269dc in b
  description: \xc3\xa0\xc3\xa1\xc3\xa2\xc3\xa3\xc3\xa4 (esc)
  diffstat:
  
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  diffs (7 lines):
  
  diff -r 6a0cf76b2701 -r 7ea05ad269dc a
  --- a/a	Thu Jan 01 00:00:03 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   a
   a
  +a
  (run 'hg update' to get a working copy)

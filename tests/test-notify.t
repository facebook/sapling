  $ cat > $TESTTMP/filter.py <<EOF
  > from __future__ import absolute_import, print_function
  > import re
  > import sys
  > print(re.sub("\n[ \t]", " ", sys.stdin.read()), end="")
  > EOF

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
  
  This extension implements hooks to send email notifications when changesets
  are sent from or received by the local repository.
  
  First, enable the extension as explained in 'hg help extensions', and register
  the hook you want to run. "incoming" and "changegroup" hooks are run when
  changesets are received, while "outgoing" hooks are for changesets sent to
  another repository:
  
    [hooks]
    # one email for each incoming changeset
    incoming.notify = python:hgext.notify.hook
    # one email for all incoming changesets
    changegroup.notify = python:hgext.notify.hook
  
    # one email for all outgoing changesets
    outgoing.notify = python:hgext.notify.hook
  
  This registers the hooks. To enable notification, subscribers must be assigned
  to repositories. The "[usersubs]" section maps multiple repositories to a
  given recipient. The "[reposubs]" section maps multiple recipients to a single
  repository:
  
    [usersubs]
    # key is subscriber email, value is a comma-separated list of repo patterns
    user@host = pattern
  
    [reposubs]
    # key is repo pattern, value is a comma-separated list of subscriber emails
    pattern = user@host
  
  A "pattern" is a "glob" matching the absolute path to a repository, optionally
  combined with a revset expression. A revset expression, if present, is
  separated from the glob by a hash. Example:
  
    [reposubs]
    */widgets#branch(release) = qa-team@example.com
  
  This sends to "qa-team@example.com" whenever a changeset on the "release"
  branch triggers a notification in any repository ending in "widgets".
  
  In order to place them under direct user management, "[usersubs]" and
  "[reposubs]" sections may be placed in a separate "hgrc" file and incorporated
  by reference:
  
    [notify]
    config = /path/to/subscriptionsfile
  
  Notifications will not be sent until the "notify.test" value is set to
  "False"; see below.
  
  Notifications content can be tweaked with the following configuration entries:
  
  notify.test
    If "True", print messages to stdout instead of sending them. Default: True.
  
  notify.sources
    Space-separated list of change sources. Notifications are activated only
    when a changeset's source is in this list. Sources may be:
  
    "serve"       changesets received via http or ssh
    "pull"        changesets received via "hg pull"
    "unbundle"    changesets received via "hg unbundle"
    "push"        changesets sent or received via "hg push"
    "bundle"      changesets sent via "hg unbundle"
  
    Default: serve.
  
  notify.strip
    Number of leading slashes to strip from url paths. By default, notifications
    reference repositories with their absolute path. "notify.strip" lets you
    turn them into relative paths. For example, "notify.strip=3" will change
    "/long/path/repository" into "repository". Default: 0.
  
  notify.domain
    Default email domain for sender or recipients with no explicit domain.
  
  notify.style
    Style file to use when formatting emails.
  
  notify.template
    Template to use when formatting emails.
  
  notify.incoming
    Template to use when run as an incoming hook, overriding "notify.template".
  
  notify.outgoing
    Template to use when run as an outgoing hook, overriding "notify.template".
  
  notify.changegroup
    Template to use when running as a changegroup hook, overriding
    "notify.template".
  
  notify.maxdiff
    Maximum number of diff lines to include in notification email. Set to 0 to
    disable the diff, or -1 to include all of it. Default: 300.
  
  notify.maxsubject
    Maximum number of characters in email's subject line. Default: 67.
  
  notify.diffstat
    Set to True to include a diffstat before diff content. Default: True.
  
  notify.merge
    If True, send notifications for merge changesets. Default: True.
  
  notify.mbox
    If set, append mails to this mbox file instead of sending. Default: None.
  
  notify.fromauthor
    If set, use the committer of the first changeset in a changegroup for the
    "From" field of the notification mail. If not set, take the user from the
    pushing repo.  Default: False.
  
  If set, the following entries will also be used to customize the
  notifications:
  
  email.from
    Email "From" address to use if none can be found in the generated email
    content.
  
  web.baseurl
    Root repository URL to combine with repository paths when making references.
    See also "notify.strip".
  
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

  $ hg --traceback --cwd b pull ../a | $PYTHON $TESTTMP/filter.py
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 0647d048b600
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  Date: * (glob)
  Subject: changeset in $TESTTMP/b: b
  From: test
  X-Hg-Notification: changeset 0647d048b600
  Message-Id: <*> (glob)
  To: baz, foo@bar
  
  changeset 0647d048b600 in $TESTTMP/b (glob)
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
  $ hg --traceback --cwd b pull ../a  | $PYTHON $TESTTMP/filter.py
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 0647d048b600
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
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
  @@ -1,1 +1,2 @@ a
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
  $ hg --traceback --cwd b pull ../a | $PYTHON $TESTTMP/filter.py
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 0647d048b600
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
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
   a |  1 + 1 files changed, 1 insertions(+), 0 deletions(-)
  
  diffs (6 lines):
  
  diff -r cb9a9f314b8b -r 0647d048b600 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -1,1 +1,2 @@ a
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
  $ hg --traceback --cwd b pull ../a | $PYTHON $TESTTMP/filter.py
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets 0a184ce6067f:6a0cf76b2701
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
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
   a |  1 + 1 files changed, 1 insertions(+), 0 deletions(-)
  
  diffs (6 lines):
  
  diff -r cb9a9f314b8b -r 0a184ce6067f a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:02 1970 +0000
  @@ -1,1 +1,2 @@ a
  +a
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
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

non-ascii content and truncation of multi-byte subject

  $ cat <<EOF >> $HGRCPATH
  > [notify]
  > maxsubject = 4
  > EOF
  $ echo a >> a/a
  $ hg --cwd a --encoding utf-8 commit -A -d '0 0' \
  >   -m `$PYTHON -c 'print "\xc3\xa0\xc3\xa1\xc3\xa2\xc3\xa3\xc3\xa4"'`
  $ hg --traceback --cwd b --encoding utf-8 pull ../a | \
  >   $PYTHON $TESTTMP/filter.py
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 7ea05ad269dc
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
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
   a |  1 + 1 files changed, 1 insertions(+), 0 deletions(-)
  
  diffs (7 lines):
  
  diff -r 6a0cf76b2701 -r 7ea05ad269dc a
  --- a/a	Thu Jan 01 00:00:03 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@ a a
  +a
  (run 'hg update' to get a working copy)

long lines

  $ cat <<EOF >> $HGRCPATH
  > [notify]
  > maxsubject = 67
  > test = False
  > mbox = mbox
  > EOF
  $ $PYTHON -c 'file("a/a", "ab").write("no" * 500 + "\xd1\x84" + "\n")'
  $ hg --cwd a commit -A -m "long line"
  $ hg --traceback --cwd b pull ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets a323cae54f6e
  notify: sending 2 subscribers 1 changes
  (run 'hg update' to get a working copy)
  $ $PYTHON $TESTTMP/filter.py < b/mbox
  From test@test.com ... ... .. ..:..:.. .... (re)
  MIME-Version: 1.0
  Content-Type: text/plain; charset="*" (glob)
  Content-Transfer-Encoding: quoted-printable
  X-Test: foo
  Date: * (glob)
  Subject: long line
  From: test@test.com
  X-Hg-Notification: changeset a323cae54f6e
  Message-Id: <hg.a323cae54f6e.*.*@*> (glob)
  To: baz@test.com, foo@bar
  
  changeset a323cae54f6e in b
  description: long line
  diffstat:
   a |  1 + 1 files changed, 1 insertions(+), 0 deletions(-)
  
  diffs (8 lines):
  
  diff -r 7ea05ad269dc -r a323cae54f6e a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,3 +1,4 @@ a a a
  +nonononononononononononononononononononononononononononononononononononono=
  nononononononononononononononononononononononononononononononononononononon=
  ononononononononononononononononononononononononononononononononononononono=
  nononononononononononononononononononononononononononononononononononononon=
  ononononononononononononononononononononononononononononononononononononono=
  nononononononononononononononononononononononononononononononononononononon=
  ononononononononononononononononononononononononononononononononononononono=
  nononononononononononononononononononononononononononononononononononononon=
  ononononononononononononononononononononononononononononononononononononono=
  nononononononononononononononononononononononononononononononononononononon=
  ononononononononononononononononononononononononononononononononononononono=
  nononononononononononononononononononononononononononononononononononononon=
  ononononononononononononononononononononononononononononononononononononono=
  nonononononononononononono=D1=84
  
 revset selection: send to address that matches branch and repo

  $ cat << EOF >> $HGRCPATH
  > [hooks]
  > incoming.notify = python:hgext.notify.hook
  > 
  > [notify]
  > sources = pull
  > test = True
  > diffstat = False
  > maxdiff = 0
  > 
  > [reposubs]
  > */a#branch(test) = will_no_be_send@example.com
  > */b#branch(test) = notify@example.com
  > EOF
  $ hg --cwd a branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ echo a >> a/a
  $ hg --cwd a ci -m test -d '1 0'
  $ hg --traceback --cwd b pull ../a | $PYTHON $TESTTMP/filter.py
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets b7cf10b2bdec
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  X-Test: foo
  Date: * (glob)
  Subject: test
  From: test@test.com
  X-Hg-Notification: changeset b7cf10b2bdec
  Message-Id: <hg.b7cf10b2bdec.*.*@*> (glob)
  To: baz@test.com, foo@bar, notify@example.com
  
  changeset b7cf10b2bdec in b
  description: test
  (run 'hg update' to get a working copy)

revset selection: don't send to address that waits for mails
from different branch

  $ hg --cwd a update default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a >> a/a
  $ hg --cwd a ci -m test -d '1 0'
  $ hg --traceback --cwd b pull ../a | $PYTHON $TESTTMP/filter.py
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets 5a07df312a79
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  X-Test: foo
  Date: * (glob)
  Subject: test
  From: test@test.com
  X-Hg-Notification: changeset 5a07df312a79
  Message-Id: <hg.5a07df312a79.*.*@*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 5a07df312a79 in b
  description: test
  (run 'hg heads' to see heads)

default template:

  $ grep -v '^template =' $HGRCPATH > "$HGRCPATH.new"
  $ mv "$HGRCPATH.new" $HGRCPATH
  $ echo a >> a/a
  $ hg --cwd a commit -m 'default template'
  $ hg --cwd b pull ../a -q | $PYTHON $TESTTMP/filter.py
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  Date: * (glob)
  Subject: changeset in b: default template
  From: test@test.com
  X-Hg-Notification: changeset f5e8ec95bf59
  Message-Id: <hg.f5e8ec95bf59.*.*@*> (glob)
  To: baz@test.com, foo@bar
  
  changeset f5e8ec95bf59 in $TESTTMP/b (glob)
  details: http://test/b?cmd=changeset;node=f5e8ec95bf59
  description: default template

with style:

  $ cat <<EOF > notifystyle.map
  > changeset = "Subject: {desc|firstline|strip}
  >              From: {author}
  >              {""}
  >              changeset {node|short}"
  > EOF
  $ cat <<EOF >> $HGRCPATH
  > [notify]
  > style = $TESTTMP/notifystyle.map
  > EOF
  $ echo a >> a/a
  $ hg --cwd a commit -m 'with style'
  $ hg --cwd b pull ../a -q | $PYTHON $TESTTMP/filter.py
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  Date: * (glob)
  Subject: with style
  From: test@test.com
  X-Hg-Notification: changeset 9e2c3a8e9c43
  Message-Id: <hg.9e2c3a8e9c43.*.*@*> (glob)
  To: baz@test.com, foo@bar
  
  changeset 9e2c3a8e9c43

with template (overrides style):

  $ cat <<EOF >> $HGRCPATH
  > template = Subject: {node|short}: {desc|firstline|strip}
  >            From: {author}
  >            {""}
  >            {desc}
  > EOF
  $ echo a >> a/a
  $ hg --cwd a commit -m 'with template'
  $ hg --cwd b pull ../a -q | $PYTHON $TESTTMP/filter.py
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  Date: * (glob)
  Subject: e2cbf5bf18a7: with template
  From: test@test.com
  X-Hg-Notification: changeset e2cbf5bf18a7
  Message-Id: <hg.e2cbf5bf18a7.*.*@*> (glob)
  To: baz@test.com, foo@bar
  
  with template

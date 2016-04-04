mock bugzilla driver for testing template output:

  $ cat <<EOF > bzmock.py
  > from __future__ import absolute_import
  > from mercurial import extensions
  > 
  > def extsetup(ui):
  >     bugzilla = extensions.find('bugzilla')
  >     class bzmock(bugzilla.bzaccess):
  >         def __init__(self, ui):
  >             super(bzmock, self).__init__(ui)
  >             self._logfile = ui.config('bugzilla', 'mocklog')
  >         def updatebug(self, bugid, newstate, text, committer):
  >             with open(self._logfile, 'a') as f:
  >                 f.write('update bugid=%r, newstate=%r, committer=%r\n'
  >                         % (bugid, newstate, committer))
  >                 f.write('----\n' + text + '\n----\n')
  >         def notify(self, bugs, committer):
  >             with open(self._logfile, 'a') as f:
  >                 f.write('notify bugs=%r, committer=%r\n'
  >                         % (bugs, committer))
  >     bugzilla.bugzilla._versions['mock'] = bzmock
  > EOF

set up mock repository:

  $ hg init mockremote
  $ cat <<EOF > mockremote/.hg/hgrc
  > [extensions]
  > bugzilla =
  > bzmock = $TESTTMP/bzmock.py
  > 
  > [bugzilla]
  > version = mock
  > mocklog = $TESTTMP/bzmock.log
  > 
  > [hooks]
  > incoming.bugzilla = python:hgext.bugzilla.hook
  > 
  > [web]
  > baseurl=http://example.org/hg
  > 
  > %include $TESTTMP/bzstyle.hgrc
  > EOF

  $ hg clone -q mockremote mocklocal

push with default template:

  $ echo '[bugzilla]' > bzstyle.hgrc
  $ echo foo > mocklocal/foo
  $ hg ci -R mocklocal -Aqm 'Fixes bug 123'
  $ hg -R mocklocal push -q
  $ cat bzmock.log && rm bzmock.log
  update bugid=123, newstate={}, committer='test'
  ----
  changeset 7875a8342c6f in repo $TESTTMP/mockremote refers to bug 123.
  details:
  	Fixes bug 123
  ----
  notify bugs={123: {}}, committer='test'

push with style:

  $ cat <<EOF > bzstyle.map
  > changeset = "{node|short} refers to bug {bug}."
  > EOF
  $ echo "style = $TESTTMP/bzstyle.map" >> bzstyle.hgrc
  $ echo foo >> mocklocal/foo
  $ hg ci -R mocklocal -qm 'Fixes bug 456'
  $ hg -R mocklocal push -q
  $ cat bzmock.log && rm bzmock.log
  update bugid=456, newstate={}, committer='test'
  ----
  2808b172464b refers to bug 456.
  ----
  notify bugs={456: {}}, committer='test'

push with template (overrides style):

  $ cat <<EOF >> bzstyle.hgrc
  > template = Changeset {node|short} in {root|basename}.
  >            {hgweb}/rev/{node|short}\n
  >            {desc}
  > EOF
  $ echo foo >> mocklocal/foo
  $ hg ci -R mocklocal -qm 'Fixes bug 789'
  $ hg -R mocklocal push -q
  $ cat bzmock.log && rm bzmock.log
  update bugid=789, newstate={}, committer='test'
  ----
  Changeset a770f3e409f2 in mockremote.
  http://example.org/hg/rev/a770f3e409f2
  
  Fixes bug 789
  ----
  notify bugs={789: {}}, committer='test'

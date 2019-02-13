  $ newrepo a
  $ drawdag <<'EOS'
  > A
  > |
  > B
  > EOS

Create a local tag:

  $ hg tag -l -r $A tag1
  $ hg tags
  tip                                1:25c348c2bb87
  tag1                               1:25c348c2bb87

  $ hg update -r tag1 -q

Tag does not move with commit:

  $ hg ci -m C --config ui.allowemptycommit=1
  $ hg log -r tag1 -T '{desc}\n'
  A

When tag and bookmark conflict, resolve bookmark first:

  $ hg bookmark -ir $B tag1
  bookmark tag1 matches a changeset hash
  (did you leave a -r out of an 'hg bookmark' command?)
  $ hg bookmarks
     tag1                      0:fc2b737bb2e5
  $ hg log -r tag1 -T '{desc}\n'
  B

  $ hg bookmark -d tag1
  $ hg log -r tag1 -T '{desc}\n'
  A

Templates:

  $ hg log -r $A -T '{tags}\n'
  tag1

Delete a tag:

  $ hg tag -l --remove tag1
  $ hg tags
  tip                                2:6a5655092097

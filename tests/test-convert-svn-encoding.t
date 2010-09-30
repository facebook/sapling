
  $ "$TESTDIR/hghave" svn svn-bindings || exit 80

  $ cat > $HGRCPATH <<EOF
  > [extensions]
  > convert = 
  > graphlog =
  > EOF

  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/encoding.svndump"

Convert while testing all possible outputs

  $ hg --debug convert svn-repo A-hg
  initializing destination A-hg repository
  reparent to file://*/svn-repo (glob)
  run hg sink pre-conversion action
  scanning source...
  found trunk at 'trunk'
  found tags at 'tags'
  found branches at 'branches'
  found branch branché at 5
  found branch branchée at 6
  scanning: 1 revisions
  reparent to file://*/svn-repo/trunk (glob)
  fetching revision log for "/trunk" from 4 to 0
  parsing revision 4 (2 changes)
  parsing revision 3 (4 changes)
  parsing revision 2 (3 changes)
  parsing revision 1 (3 changes)
  no copyfrom path, don't know what to do.
  '/branches' is not under '/trunk', ignoring
  '/tags' is not under '/trunk', ignoring
  scanning: 2 revisions
  reparent to file://*/svn-repo/branches/branch%C3%A9 (glob)
  fetching revision log for "/branches/branché" from 5 to 0
  parsing revision 5 (1 changes)
  reparent to file://*/svn-repo (glob)
  reparent to file://*/svn-repo/branches/branch%C3%A9 (glob)
  found parent of branch /branches/branché at 4: /trunk
  scanning: 3 revisions
  reparent to file://*/svn-repo/branches/branch%C3%A9e (glob)
  fetching revision log for "/branches/branchée" from 6 to 0
  parsing revision 6 (1 changes)
  reparent to file://*/svn-repo (glob)
  reparent to file://*/svn-repo/branches/branch%C3%A9e (glob)
  found parent of branch /branches/branchée at 5: /branches/branché
  scanning: 4 revisions
  scanning: 5 revisions
  scanning: 6 revisions
  sorting...
  converting...
  5 init projA
  source: svn:afeb9c47-92ff-4c0c-9f72-e1f6eb8ac9af/trunk@1
  converting: 0/6 revisions (0.00%)
  4 hello
  source: svn:afeb9c47-92ff-4c0c-9f72-e1f6eb8ac9af/trunk@2
  converting: 1/6 revisions (16.67%)
  reparent to file://*/svn-repo/trunk (glob)
  scanning paths: /trunk/à 0/3 (0.00%)
  scanning paths: /trunk/à/é 1/3 (33.33%)
  scanning paths: /trunk/é 2/3 (66.67%)
  à/é
  getting files: à/é 1/2 (50.00%)
  é
  getting files: é 2/2 (100.00%)
  3 copy files
  source: svn:afeb9c47-92ff-4c0c-9f72-e1f6eb8ac9af/trunk@3
  converting: 2/6 revisions (33.33%)
  scanning paths: /trunk/à 0/4 (0.00%)
  gone from -1
  reparent to file://*/svn-repo (glob)
  reparent to file://*/svn-repo/trunk (glob)
  scanning paths: /trunk/è 1/4 (25.00%)
  copied to è from é@2
  scanning paths: /trunk/é 2/4 (50.00%)
  gone from -1
  reparent to file://*/svn-repo (glob)
  reparent to file://*/svn-repo/trunk (glob)
  scanning paths: /trunk/ù 3/4 (75.00%)
  mark /trunk/ù came from à:2
  à/é
  getting files: à/é 1/4 (25.00%)
  è
  getting files: è 2/4 (50.00%)
   è: copy é:6b67ccefd5ce6de77e7ead4f5292843a0255329f
  é
  getting files: é 3/4 (75.00%)
  ù/é
  getting files: ù/é 4/4 (100.00%)
   ù/é: copy à/é:a9092a3d84a37b9993b5c73576f6de29b7ea50f6
  2 remove files
  source: svn:afeb9c47-92ff-4c0c-9f72-e1f6eb8ac9af/trunk@4
  converting: 3/6 revisions (50.00%)
  scanning paths: /trunk/è 0/2 (0.00%)
  gone from -1
  reparent to file://*/svn-repo (glob)
  reparent to file://*/svn-repo/trunk (glob)
  scanning paths: /trunk/ù 1/2 (50.00%)
  gone from -1
  reparent to file://*/svn-repo (glob)
  reparent to file://*/svn-repo/trunk (glob)
  è
  getting files: è 1/2 (50.00%)
  ù/é
  getting files: ù/é 2/2 (100.00%)
  1 branch to branch?
  source: svn:afeb9c47-92ff-4c0c-9f72-e1f6eb8ac9af/branches/branch?@5
  converting: 4/6 revisions (66.67%)
  reparent to file://*/svn-repo/branches/branch%C3%A9 (glob)
  scanning paths: /branches/branché 0/1 (0.00%)
  0 branch to branch?e
  source: svn:afeb9c47-92ff-4c0c-9f72-e1f6eb8ac9af/branches/branch?e@6
  converting: 5/6 revisions (83.33%)
  reparent to file://*svn-repo/branches/branch%C3%A9e (glob)
  scanning paths: /branches/branchée 0/1 (0.00%)
  reparent to file://*svn-repo (glob)
  reparent to file://*svn-repo/branches/branch%C3%A9e (glob)
  reparent to file://*svn-repo (glob)
  reparent to file://*svn-repo/branches/branch%C3%A9e (glob)
  updating tags
  .hgtags
  run hg sink post-conversion action
  $ cd A-hg
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Check tags are in UTF-8

  $ python -c "print '\n'.join([('%r' % l) for l in file('.hgtags', 'rb').readlines()])"
  '221c3fdaf24df5f14c0a64c597581e2eacfb47bb branch\xc3\xa9e\n'
  '7a40952c2db29cf00d9e31df3749e98d8a4bdcbf branch\xc3\xa9\n'

  $ cd ..

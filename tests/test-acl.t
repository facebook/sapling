  > do_push()
  > {
  >     user=$1
  >     shift
  >     echo "Pushing as user $user"
  >     echo 'hgrc = """'
  >     sed -e 1,2d b/.hg/hgrc | grep -v fakegroups.py
  >     echo '"""'
  >     if test -f acl.config; then
  >         echo 'acl.config = """'
  >         cat acl.config
  >         echo '"""'
  >     fi
  >     # On AIX /etc/profile sets LOGNAME read-only. So
  >     #  LOGNAME=$user hg --cws a --debug push ../b
  >     # fails with "This variable is read only."
  >     # Use env to work around this.
  >     env LOGNAME=$user hg --cwd a --debug push ../b
  >     hg --cwd b rollback
  >     hg --cwd b --quiet tip
  >     echo
  > }

  > init_config()
  > {
  >     cat > fakegroups.py <<EOF
  > from hgext import acl
  > def fakegetusers(ui, group):
  >     try:
  >         return acl._getusersorig(ui, group)
  >     except:
  >         return ["fred", "betty"]
  > acl._getusersorig = acl._getusers
  > acl._getusers = fakegetusers
  > EOF
  >     rm -f acl.config
  >     cat > $config <<EOF
  > [hooks]
  > pretxnchangegroup.acl = python:hgext.acl.hook
  > [acl]
  > sources = push
  > [extensions]
  > f=`pwd`/fakegroups.py
  > EOF
  > }

  $ hg init a
  $ cd a
  $ mkdir foo foo/Bar quux
  $ echo 'in foo' > foo/file.txt
  $ echo 'in foo/Bar' > foo/Bar/file.txt
  $ echo 'in quux' > quux/file.py
  $ hg add -q
  $ hg ci -m 'add files' -d '1000000 0'
  $ echo >> foo/file.txt
  $ hg ci -m 'change foo/file' -d '1000001 0'
  $ echo >> foo/Bar/file.txt
  $ hg ci -m 'change foo/Bar/file' -d '1000002 0'
  $ echo >> quux/file.py
  $ hg ci -m 'change quux/file' -d '1000003 0'
  $ hg tip --quiet
  3:911600dab2ae

  $ cd ..
  $ hg clone -r 0 a b
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'acl =' >> $HGRCPATH

  $ config=b/.hg/hgrc

Extension disabled for lack of a hook

  $ do_push fred
  Pushing as user fred
  hgrc = """
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  updating the branch cache
  checking for updated bookmarks
  repository tip rolled back to revision 0 (undo push)
  working directory now based on revision 0
  0:6675d58eff77
  

  $ echo '[hooks]' >> $config
  $ echo 'pretxnchangegroup.acl = python:hgext.acl.hook' >> $config

Extension disabled for lack of acl.sources

  $ do_push fred
  Pushing as user fred
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  invalidating branch cache (tip differs)
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: changes have source "push" - skipping
  updating the branch cache
  checking for updated bookmarks
  repository tip rolled back to revision 0 (undo push)
  working directory now based on revision 0
  0:6675d58eff77
  

No [acl.allow]/[acl.deny]

  $ echo '[acl]' >> $config
  $ echo 'sources = push' >> $config
  $ do_push fred
  Pushing as user fred
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  invalidating branch cache (tip differs)
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: allowing changeset 911600dab2ae
  updating the branch cache
  checking for updated bookmarks
  repository tip rolled back to revision 0 (undo push)
  working directory now based on revision 0
  0:6675d58eff77
  

Empty [acl.allow]

  $ echo '[acl.allow]' >> $config
  $ do_push fred
  Pushing as user fred
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  invalidating branch cache (tip differs)
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 0 entries for user fred
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: user fred not allowed on foo/file.txt
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset ef1ea85a6374
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset ef1ea85a6374
  no rollback information available
  0:6675d58eff77
  

fred is allowed inside foo/

  $ echo 'foo/** = fred' >> $config
  $ do_push fred
  Pushing as user fred
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: user fred not allowed on quux/file.py
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset 911600dab2ae
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset 911600dab2ae
  no rollback information available
  0:6675d58eff77
  

Empty [acl.deny]

  $ echo '[acl.deny]' >> $config
  $ do_push barney
  Pushing as user barney
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 0 entries for user barney
  acl: acl.deny enabled, 0 entries for user barney
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: user barney not allowed on foo/file.txt
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset ef1ea85a6374
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset ef1ea85a6374
  no rollback information available
  0:6675d58eff77
  

fred is allowed inside foo/, but not foo/bar/ (case matters)

  $ echo 'foo/bar/** = fred' >> $config
  $ do_push fred
  Pushing as user fred
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  foo/bar/** = fred
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny enabled, 1 entries for user fred
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: user fred not allowed on quux/file.py
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset 911600dab2ae
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset 911600dab2ae
  no rollback information available
  0:6675d58eff77
  

fred is allowed inside foo/, but not foo/Bar/

  $ echo 'foo/Bar/** = fred' >> $config
  $ do_push fred
  Pushing as user fred
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  foo/bar/** = fred
  foo/Bar/** = fred
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny enabled, 2 entries for user fred
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: user fred denied on foo/Bar/file.txt
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset f9cafe1212c8
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset f9cafe1212c8
  no rollback information available
  0:6675d58eff77
  

  $ echo 'barney is not mentioned => not allowed anywhere'
  barney is not mentioned => not allowed anywhere
  $ do_push barney
  Pushing as user barney
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  foo/bar/** = fred
  foo/Bar/** = fred
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 0 entries for user barney
  acl: acl.deny enabled, 0 entries for user barney
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: user barney not allowed on foo/file.txt
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset ef1ea85a6374
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset ef1ea85a6374
  no rollback information available
  0:6675d58eff77
  

barney is allowed everywhere

  $ echo '[acl.allow]' >> $config
  $ echo '** = barney' >> $config
  $ do_push barney
  Pushing as user barney
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  foo/bar/** = fred
  foo/Bar/** = fred
  [acl.allow]
  ** = barney
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user barney
  acl: acl.deny enabled, 0 entries for user barney
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: allowing changeset 911600dab2ae
  updating the branch cache
  checking for updated bookmarks
  repository tip rolled back to revision 0 (undo push)
  working directory now based on revision 0
  0:6675d58eff77
  

wilma can change files with a .txt extension

  $ echo '**/*.txt = wilma' >> $config
  $ do_push wilma
  Pushing as user wilma
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  foo/bar/** = fred
  foo/Bar/** = fred
  [acl.allow]
  ** = barney
  **/*.txt = wilma
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  invalidating branch cache (tip differs)
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user wilma
  acl: acl.deny enabled, 0 entries for user wilma
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: user wilma not allowed on quux/file.py
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset 911600dab2ae
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset 911600dab2ae
  no rollback information available
  0:6675d58eff77
  

file specified by acl.config does not exist

  $ echo '[acl]' >> $config
  $ echo 'config = ../acl.config' >> $config
  $ do_push barney
  Pushing as user barney
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  foo/bar/** = fred
  foo/Bar/** = fred
  [acl.allow]
  ** = barney
  **/*.txt = wilma
  [acl]
  config = ../acl.config
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  error: pretxnchangegroup.acl hook raised an exception: [Errno 2] No such file or directory: '../acl.config'
  transaction abort!
  rollback completed
  abort: No such file or directory: ../acl.config
  no rollback information available
  0:6675d58eff77
  

betty is allowed inside foo/ by a acl.config file

  $ echo '[acl.allow]' >> acl.config
  $ echo 'foo/** = betty' >> acl.config
  $ do_push betty
  Pushing as user betty
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  foo/bar/** = fred
  foo/Bar/** = fred
  [acl.allow]
  ** = barney
  **/*.txt = wilma
  [acl]
  config = ../acl.config
  """
  acl.config = """
  [acl.allow]
  foo/** = betty
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user betty
  acl: acl.deny enabled, 0 entries for user betty
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: user betty not allowed on quux/file.py
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset 911600dab2ae
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset 911600dab2ae
  no rollback information available
  0:6675d58eff77
  

acl.config can set only [acl.allow]/[acl.deny]

  $ echo '[hooks]' >> acl.config
  $ echo 'changegroup.acl = false' >> acl.config
  $ do_push barney
  Pushing as user barney
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [acl.allow]
  foo/** = fred
  [acl.deny]
  foo/bar/** = fred
  foo/Bar/** = fred
  [acl.allow]
  ** = barney
  **/*.txt = wilma
  [acl]
  config = ../acl.config
  """
  acl.config = """
  [acl.allow]
  foo/** = betty
  [hooks]
  changegroup.acl = false
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user barney
  acl: acl.deny enabled, 0 entries for user barney
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: allowing changeset 911600dab2ae
  updating the branch cache
  checking for updated bookmarks
  repository tip rolled back to revision 0 (undo push)
  working directory now based on revision 0
  0:6675d58eff77
  

asterisk

  $ init_config

asterisk test

  $ echo '[acl.allow]' >> $config
  $ echo "** = fred" >> $config

fred is always allowed

  $ do_push fred
  Pushing as user fred
  hgrc = """
  [acl]
  sources = push
  [extensions]
  [acl.allow]
  ** = fred
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  invalidating branch cache (tip differs)
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: allowing changeset 911600dab2ae
  updating the branch cache
  checking for updated bookmarks
  repository tip rolled back to revision 0 (undo push)
  working directory now based on revision 0
  0:6675d58eff77
  

  $ echo '[acl.deny]' >> $config
  $ echo "foo/Bar/** = *" >> $config

no one is allowed inside foo/Bar/

  $ do_push fred
  Pushing as user fred
  hgrc = """
  [acl]
  sources = push
  [extensions]
  [acl.allow]
  ** = fred
  [acl.deny]
  foo/Bar/** = *
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  invalidating branch cache (tip differs)
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny enabled, 1 entries for user fred
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: user fred denied on foo/Bar/file.txt
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset f9cafe1212c8
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset f9cafe1212c8
  no rollback information available
  0:6675d58eff77
  

Groups

  $ init_config

OS-level groups

  $ echo '[acl.allow]' >> $config
  $ echo "** = @group1" >> $config

@group1 is always allowed

  $ do_push fred
  Pushing as user fred
  hgrc = """
  [acl]
  sources = push
  [extensions]
  [acl.allow]
  ** = @group1
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: "group1" not defined in [acl.groups]
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: allowing changeset f9cafe1212c8
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: allowing changeset 911600dab2ae
  updating the branch cache
  checking for updated bookmarks
  repository tip rolled back to revision 0 (undo push)
  working directory now based on revision 0
  0:6675d58eff77
  

  $ echo '[acl.deny]' >> $config
  $ echo "foo/Bar/** = @group1" >> $config

@group is allowed inside anything but foo/Bar/

  $ do_push fred
  Pushing as user fred
  hgrc = """
  [acl]
  sources = push
  [extensions]
  [acl.allow]
  ** = @group1
  [acl.deny]
  foo/Bar/** = @group1
  """
  pushing to ../b
  searching for changes
  common changesets up to 6675d58eff77
  invalidating branch cache (tip differs)
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  adding changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 0 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 1 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 2 changesets
  bundling: 3 changesets
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 0/3 manifests (0.00%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 1/3 manifests (33.33%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 2/3 manifests (66.67%)
  bundling: 3/3 manifests (100.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/Bar/file.txt 0/3 files (0.00%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: foo/file.txt 1/3 files (33.33%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  bundling: quux/file.py 2/3 files (66.67%)
  changesets: 1 chunks
  add changeset ef1ea85a6374
  changesets: 2 chunks
  add changeset f9cafe1212c8
  changesets: 3 chunks
  add changeset 911600dab2ae
  adding manifests
  manifests: 1/3 chunks (33.33%)
  manifests: 2/3 chunks (66.67%)
  manifests: 3/3 chunks (100.00%)
  adding file changes
  adding foo/Bar/file.txt revisions
  files: 1/3 chunks (33.33%)
  adding foo/file.txt revisions
  files: 2/3 chunks (66.67%)
  adding quux/file.py revisions
  files: 3/3 chunks (100.00%)
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: "group1" not defined in [acl.groups]
  acl: acl.allow enabled, 1 entries for user fred
  acl: "group1" not defined in [acl.groups]
  acl: acl.deny enabled, 1 entries for user fred
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: allowing changeset ef1ea85a6374
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: user fred denied on foo/Bar/file.txt
  error: pretxnchangegroup.acl hook failed: acl: access denied for changeset f9cafe1212c8
  transaction abort!
  rollback completed
  abort: acl: access denied for changeset f9cafe1212c8
  no rollback information available
  0:6675d58eff77
  

Invalid group

Disable the fakegroups trick to get real failures

  $ grep -v fakegroups $config > config.tmp
  $ mv config.tmp $config
  $ echo '[acl.allow]' >> $config
  $ echo "** = @unlikelytoexist" >> $config
  $ do_push fred 2>&1 | grep unlikelytoexist
  ** = @unlikelytoexist
  acl: "unlikelytoexist" not defined in [acl.groups]
  error: pretxnchangegroup.acl hook failed: group 'unlikelytoexist' is undefined
  abort: group 'unlikelytoexist' is undefined

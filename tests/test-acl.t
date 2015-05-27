  > do_push()
  > {
  >     user=$1
  >     shift
  >     echo "Pushing as user $user"
  >     echo 'hgrc = """'
  >     sed -n '/\[[ha]/,$p' b/.hg/hgrc | grep -v fakegroups.py
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

  $ cat << EOF >> $HGRCPATH
  > [experimental]
  > # drop me once bundle2 is the default,
  > # added to get test change early.
  > bundle2-exp = True
  > EOF

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

  $ config=b/.hg/hgrc

Extension disabled for lack of a hook

  $ do_push fred
  Pushing as user fred
  hgrc = """
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  bundle2-input-part: total payload size 1606
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-bundle: 3 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 2 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 1 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 0 (undo push)
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: changes have source "push" - skipping
  bundle2-input-part: total payload size 1606
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-bundle: 3 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 2 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 1 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 0 (undo push)
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  bundle2-input-part: total payload size 1606
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-bundle: 3 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 2 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 1 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 0 (undo push)
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 0 entries for user fred
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "fred" not allowed on "foo/file.txt" (changeset "ef1ea85a6374")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "fred" not allowed on "foo/file.txt" (changeset "ef1ea85a6374")
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "fred" not allowed on "quux/file.py" (changeset "911600dab2ae")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "fred" not allowed on "quux/file.py" (changeset "911600dab2ae")
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "barney"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 0 entries for user barney
  acl: acl.deny enabled, 0 entries for user barney
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "barney" not allowed on "foo/file.txt" (changeset "ef1ea85a6374")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "barney" not allowed on "foo/file.txt" (changeset "ef1ea85a6374")
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny enabled, 1 entries for user fred
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "fred" not allowed on "quux/file.py" (changeset "911600dab2ae")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "fred" not allowed on "quux/file.py" (changeset "911600dab2ae")
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny enabled, 2 entries for user fred
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "fred" denied on "foo/Bar/file.txt" (changeset "f9cafe1212c8")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "fred" denied on "foo/Bar/file.txt" (changeset "f9cafe1212c8")
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "barney"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 0 entries for user barney
  acl: acl.deny enabled, 0 entries for user barney
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "barney" not allowed on "foo/file.txt" (changeset "ef1ea85a6374")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "barney" not allowed on "foo/file.txt" (changeset "ef1ea85a6374")
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "barney"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user barney
  acl: acl.deny enabled, 0 entries for user barney
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  bundle2-input-part: total payload size 1606
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-bundle: 3 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 2 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 1 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 0 (undo push)
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "wilma"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user wilma
  acl: acl.deny enabled, 0 entries for user wilma
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "wilma" not allowed on "quux/file.py" (changeset "911600dab2ae")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "wilma" not allowed on "quux/file.py" (changeset "911600dab2ae")
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "barney"
  error: pretxnchangegroup.acl hook raised an exception: [Errno 2] No such file or directory: '../acl.config'
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "betty"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user betty
  acl: acl.deny enabled, 0 entries for user betty
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "betty" not allowed on "quux/file.py" (changeset "911600dab2ae")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "betty" not allowed on "quux/file.py" (changeset "911600dab2ae")
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
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "barney"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user barney
  acl: acl.deny enabled, 0 entries for user barney
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  bundle2-input-part: total payload size 1606
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-bundle: 3 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 2 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 1 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 0 (undo push)
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
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.allow]
  ** = fred
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  bundle2-input-part: total payload size 1606
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-bundle: 3 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 2 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 1 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 0 (undo push)
  0:6675d58eff77
  

  $ echo '[acl.deny]' >> $config
  $ echo "foo/Bar/** = *" >> $config

no one is allowed inside foo/Bar/

  $ do_push fred
  Pushing as user fred
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.allow]
  ** = fred
  [acl.deny]
  foo/Bar/** = *
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny enabled, 1 entries for user fred
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "fred" denied on "foo/Bar/file.txt" (changeset "f9cafe1212c8")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "fred" denied on "foo/Bar/file.txt" (changeset "f9cafe1212c8")
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
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.allow]
  ** = @group1
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: "group1" not defined in [acl.groups]
  acl: acl.allow enabled, 1 entries for user fred
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  bundle2-input-part: total payload size 1606
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-bundle: 3 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 2 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 1 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 0 (undo push)
  0:6675d58eff77
  

  $ echo '[acl.deny]' >> $config
  $ echo "foo/Bar/** = @group1" >> $config

@group is allowed inside anything but foo/Bar/

  $ do_push fred
  Pushing as user fred
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.allow]
  ** = @group1
  [acl.deny]
  foo/Bar/** = @group1
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  invalid branchheads cache (served): tip differs
  listing keys for "bookmarks"
  3 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  adding manifests
  adding file changes
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 3 changesets with 3 changes to 3 files
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "fred"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: "group1" not defined in [acl.groups]
  acl: acl.allow enabled, 1 entries for user fred
  acl: "group1" not defined in [acl.groups]
  acl: acl.deny enabled, 1 entries for user fred
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  error: pretxnchangegroup.acl hook failed: acl: user "fred" denied on "foo/Bar/file.txt" (changeset "f9cafe1212c8")
  bundle2-input-part: total payload size 1606
  bundle2-input-bundle: 3 parts total
  transaction abort!
  rollback completed
  abort: acl: user "fred" denied on "foo/Bar/file.txt" (changeset "f9cafe1212c8")
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


Branch acl tests setup

  $ init_config
  $ cd b
  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch foobar
  marked working directory as branch foobar
  (branches are permanent and global, did you want a bookmark?)
  $ hg commit -m 'create foobar'
  $ echo 'foo contents' > abc.txt
  $ hg add abc.txt
  $ hg commit -m 'foobar contents'
  $ cd ..
  $ hg --cwd a pull ../b
  pulling from ../b
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads)

Create additional changeset on foobar branch

  $ cd a
  $ hg up -C foobar
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'foo contents2' > abc.txt
  $ hg commit -m 'foobar contents2'
  $ cd ..


No branch acls specified

  $ do_push astro
  Pushing as user astro
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "astro"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches not enabled
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  acl: branch access granted: "e8fc755d4d82" on branch "foobar"
  acl: path access granted: "e8fc755d4d82"
  bundle2-input-part: total payload size 2101
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:e8fc755d4d8217ee5b0c2bb41558c40d43b92c01"
  bundle2-input-bundle: 4 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 3 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 2 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 2 (undo push)
  2:fb35475503ef
  

Branch acl deny test

  $ echo "[acl.deny.branches]" >> $config
  $ echo "foobar = *" >> $config
  $ do_push astro
  Pushing as user astro
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.deny.branches]
  foobar = *
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "astro"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches enabled, 1 entries for user astro
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  error: pretxnchangegroup.acl hook failed: acl: user "astro" denied on branch "foobar" (changeset "e8fc755d4d82")
  bundle2-input-part: total payload size 2101
  bundle2-input-bundle: 4 parts total
  transaction abort!
  rollback completed
  abort: acl: user "astro" denied on branch "foobar" (changeset "e8fc755d4d82")
  no rollback information available
  2:fb35475503ef
  

Branch acl empty allow test

  $ init_config
  $ echo "[acl.allow.branches]" >> $config
  $ do_push astro
  Pushing as user astro
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.allow.branches]
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "astro"
  acl: acl.allow.branches enabled, 0 entries for user astro
  acl: acl.deny.branches not enabled
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  error: pretxnchangegroup.acl hook failed: acl: user "astro" not allowed on branch "default" (changeset "ef1ea85a6374")
  bundle2-input-part: total payload size 2101
  bundle2-input-bundle: 4 parts total
  transaction abort!
  rollback completed
  abort: acl: user "astro" not allowed on branch "default" (changeset "ef1ea85a6374")
  no rollback information available
  2:fb35475503ef
  

Branch acl allow other

  $ init_config
  $ echo "[acl.allow.branches]" >> $config
  $ echo "* = george" >> $config
  $ do_push astro
  Pushing as user astro
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.allow.branches]
  * = george
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "astro"
  acl: acl.allow.branches enabled, 0 entries for user astro
  acl: acl.deny.branches not enabled
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  error: pretxnchangegroup.acl hook failed: acl: user "astro" not allowed on branch "default" (changeset "ef1ea85a6374")
  bundle2-input-part: total payload size 2101
  bundle2-input-bundle: 4 parts total
  transaction abort!
  rollback completed
  abort: acl: user "astro" not allowed on branch "default" (changeset "ef1ea85a6374")
  no rollback information available
  2:fb35475503ef
  
  $ do_push george
  Pushing as user george
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.allow.branches]
  * = george
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "george"
  acl: acl.allow.branches enabled, 1 entries for user george
  acl: acl.deny.branches not enabled
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  acl: branch access granted: "e8fc755d4d82" on branch "foobar"
  acl: path access granted: "e8fc755d4d82"
  bundle2-input-part: total payload size 2101
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:e8fc755d4d8217ee5b0c2bb41558c40d43b92c01"
  bundle2-input-bundle: 4 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 3 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 2 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 2 (undo push)
  2:fb35475503ef
  

Branch acl conflicting allow
asterisk ends up applying to all branches and allowing george to
push foobar into the remote

  $ init_config
  $ echo "[acl.allow.branches]" >> $config
  $ echo "foobar = astro" >> $config
  $ echo "* = george" >> $config
  $ do_push george
  Pushing as user george
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.allow.branches]
  foobar = astro
  * = george
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "george"
  acl: acl.allow.branches enabled, 1 entries for user george
  acl: acl.deny.branches not enabled
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  acl: branch access granted: "e8fc755d4d82" on branch "foobar"
  acl: path access granted: "e8fc755d4d82"
  bundle2-input-part: total payload size 2101
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:e8fc755d4d8217ee5b0c2bb41558c40d43b92c01"
  bundle2-input-bundle: 4 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 3 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 2 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 2 (undo push)
  2:fb35475503ef
  
Branch acl conflicting deny

  $ init_config
  $ echo "[acl.deny.branches]" >> $config
  $ echo "foobar = astro" >> $config
  $ echo "default = astro" >> $config
  $ echo "* = george" >> $config
  $ do_push george
  Pushing as user george
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.deny.branches]
  foobar = astro
  default = astro
  * = george
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "george"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches enabled, 1 entries for user george
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  error: pretxnchangegroup.acl hook failed: acl: user "george" denied on branch "default" (changeset "ef1ea85a6374")
  bundle2-input-part: total payload size 2101
  bundle2-input-bundle: 4 parts total
  transaction abort!
  rollback completed
  abort: acl: user "george" denied on branch "default" (changeset "ef1ea85a6374")
  no rollback information available
  2:fb35475503ef
  
User 'astro' must not be denied

  $ init_config
  $ echo "[acl.deny.branches]" >> $config
  $ echo "default = !astro" >> $config
  $ do_push astro
  Pushing as user astro
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.deny.branches]
  default = !astro
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "astro"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches enabled, 0 entries for user astro
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  acl: branch access granted: "ef1ea85a6374" on branch "default"
  acl: path access granted: "ef1ea85a6374"
  acl: branch access granted: "f9cafe1212c8" on branch "default"
  acl: path access granted: "f9cafe1212c8"
  acl: branch access granted: "911600dab2ae" on branch "default"
  acl: path access granted: "911600dab2ae"
  acl: branch access granted: "e8fc755d4d82" on branch "foobar"
  acl: path access granted: "e8fc755d4d82"
  bundle2-input-part: total payload size 2101
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:911600dab2ae7a9baff75958b84fe606851ce955"
  bundle2-input-part: "pushkey" (params: 4 mandatory) supported
  pushing key for "phases:e8fc755d4d8217ee5b0c2bb41558c40d43b92c01"
  bundle2-input-bundle: 4 parts total
  updating the branch cache
  bundle2-output-bundle: "HG20", 3 parts total
  bundle2-output-part: "reply:changegroup" (advisory) (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-output-part: "reply:pushkey" (params: 0 advisory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "reply:changegroup" (advisory) (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-part: "reply:pushkey" (params: 0 advisory) supported
  bundle2-input-bundle: 2 parts total
  listing keys for "phases"
  try to push obsolete markers to remote
  repository tip rolled back to revision 2 (undo push)
  2:fb35475503ef
  

Non-astro users must be denied

  $ do_push george
  Pushing as user george
  hgrc = """
  [hooks]
  pretxnchangegroup.acl = python:hgext.acl.hook
  [acl]
  sources = push
  [extensions]
  [acl.deny.branches]
  default = !astro
  """
  pushing to ../b
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  4 changesets found
  list of changesets:
  ef1ea85a6374b77d6da9dcda9541f498f2d17df7
  f9cafe1212c8c6fa1120d14a556e18cc44ff8bdd
  911600dab2ae7a9baff75958b84fe606851ce955
  e8fc755d4d8217ee5b0c2bb41558c40d43b92c01
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 93 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "replycaps" supported
  bundle2-input-part: total payload size 93
  bundle2-input-part: "check:heads" supported
  bundle2-input-part: total payload size 20
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset ef1ea85a6374
  add changeset f9cafe1212c8
  add changeset 911600dab2ae
  add changeset e8fc755d4d82
  adding manifests
  adding file changes
  adding abc.txt revisions
  adding foo/Bar/file.txt revisions
  adding foo/file.txt revisions
  adding quux/file.py revisions
  added 4 changesets with 4 changes to 4 files (+1 heads)
  calling hook pretxnchangegroup.acl: hgext.acl.hook
  acl: checking access for user "george"
  acl: acl.allow.branches not enabled
  acl: acl.deny.branches enabled, 1 entries for user george
  acl: acl.allow not enabled
  acl: acl.deny not enabled
  error: pretxnchangegroup.acl hook failed: acl: user "george" denied on branch "default" (changeset "ef1ea85a6374")
  bundle2-input-part: total payload size 2101
  bundle2-input-bundle: 4 parts total
  transaction abort!
  rollback completed
  abort: acl: user "george" denied on branch "default" (changeset "ef1ea85a6374")
  no rollback information available
  2:fb35475503ef
  


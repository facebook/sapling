test sparse

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext/fbsparse.py
  > purge=
  > strip=
  > rebase=
  > EOF

  $ echo a > index.html
  $ echo x > data.py
  $ echo z > readme.txt
  $ cat > webpage.sparse <<EOF
  > [metadata]
  > title: frontend sparse profile
  > [include]
  > *.html
  > EOF
  $ cat > backend.sparse <<EOF
  > [metadata]
  > title: backend sparse profile
  > [include]
  > *.py
  > EOF
  $ hg ci -Aqm 'initial'

  $ hg sparse --include '*.sparse'

Verify enabling a single profile works

  $ hg sparse --enable-profile webpage.sparse
  $ ls
  backend.sparse
  index.html
  webpage.sparse

Verify enabling two profiles works

  $ hg sparse --enable-profile backend.sparse
  $ ls
  backend.sparse
  data.py
  index.html
  webpage.sparse

Verify disabling a profile works

  $ hg sparse --disable-profile webpage.sparse
  $ ls
  backend.sparse
  data.py
  webpage.sparse

Verify error checking includes filename and line numbers

  $ cat > broken.sparse <<EOF
  > # include section omitted
  > [exclude]
  > *.html
  > /absolute/paths/are/ignored
  > [include]
  > EOF
  $ hg add broken.sparse
  $ hg ci -m 'Adding a broken file'
  $ hg sparse --enable-profile broken.sparse
  warning: sparse profile cannot use paths starting with /, ignoring /absolute/paths/are/ignored, in broken.sparse:4
  abort: A sparse file cannot have includes after excludes in broken.sparse:5
  [255]
  $ hg strip .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/myrepo/.hg/strip-backup/* (glob)

Verify that a profile is updated across multiple commits

  $ cat > webpage.sparse <<EOF
  > [metadata]
  > title: frontend sparse profile
  > [include]
  > *.html
  > EOF
  $ cat > backend.sparse <<EOF
  > [metadata]
  > title: backend sparse profile
  > [include]
  > *.py
  > *.txt
  > EOF

  $ echo foo >> data.py

  $ hg ci -m 'edit profile'
  $ ls
  backend.sparse
  data.py
  readme.txt
  webpage.sparse

  $ hg up -q 0
  $ ls
  backend.sparse
  data.py
  webpage.sparse

  $ hg up -q 1
  $ ls
  backend.sparse
  data.py
  readme.txt
  webpage.sparse

Introduce a conflicting .hgsparse change

  $ hg up -q 0
  $ cat > backend.sparse <<EOF
  > [metadata]
  > title: Different backend sparse profile
  > [include]
  > *.html
  > EOF
  $ echo bar >> data.py

  $ hg ci -qAm "edit profile other"
  $ ls
  backend.sparse
  index.html
  webpage.sparse

Verify conflicting merge pulls in the conflicting changes

  $ hg merge 1
  temporarily included 1 file(s) in the sparse checkout for merging
  merging backend.sparse
  merging data.py
  warning: conflicts while merging backend.sparse! (edit, then use 'hg resolve --mark')
  warning: conflicts while merging data.py! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ rm *.orig
  $ ls
  backend.sparse
  data.py
  index.html
  webpage.sparse

Verify resolving the merge removes the temporarily unioned files

  $ cat > backend.sparse <<EOF
  > [metadata]
  > title: backend sparse profile
  > [include]
  > *.html
  > *.txt
  > EOF
  $ hg resolve -m backend.sparse

  $ cat > data.py <<EOF
  > x
  > foo
  > bar
  > EOF
  $ hg resolve -m data.py
  (no more unresolved files)

  $ hg ci -qAm "merge profiles"
  $ ls
  backend.sparse
  index.html
  readme.txt
  webpage.sparse

  $ hg cat -r . data.py
  x
  foo
  bar

Verify stripping refreshes dirstate

  $ hg strip -q -r .
  $ ls
  backend.sparse
  index.html
  webpage.sparse

Verify rebase conflicts pulls in the conflicting changes

  $ hg up -q 1
  $ ls
  backend.sparse
  data.py
  readme.txt
  webpage.sparse

  $ hg rebase -d 2
  rebasing 1:e7901640ca22 "edit profile"
  temporarily included 1 file(s) in the sparse checkout for merging
  merging backend.sparse
  merging data.py
  warning: conflicts while merging backend.sparse! (edit, then use 'hg resolve --mark')
  warning: conflicts while merging data.py! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ rm *.orig
  $ ls
  backend.sparse
  data.py
  index.html
  webpage.sparse

Verify resolving conflict removes the temporary files

  $ cat > backend.sparse <<EOF
  > [include]
  > *.html
  > *.txt
  > EOF
  $ hg resolve -m backend.sparse

  $ cat > data.py <<EOF
  > x
  > foo
  > bar
  > EOF
  $ hg resolve -m data.py
  (no more unresolved files)
  continue: hg rebase --continue

  $ hg rebase -q --continue
  $ ls
  backend.sparse
  index.html
  readme.txt
  webpage.sparse

  $ hg cat -r . data.py
  x
  foo
  bar

Test checking out a commit that does not contain the sparse profile. The
warning message can be suppressed by setting missingwarning = false in
[sparse] section of your config:

  $ hg sparse --reset
  $ hg rm *.sparse
  $ hg commit -m "delete profiles"
  $ hg up -q ".^"
  $ hg sparse --enable-profile backend.sparse
  $ ls
  index.html
  readme.txt
  $ hg up tip | grep warning
  warning: sparse profile 'backend.sparse' not found in rev 42b23bc43905 - ignoring it
  [1]
  $ ls
  data.py
  index.html
  readme.txt
  $ hg sparse --disable-profile backend.sparse | grep warning
  warning: sparse profile 'backend.sparse' not found in rev 42b23bc43905 - ignoring it
  [1]
  $ cat >> .hg/hgrc <<EOF
  > [sparse]
  > missingwarning = false
  > EOF
  $ hg sparse --enable-profile backend.sparse

  $ cd ..

Test file permissions changing across a sparse profile change
  $ hg init sparseperm
  $ cd sparseperm
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext/fbsparse.py
  > EOF
  $ touch a b
  $ cat > .hgsparse <<EOF
  > a
  > EOF
  $ hg commit -Aqm 'initial'
  $ chmod a+x b
  $ hg commit -qm 'make executable'
  $ cat >> .hgsparse <<EOF
  > b
  > EOF
  $ hg commit -qm 'update profile'
  $ hg up -q 0
  $ hg sparse --enable-profile .hgsparse
  $ hg up -q 2
  $ ls -l b
  -rwxr-xr-x* b (glob)

  $ cd ..

Test profile discovery
  $ hg init sparseprofiles
  $ cd sparseprofiles
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext/fbsparse.py
  > EOF
  $ mkdir -p profiles/foo profiles/bar
  $ touch profiles/README.txt
  $ touch profiles/foo/README
  $ cat > profiles/foo/spam <<EOF
  > %include profiles/bar/eggs
  > [metadata]
  > title: Profile that only includes another
  > EOF
  $ cat > profiles/bar/eggs <<EOF
  > [metadata]
  > title: Base profile including the profiles directory
  > description: This is a base profile, you really want to include this one
  >  if you want to be able to edit profiles.
  > [include]
  > profiles
  > EOF
  $ touch profiles/foo/monty
  $ touch profiles/bar/python
  $ hg add -q profiles
  $ hg commit -qm 'created profiles'
  $ hg sparse --enable-profile profiles/foo/spam
  $ hg sparse --list-profiles
  symbols: * = active profile, ~ = transitively included
  ~ profiles/bar/eggs - Base profile including the profiles directory
  * profiles/foo/spam - Profile that only includes another
  $ hg sparse -l -T json
  [
   {
    "active": "included",
    "metadata": {"description": "This is a base profile, you really want to include this one\nif you want to be able to edit profiles.", "title": "Base profile including the profiles directory"},
    "path": "profiles/bar/eggs"
   },
   {
    "active": "active",
    "metadata": {"title": "Profile that only includes another"},
    "path": "profiles/foo/spam"
   }
  ]
  $ cat >> .hg/hgrc <<EOF
  > [sparse]
  > profile_directory = profiles/
  > EOF
  $ hg sparse -l
  symbols: * = active profile, ~ = transitively included
  ~ profiles/bar/eggs   - Base profile including the profiles directory
    profiles/bar/python
    profiles/foo/monty 
  * profiles/foo/spam   - Profile that only includes another
  $ hg sparse -l -T json
  [
   {
    "active": "included",
    "metadata": {"description": "This is a base profile, you really want to include this one\nif you want to be able to edit profiles.", "title": "Base profile including the profiles directory"},
    "path": "profiles/bar/eggs"
   },
   {
    "active": "inactive",
    "metadata": {},
    "path": "profiles/bar/python"
   },
   {
    "active": "inactive",
    "metadata": {},
    "path": "profiles/foo/monty"
   },
   {
    "active": "active",
    "metadata": {"title": "Profile that only includes another"},
    "path": "profiles/foo/spam"
   }
  ]

The current working directory plays no role in listing profiles:

  $ mkdir otherdir
  $ cd otherdir
  $ hg sparse -l
  symbols: * = active profile, ~ = transitively included
  ~ profiles/bar/eggs   - Base profile including the profiles directory
    profiles/bar/python
    profiles/foo/monty 
  * profiles/foo/spam   - Profile that only includes another
  $ cd ..

Profiles are loaded from the manifest, so excluding a profile directory should
not hamper listing.

  $ hg sparse --exclude profiles/bar
  $ hg sparse -l
  symbols: * = active profile, ~ = transitively included
  ~ profiles/bar/eggs   - Base profile including the profiles directory
    profiles/bar/python
    profiles/foo/monty 
  * profiles/foo/spam   - Profile that only includes another

The metadata section format can have errors, but those are only listed as
warnings:

  $ cat > profiles/foo/errors <<EOF
  > [metadata]
  >   indented line but no current key active
  > not an option line, there is no delimiter
  > EOF
  $ hg add -q profiles
  $ hg commit -qm 'Broken profile added'
  $ hg sparse -l
  symbols: * = active profile, ~ = transitively included
  warning: sparse profile [metadata] section indented lines that do not belong to a multi-line entry, ignoring, in profiles/foo/errors:2
  warning: sparse profile [metadata] section does not appear to have a valid option definition, ignoring, in profiles/foo/errors:3
  ~ profiles/bar/eggs   - Base profile including the profiles directory
    profiles/bar/python
    profiles/foo/errors
    profiles/foo/monty 
  * profiles/foo/spam   - Profile that only includes another


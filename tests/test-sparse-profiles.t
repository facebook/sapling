test sparse

  $ enable sparse purge rebase
  $ hg init myrepo
  $ cd myrepo

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

Show with no sparse profile enabled
  $ hg sparse show
  No sparse profile enabled

  $ hg sparse include '*.sparse'

Verify enabling a single profile works

  $ hg sparse enableprofile webpage.sparse
  $ ls
  backend.sparse
  index.html
  webpage.sparse

Verify enabling two profiles works

  $ hg sparse enableprofile backend.sparse
  $ ls
  backend.sparse
  data.py
  index.html
  webpage.sparse

Verify disabling a profile works

  $ hg sparse disableprofile webpage.sparse
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
  $ hg sparse enableprofile broken.sparse
  warning: sparse profile cannot use paths starting with /, ignoring /absolute/paths/are/ignored, in broken.sparse:4
  abort: A sparse file cannot have includes after excludes in broken.sparse:5
  [255]
  $ hg -q debugstrip . --no-backup

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
  warning: 1 conflicts while merging backend.sparse! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging data.py! (edit, then use 'hg resolve --mark')
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

  $ hg debugstrip -q -r . --no-backup
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
  rebasing e7901640ca22 "edit profile"
  temporarily included 1 file(s) in the sparse checkout for merging
  merging backend.sparse
  merging data.py
  warning: 1 conflicts while merging backend.sparse! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging data.py! (edit, then use 'hg resolve --mark')
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

  $ hg sparse reset
  $ hg rm *.sparse
  $ hg commit -m "delete profiles"
  $ hg up -q ".^"
  $ hg sparse enableprofile backend.sparse
  $ ls
  index.html
  readme.txt
  $ hg up tip | grep warning
  [1]
  $ ls
  data.py
  index.html
  readme.txt
  $ hg sparse disableprofile backend.sparse | grep warning
  [1]
  $ cat >> .hg/hgrc <<EOF
  > [sparse]
  > missingwarning = true
  > EOF
  $ hg sparse enableprofile backend.sparse
  warning: sparse profile 'backend.sparse' not found in rev 42b23bc43905 - ignoring it

  $ cd ..

Test file permissions changing across a sparse profile change
  $ hg init sparseperm
  $ cd sparseperm
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
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
  $ hg sparse enableprofile .hgsparse
  $ hg up -q 2
  $ ls -l b
  -rwxr-xr-x* b (glob)

  $ cd ..

Test profile discovery
  $ hg init sparseprofiles
  $ cd sparseprofiles
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > [hint]
  > ack-hint-ack = True
  > EOF
  $ mkdir -p profiles/foo profiles/bar interesting
  $ touch profiles/README.txt
  $ touch profiles/foo/README
  $ dd if=/dev/zero of=interesting/sizeable bs=4048 count=1024 2> /dev/null
  $ cat > profiles/foo/spam <<EOF
  > %include profiles/bar/eggs
  > [metadata]
  > title: Profile that only includes another
  > EOF
  $ cat > profiles/bar/eggs <<EOF
  > [metadata]
  > title: Profile including the profiles directory
  > description: This is a base profile, you really want to include this one
  >  if you want to be able to edit profiles. In addition, this profiles has
  >  some metadata.
  > foo = bar baz and a whole
  >   lot more.
  > team: me, myself and I
  > [include]
  > profiles
  > EOF
  $ cat > profiles/bar/ham <<EOF
  > %include profiles/bar/eggs
  > [metadata]
  > title: An extended profile including some interesting files
  > [include]
  > interesting
  > EOF
  $ cat > profiles/foo/monty <<EOF
  > [metadata]
  > hidden: this profile is deliberatly hidden from listings
  > [include]
  > eric_idle
  > john_cleese
  > [exclude]
  > guido_van_rossum
  > EOF
  $ touch profiles/bar/python
  $ mkdir hidden
  $ cat > hidden/outsidesparseprofile <<EOF
  > A non-empty file to show that a sparse profile has an impact in terms of
  > file count and bytesize.
  > EOF
  $ hg add -q profiles hidden interesting
  $ hg commit -qm 'created profiles and some data'
  $ hg sparse enableprofile profiles/foo/spam
  $ hg sparse list
  Available Profiles:
  
   ~ profiles/bar/eggs  Profile including the profiles directory
   * profiles/foo/spam  Profile that only includes another
  $ hg sparse list -T json
  [
   {
    "active": "included",
    "metadata": {"description": "This is a base profile, you really want to include this one\nif you want to be able to edit profiles. In addition, this profiles has\nsome metadata.", "foo": "bar baz and a whole\nlot more.", "team": "me, myself and I", "title": "Profile including the profiles directory"},
    "path": "profiles/bar/eggs"
   },
   {
    "active": "active",
    "metadata": {"title": "Profile that only includes another"},
    "path": "profiles/foo/spam"
   }
  ]
  $ enable simplecache
  $ cat >> .hg/hgrc <<EOF
  > [sparse]
  > profile_directory = profiles/
  > [simplecache]
  > caches=
  > EOF
  $ hg sparse list
  Available Profiles:
  
   ~ profiles/bar/eggs    Profile including the profiles directory
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
   * profiles/foo/spam    Profile that only includes another
  hint[sparse-list-verbose]: 1 hidden profiles not shown; add '--verbose' to include these
  $ hg sparse list -T json
  [
   {
    "active": "included",
    "metadata": {"description": "This is a base profile, you really want to include this one\nif you want to be able to edit profiles. In addition, this profiles has\nsome metadata.", "foo": "bar baz and a whole\nlot more.", "team": "me, myself and I", "title": "Profile including the profiles directory"},
    "path": "profiles/bar/eggs"
   },
   {
    "active": "inactive",
    "metadata": {"title": "An extended profile including some interesting files"},
    "path": "profiles/bar/ham"
   },
   {
    "active": "inactive",
    "metadata": {},
    "path": "profiles/bar/python"
   },
   {
    "active": "active",
    "metadata": {"title": "Profile that only includes another"},
    "path": "profiles/foo/spam"
   }
  ]
  hint[sparse-list-verbose]: 1 hidden profiles not shown; add '--verbose' to include these
  $ hg sparse show
  Enabled Profiles:
  
    * profiles/foo/spam    Profile that only includes another
      ~ profiles/bar/eggs  Profile including the profiles directory
  $ hg sparse show -Tjson
  [
   {
    "depth": 0,
    "name": "profiles/foo/spam",
    "title": "Profile that only includes another",
    "type": "profile"
   },
   {
    "depth": 1,
    "name": "profiles/bar/eggs",
    "title": "Profile including the profiles directory",
    "type": "profile"
   }
  ]

The current working directory plays no role in listing profiles:

  $ mkdir otherdir
  $ cd otherdir
  $ hg sparse list
  Available Profiles:
  
   ~ profiles/bar/eggs    Profile including the profiles directory
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
   * profiles/foo/spam    Profile that only includes another
  hint[sparse-list-verbose]: 1 hidden profiles not shown; add '--verbose' to include these
  $ cd ..

Profiles are loaded from the manifest, so excluding a profile directory should
not hamper listing.

  $ hg sparse exclude profiles/bar
  $ hg sparse list
  Available Profiles:
  
   ~ profiles/bar/eggs    Profile including the profiles directory
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
   * profiles/foo/spam    Profile that only includes another
  hint[sparse-list-verbose]: 1 hidden profiles not shown; add '--verbose' to include these
  $ hg sparse show
  Enabled Profiles:
  
    * profiles/foo/spam    Profile that only includes another
      ~ profiles/bar/eggs  Profile including the profiles directory
  
  Additional Excluded Paths:
  
    profiles/bar

Hidden profiles only show up when we use the --verbose switch:

  $ hg sparse list --verbose
  Available Profiles:
  
   ~ profiles/bar/eggs    Profile including the profiles directory
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
     profiles/foo/monty 
   * profiles/foo/spam    Profile that only includes another
  $ cat >> .hg/hgrc << EOF  # enough hints now
  > [hint]
  > ack-sparse-list-verbose = true
  > EOF

We can filter on fields being present or absent. This is how the --verbose
switch is implemented. We can invert that test by filtering on the presence
of the hidden field:

  $ hg sparse list --with-field hidden
  Available Profiles:
  
     profiles/foo/monty

or we can filter on other fields, like missing description:

  $ hg sparse list --without-field description
  Available Profiles:
  
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
   * profiles/foo/spam    Profile that only includes another

multiple tests are cumulative, like a boolean AND operation; both for exclusion

  $ hg sparse list --without-field description --without-field title
  Available Profiles:
  
     profiles/bar/python

and inclusion

  $ hg sparse list --with-field description --with-field title
  Available Profiles:
  
   ~ profiles/bar/eggs  Profile including the profiles directory

Naming the same field in without- and with- filters is an error:

  $ hg sparse list --with-field bar --without-field bar
  abort: You can't specify fields in both --with-field and --without-field, please use only one or the other, for bar
  [255]

We can filter on the contents of a field or the path, case-insensitively:

  $ hg sparse list --filter path:/bar/ --filter title:profile
  Available Profiles:
  
   ~ profiles/bar/eggs  Profile including the profiles directory
     profiles/bar/ham   An extended profile including some interesting files

We can filter on specific files being included in a sparse profile:

  $ hg sparse list --contains-file interesting/sizeable
  Available Profiles:
  
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python

You can specify a revision to list profiles for; in this case the current
sparse configuration is ignored; no profile can be 'active' or 'included':

  $ cat > profiles/foo/new_in_later_revision <<EOF
  > [metadata]
  > title: this profile is only available in a later revision, not the current.
  > EOF
  $ hg commit -Aqm 'Add another profile in a later revision'
  $ hg up -r ".^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg sparse list -r tip
  Available Profiles:
  
     profiles/bar/eggs                   Profile including the profiles directory
     profiles/bar/ham                    An extended profile including some interesting files
     profiles/bar/python               
     profiles/foo/new_in_later_revision  this profile is only available in a later revision, not the current.
     profiles/foo/spam                   Profile that only includes another
  $ hg -q debugstrip -r tip --no-backup

The metadata section format can have errors, but those are only listed as
warnings:

  $ cat > profiles/foo/errors <<EOF
  > [metadata]
  >   indented line but no current key active
  > not an option line, there is no delimiter
  > EOF
  $ hg add -q profiles
  $ hg commit -qm 'Broken profile added'
  $ hg sparse list
  Available Profiles:
  
  warning: sparse profile [metadata] section indented lines that do not belong to a multi-line entry, ignoring, in profiles/foo/errors:2
  warning: sparse profile [metadata] section does not appear to have a valid option definition, ignoring, in profiles/foo/errors:3
   ~ profiles/bar/eggs    Profile including the profiles directory
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
     profiles/foo/errors
   * profiles/foo/spam    Profile that only includes another

The .hg/sparse file could list non-existing profiles, these should be ignored
when listing:

  $ hg sparse enableprofile nonesuch
  the profile 'nonesuch' does not exist in the current commit, it will only take effect when you check out a commit containing a profile with that name
  $ hg sparse list
  Available Profiles:
  
  warning: sparse profile [metadata] section indented lines that do not belong to a multi-line entry, ignoring, in profiles/foo/errors:2
  warning: sparse profile [metadata] section does not appear to have a valid option definition, ignoring, in profiles/foo/errors:3
   ~ profiles/bar/eggs    Profile including the profiles directory
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
     profiles/foo/errors
   * profiles/foo/spam    Profile that only includes another
  $ hg sparse disableprofile nonesuch

Can switch between profiles

  $ test -f interesting/sizeable
  [1]
  $ hg sparse switchprofile profiles/bar/ham
  $ hg sparse list
  Available Profiles:
  
  warning: sparse profile [metadata] section indented lines that do not belong to a multi-line entry, ignoring, in profiles/foo/errors:2
  warning: sparse profile [metadata] section does not appear to have a valid option definition, ignoring, in profiles/foo/errors:3
   ~ profiles/bar/eggs    Profile including the profiles directory
   * profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
     profiles/foo/errors
     profiles/foo/spam    Profile that only includes another
  $ test -f interesting/sizeable

We can look at invididual profiles:

  $ hg sparse explain profiles/bar/eggs
  profiles/bar/eggs
  
  Profile including the profiles directory
  """"""""""""""""""""""""""""""""""""""""
  
  This is a base profile, you really want to include this one if you want to be
  able to edit profiles. In addition, this profiles has some metadata.
  
  Size impact compared to a full checkout
  =======================================
  
  file count    8 (80.00%)
  
  Additional metadata
  ===================
  
  foo           bar baz and a whole lot more.
  team          me, myself and I
  
  Inclusion rules
  ===============
  
    profiles
  hint[sparse-explain-verbose]: use 'hg sparse explain --verbose profiles/bar/eggs' to include the total file size for a give profile

  $ hg sparse explain profiles/bar/ham -T json
  [
   {
    "excludes": [],
    "includes": ["interesting"],
    "metadata": {"title": "An extended profile including some interesting files"},
    "path": "profiles/bar/ham",
    "profiles": ["profiles/bar/eggs"],
    "stats": {"filecount": 9, "filecountpercentage": 90.0}
   }
  ]
  hint[sparse-explain-verbose]: use 'hg sparse explain --verbose profiles/bar/ham' to include the total file size for a give profile
  $ hg sparse explain profiles/bar/ham -T json --verbose
  [
   {
    "excludes": [],
    "includes": ["interesting"],
    "metadata": {"title": "An extended profile including some interesting files"},
    "path": "profiles/bar/ham",
    "profiles": ["profiles/bar/eggs"],
    "stats": {"filecount": 9, "filecountpercentage": 90.0, "totalsize": 4145875}
   }
  ]
  $ cat >> .hg/hgrc << EOF  # enough hints now
  > [hint]
  > ack-sparse-explain-verbose = true
  > EOF
  $ hg sparse explain profiles/bar/eggs
  profiles/bar/eggs
  
  Profile including the profiles directory
  """"""""""""""""""""""""""""""""""""""""
  
  This is a base profile, you really want to include this one if you want to be
  able to edit profiles. In addition, this profiles has some metadata.
  
  Size impact compared to a full checkout
  =======================================
  
  file count    8 (80.00%)
  
  Additional metadata
  ===================
  
  foo           bar baz and a whole lot more.
  team          me, myself and I
  
  Inclusion rules
  ===============
  
    profiles

  $ hg sparse explain profiles/bar/eggs --verbose
  profiles/bar/eggs
  
  Profile including the profiles directory
  """"""""""""""""""""""""""""""""""""""""
  
  This is a base profile, you really want to include this one if you want to be
  able to edit profiles. In addition, this profiles has some metadata.
  
  Size impact compared to a full checkout
  =======================================
  
  file count    8 (80.00%)
  total size    723 bytes
  
  Additional metadata
  ===================
  
  foo           bar baz and a whole lot more.
  team          me, myself and I
  
  Inclusion rules
  ===============
  
    profiles

  $ hg sparse explain profiles/bar/eggs profiles/bar/ham profiles/nonsuch --verbose
  The profile profiles/nonsuch was not found
  profiles/bar/eggs
  
  Profile including the profiles directory
  """"""""""""""""""""""""""""""""""""""""
  
  This is a base profile, you really want to include this one if you want to be
  able to edit profiles. In addition, this profiles has some metadata.
  
  Size impact compared to a full checkout
  =======================================
  
  file count    8 (80.00%)
  total size    723 bytes
  
  Additional metadata
  ===================
  
  foo           bar baz and a whole lot more.
  team          me, myself and I
  
  Inclusion rules
  ===============
  
    profiles
  
  profiles/bar/ham
  
  An extended profile including some interesting files
  """"""""""""""""""""""""""""""""""""""""""""""""""""
  
  Size impact compared to a full checkout
  =======================================
  
  file count    9 (90.00%)
  total size    3.95 MB
  
  Profiles included
  =================
  
    profiles/bar/eggs
  
  Inclusion rules
  ===============
  
    interesting

  $ hg sparse explain profiles/bar/eggs -T "{path}\n{metadata.title}\n{stats.filecount}\n"
  profiles/bar/eggs
  Profile including the profiles directory
  8

The -r switch tells hg sparse explain to look at something other than the
current working copy:

  $ hg sparse reset
  $ touch interesting/later_revision
  $ hg commit -Aqm 'Add another file in a later revision'
  $ hg sparse explain profiles/bar/ham -T "{stats.filecount}\n" -r ".^"
  9
  $ hg sparse explain profiles/bar/ham -T "{stats.filecount}\n" -r .
  10
  $ hg sparse list --contains-file interesting/later_revision -r ".^"
  Available Profiles:
  
  warning: sparse profile [metadata] section indented lines that do not belong to a multi-line entry, ignoring, in profiles/foo/errors:2
  warning: sparse profile [metadata] section does not appear to have a valid option definition, ignoring, in profiles/foo/errors:3
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
     profiles/foo/errors
  $ hg sparse list --contains-file interesting/later_revision -r .
  Available Profiles:
  
  warning: sparse profile [metadata] section indented lines that do not belong to a multi-line entry, ignoring, in profiles/foo/errors:2
  warning: sparse profile [metadata] section does not appear to have a valid option definition, ignoring, in profiles/foo/errors:3
     profiles/bar/ham     An extended profile including some interesting files
     profiles/bar/python
     profiles/foo/errors
  $ hg up -q ".^"

We can list the files in a profile with the hg sparse files command:

  $ hg sparse files profiles/bar/eggs
  profiles/README.txt
  profiles/bar/eggs
  profiles/bar/ham
  profiles/bar/python
  profiles/foo/README
  profiles/foo/errors
  profiles/foo/monty
  profiles/foo/spam
  $ hg sparse files profiles/bar/eggs **/README **/README.*
  profiles/README.txt
  profiles/foo/README

Files for included profiles are taken along:

  $ hg sparse files profiles/bar/ham | wc -l
  \s*9 (re)

File count and size data for hg explain is cached in the simplecache extension:

  $ cat >> .hg/hgrc <<EOF
  > [simplecache]
  > caches=local
  > cachedir=$TESTTMP/cache
  > EOF
  $ hg sparse explain profiles/bar/eggs profiles/bar/ham > /dev/null
  $ ls -1 $TESTTMP/cache
  sparseprofile:profiles__bar__eggs:07b307002dae98240fe64a42df9598263f69d925:v2
  sparseprofile:profiles__bar__ham:07b307002dae98240fe64a42df9598263f69d925:v2
  sparseprofilestats:sparseprofiles:profiles__bar__eggs:ab56132ffe9320163b73f769a0a32d84c6869949:0:07b307002dae98240fe64a42df9598263f69d925:False:v2
  sparseprofilestats:sparseprofiles:profiles__bar__ham:07b4880e6fcb1f6b13998b0c6bc47f256a0f6d33:0:07b307002dae98240fe64a42df9598263f69d925:False:v2
  sparseprofilestats:sparseprofiles:unfiltered:07b307002dae98240fe64a42df9598263f69d925:v2

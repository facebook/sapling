#chg-compatible

test sparse

  $ setconfig workingcopy.ruststatus=False
  $ configure modernclient
  $ enable sparse rebase
  $ newclientrepo myrepo

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

 Match files with two sparse profiles

  $ hg debugsparsematch --sparse-profile webpage.sparse --sparse-profile backend.sparse index.html foo.py bar.html
  considering 3 file(s)
  index.html
  foo.py
  bar.html

Match files with one sparse profile

  $ hg debugsparsematch --sparse-profile backend.sparse index.html foo.py bar.html
  considering 3 file(s)
  foo.py

Match fileset
NOTE - this command is used in validate_sparse_profiles scripts, so be careful with
changing it!

  $ cat > file_list.txt <<EOF
  > first.py
  > second.py
  > something/not/matched.cpp
  > EOF
  $ hg debugsparsematch --sparse-profile backend.sparse listfile:file_list.txt no_match.cpp third.py
  considering 5 file(s)
  first.py
  second.py
  third.py
  $ hg debugsparsematch --sparse-profile backend.sparse listfile:file_list.txt no_match.cpp third.py -0 > "$TESTTMP/out.bin"
  considering 5 file(s)
  $ f --hexdump "$TESTTMP/out.bin"
  $TESTTMP/out.bin:
  0000: 66 69 72 73 74 2e 70 79 00 73 65 63 6f 6e 64 2e |first.py.second.|
  0010: 70 79 00 74 68 69 72 64 2e 70 79 00             |py.third.py.|
  $ rm file_list.txt

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
  $ hg -q debugstrip . --no-backup 2>/dev/null

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

  $ hg up -q 'desc(initial)'
  $ ls
  backend.sparse
  data.py
  webpage.sparse

  $ hg up -q 'desc(edit)'
  $ ls
  backend.sparse
  data.py
  readme.txt
  webpage.sparse

Introduce a conflicting .hgsparse change

  $ hg up -q 'desc(initial)'
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

  $ hg merge e7901640ca22b6074f2724228278811021be5bd9
  temporarily included 1 file(s) in the sparse checkout for merging
  merging backend.sparse
  merging data.py
  warning: 1 conflicts while merging backend.sparse! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging data.py! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
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

  $ hg up -q e7901640ca22b6074f2724228278811021be5bd9
  $ ls
  backend.sparse
  data.py
  readme.txt
  webpage.sparse

  $ hg rebase -d 'max(desc(edit))'
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
  the profile 'backend.sparse' does not exist in the current commit, it will only take effect when you check out a commit containing a profile with that name
  (if the path is a typo, use 'hg sparse disableprofile' to remove it)
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
  $ hg up -q 'desc(initial)'
  $ hg sparse enableprofile .hgsparse
  $ hg up -q 'desc(update)'
  $ f -m b
  b: mode=755

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
  $ mkdir -p profiles/foo profiles/bar profiles/.hidden interesting
  $ touch profiles/README.txt
  $ touch profiles/foo/README
  $ touch profiles/.hidden/nope
  $ touch profiles/why_is_this_here.py
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
    "status": "*",
    "title": "Profile that only includes another",
    "type": "profile"
   },
   {
    "depth": 1,
    "name": "profiles/bar/eggs",
    "status": "~",
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
  
     profiles/bar/ham  An extended profile including some interesting files

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
  (if the path is a typo, use 'hg sparse disableprofile' to remove it)
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
  
  file count    10 (83.33%)
  
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
    "lines": [["profile", "profiles/bar/eggs"], ["include", "interesting"]],
    "metadata": {"title": "An extended profile including some interesting files"},
    "path": "profiles/bar/ham",
    "profiles": ["profiles/bar/eggs"],
    "stats": {"filecount": 11, "filecountpercentage": 91.66666666666666}
   }
  ]
  hint[sparse-explain-verbose]: use 'hg sparse explain --verbose profiles/bar/ham' to include the total file size for a give profile
  $ hg sparse explain profiles/bar/ham -T json --verbose
  [
   {
    "lines": [["profile", "profiles/bar/eggs"], ["include", "interesting"]],
    "metadata": {"title": "An extended profile including some interesting files"},
    "path": "profiles/bar/ham",
    "profiles": ["profiles/bar/eggs"],
    "stats": {"filecount": 11, "filecountpercentage": 91.66666666666666, "totalsize": 4145875}
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
  
  file count    10 (83.33%)
  
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
  
  file count    10 (83.33%)
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
  
  file count    10 (83.33%)
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
  
  file count    11 (91.67%)
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
  10

The -r switch tells hg sparse explain to look at something other than the
current working copy:

  $ hg sparse reset
  $ touch interesting/later_revision
  $ hg commit -Aqm 'Add another file in a later revision'
  $ hg sparse explain profiles/bar/ham -T "{stats.filecount}\n" -r ".^"
  11
  $ hg sparse explain profiles/bar/ham -T "{stats.filecount}\n" -r .
  12
  $ hg sparse list --contains-file interesting/later_revision -r ".^"
  Available Profiles:
  
  warning: sparse profile [metadata] section indented lines that do not belong to a multi-line entry, ignoring, in profiles/foo/errors:2
  warning: sparse profile [metadata] section does not appear to have a valid option definition, ignoring, in profiles/foo/errors:3
     profiles/bar/ham  An extended profile including some interesting files
  $ hg sparse list --contains-file interesting/later_revision -r .
  Available Profiles:
  
  warning: sparse profile [metadata] section indented lines that do not belong to a multi-line entry, ignoring, in profiles/foo/errors:2
  warning: sparse profile [metadata] section does not appear to have a valid option definition, ignoring, in profiles/foo/errors:3
     profiles/bar/ham  An extended profile including some interesting files
  $ hg up -q ".^"

We can list the files in a profile with the hg sparse files command:

  $ hg sparse files profiles/bar/eggs
  profiles/README.txt
  profiles/why_is_this_here.py
  profiles/.hidden/nope
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
  \s*11 (re)

File count and size data for hg explain is cached in the simplecache extension:

  $ cat >> .hg/hgrc <<EOF
  > [simplecache]
  > caches=local
  > cachedir=$TESTTMP/cache
  > EOF
  $ hg sparse explain profiles/bar/eggs profiles/bar/ham > /dev/null
  $ ls -1 $TESTTMP/cache
  sparseprofile:profiles__bar__eggs:*:v2 (glob)
  sparseprofile:profiles__bar__ham:*:v2 (glob)

Test non-existing profiles are properly reported
  $ newrepo sparsenoprofile
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > EOF
  $ cat > profile-ok <<EOF
  > [metadata]
  > title: This is a regular profile
  > EOF
  $ cat > profile-includes-existing <<EOF
  > %include profile-existing
  > [metadata]
  > title: This profile includes an existing profile
  > EOF
  $ cat > profile-existing <<EOF
  > [metadata]
  > title: A regular included profile
  > EOF
  $ cat > profile-includes-non-existing <<EOF
  > %include profile-non-existing
  > [metadata]
  > title: This profile includes a non-existing profile
  > EOF
  $ hg commit -Aqm 'initial'
  $ hg sparse enableprofile profile-ok
  $ hg sparse enableprofile profile-wrong
  the profile 'profile-wrong' does not exist in the current commit, it will only take effect when you check out a commit containing a profile with that name
  (if the path is a typo, use 'hg sparse disableprofile' to remove it)
  $ hg sparse enableprofile profile-includes-existing
  $ hg sparse enableprofile profile-includes-non-existing
  $ hg sparse show
  Enabled Profiles:
  
    * profile-includes-existing        This profile includes an existing profile
      ~ profile-existing               A regular included profile
    * profile-includes-non-existing    This profile includes a non-existing profile
      ! profile-non-existing         
    * profile-ok                       This is a regular profile
    ! profile-wrong                  
  $ hg sparse show -Tjson
  [
   {
    "depth": 0,
    "name": "profile-includes-existing",
    "status": "*",
    "title": "This profile includes an existing profile",
    "type": "profile"
   },
   {
    "depth": 1,
    "name": "profile-existing",
    "status": "~",
    "title": "A regular included profile",
    "type": "profile"
   },
   {
    "depth": 0,
    "name": "profile-includes-non-existing",
    "status": "*",
    "title": "This profile includes a non-existing profile",
    "type": "profile"
   },
   {
    "depth": 1,
    "name": "profile-non-existing",
    "status": "!",
    "type": "profile"
   },
   {
    "depth": 0,
    "name": "profile-ok",
    "status": "*",
    "title": "This is a regular profile",
    "type": "profile"
   },
   {
    "depth": 0,
    "name": "profile-wrong",
    "status": "!",
    "type": "profile"
   }
  ]
  $ cd $TESTTMP

Verify that removing from a sparse profile removes from disk
  $ hg init remove_test_repo
  $ cd remove_test_repo
  $ echo x > data.py
  $ cat > webpage.sparse <<EOF
  > [metadata]
  > title: frontend sparse profile
  > [include]
  > *.sparse
  > *.py
  > EOF
  $ hg ci -Aqm 'initial'
  $ hg sparse enable webpage.sparse
  $ ls
  data.py
  webpage.sparse
  $ cat > webpage.sparse <<EOF
  > [metadata]
  > title: frontend sparse profile
  > [include]
  > *.sparse
  > EOF
  $ hg commit -m 'remove py'
  $ ls
  webpage.sparse

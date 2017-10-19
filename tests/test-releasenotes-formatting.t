#require fuzzywuzzy

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > releasenotes=
  > EOF

  $ hg init simple-repo
  $ cd simple-repo

A fix with a single line results in a bullet point in the appropriate section

  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > single line fix
  > 
  > .. fix::
  > 
  >    Simple fix with a single line content entry.
  > EOF

  $ hg releasenotes -r . $TESTTMP/relnotes-single-line

  $ cat $TESTTMP/relnotes-single-line
  Bug Fixes
  =========
  
  * Simple fix with a single line content entry.

A fix with multiple lines is handled correctly

  $ touch fix2
  $ hg -q commit -A -l - << EOF
  > multi line fix
  > 
  > .. fix::
  > 
  >    First line of fix entry.
  >    A line after it without a space.
  > 
  >    A new paragraph in the fix entry. And this is a really long line. It goes on for a while.
  >    And it wraps around to a new paragraph.
  > EOF

  $ hg releasenotes -r . $TESTTMP/relnotes-multi-line
  $ cat $TESTTMP/relnotes-multi-line
  Bug Fixes
  =========
  
  * First line of fix entry. A line after it without a space.
  
    A new paragraph in the fix entry. And this is a really long line. It goes on
    for a while. And it wraps around to a new paragraph.

A release note with a title results in a sub-section being written

  $ touch fix3
  $ hg -q commit -A -l - << EOF
  > fix with title
  > 
  > .. fix:: Fix Title
  > 
  >    First line of fix with title.
  > 
  >    Another paragraph of fix with title. But this is a paragraph
  >    with multiple lines.
  > EOF

  $ hg releasenotes -r . $TESTTMP/relnotes-fix-with-title
  $ cat $TESTTMP/relnotes-fix-with-title
  Bug Fixes
  =========
  
  Fix Title
  ---------
  
  First line of fix with title.
  
  Another paragraph of fix with title. But this is a paragraph with multiple
  lines.

  $ cd ..

Formatting of multiple bullet points works

  $ hg init multiple-bullets
  $ cd multiple-bullets
  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix::
  > 
  >    first fix
  > EOF

  $ touch fix2
  $ hg -q commit -A -l - << EOF
  > commit 2
  > 
  > .. fix::
  > 
  >    second fix
  > 
  >    Second paragraph of second fix.
  > EOF

  $ touch fix3
  $ hg -q commit -A -l - << EOF
  > commit 3
  > 
  > .. fix::
  > 
  >    third fix
  > EOF

  $ hg releasenotes -r 'all()' $TESTTMP/relnotes-multiple-bullets
  $ cat $TESTTMP/relnotes-multiple-bullets
  Bug Fixes
  =========
  
  * first fix
  
  * second fix
  
    Second paragraph of second fix.
  
  * third fix

  $ cd ..

Formatting of multiple sections works

  $ hg init multiple-sections
  $ cd multiple-sections
  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix::
  > 
  >    first fix
  > EOF

  $ touch feature1
  $ hg -q commit -A -l - << EOF
  > commit 2
  > 
  > .. feature::
  > 
  >    description of the new feature
  > EOF

  $ touch fix2
  $ hg -q commit -A -l - << EOF
  > commit 3
  > 
  > .. fix::
  > 
  >    second fix
  > EOF

  $ hg releasenotes -r 'all()' $TESTTMP/relnotes-multiple-sections
  $ cat $TESTTMP/relnotes-multiple-sections
  New Features
  ============
  
  * description of the new feature
  
  Bug Fixes
  =========
  
  * first fix
  
  * second fix

  $ cd ..

Section with subsections and bullets

  $ hg init multiple-subsections
  $ cd multiple-subsections

  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix:: Title of First Fix
  > 
  >    First paragraph of first fix.
  > 
  >    Second paragraph of first fix.
  > EOF

  $ touch fix2
  $ hg -q commit -A -l - << EOF
  > commit 2
  > 
  > .. fix:: Title of Second Fix
  > 
  >    First paragraph of second fix.
  > 
  >    Second paragraph of second fix.
  > EOF

  $ hg releasenotes -r 'all()' $TESTTMP/relnotes-multiple-subsections
  $ cat $TESTTMP/relnotes-multiple-subsections
  Bug Fixes
  =========
  
  Title of First Fix
  ------------------
  
  First paragraph of first fix.
  
  Second paragraph of first fix.
  
  Title of Second Fix
  -------------------
  
  First paragraph of second fix.
  
  Second paragraph of second fix.

Now add bullet points to sections having sub-sections

  $ touch fix3
  $ hg -q commit -A -l - << EOF
  > commit 3
  > 
  > .. fix::
  > 
  >    Short summary of fix 3
  > EOF

  $ hg releasenotes -r 'all()' $TESTTMP/relnotes-multiple-subsections-with-bullets
  $ cat $TESTTMP/relnotes-multiple-subsections-with-bullets
  Bug Fixes
  =========
  
  Title of First Fix
  ------------------
  
  First paragraph of first fix.
  
  Second paragraph of first fix.
  
  Title of Second Fix
  -------------------
  
  First paragraph of second fix.
  
  Second paragraph of second fix.
  
  Other Changes
  -------------
  
  * Short summary of fix 3

  $ cd ..

Multiple 'Other Changes' sub-sections for every section

  $ hg init multiple-otherchanges
  $ cd multiple-otherchanges

  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix:: Title of First Fix
  > 
  >    First paragraph of fix 1.
  > EOF

  $ touch feature1
  $ hg -q commit -A -l - << EOF
  > commit 2
  > 
  > .. feature:: Title of First Feature
  > 
  >    First paragraph of feature 1.
  > EOF

  $ touch feature2
  $ hg -q commit -A -l - << EOF
  > commit 3
  > 
  > .. feature::
  > 
  >    Short summary of feature 2.
  > EOF

  $ touch fix2
  $ hg -q commit -A -l - << EOF
  > commit 4
  > 
  > .. fix::
  > 
  >    Short summary of fix 2
  > EOF

  $ hg releasenotes -r 'all()' $TESTTMP/relnotes-multiple-otherchanges
  $ cat $TESTTMP/relnotes-multiple-otherchanges
  New Features
  ============
  
  Title of First Feature
  ----------------------
  
  First paragraph of feature 1.
  
  Other Changes
  -------------
  
  * Short summary of feature 2.
  
  Bug Fixes
  =========
  
  Title of First Fix
  ------------------
  
  First paragraph of fix 1.
  
  Other Changes
  -------------
  
  * Short summary of fix 2

  $ cd ..

Using custom sections in notes

  $ hg init custom-section
  $ cd custom-section
  $ cat >> .hgreleasenotes << EOF
  > [sections]
  > testsection=Name of Section
  > EOF

  $ touch a
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. testsection::
  > 
  >    First paragraph under this admonition.
  > EOF

  $ hg releasenotes -r . $TESTTMP/relnotes-custom-section
  $ cat $TESTTMP/relnotes-custom-section
  Name of Section
  ===============
  
  * First paragraph under this admonition.

Overriding default sections (For eg. by default feature = New Features)

  $ cat >> .hgreleasenotes << EOF
  > [sections]
  > feature=Feature Additions
  > EOF

  $ touch b
  $ hg -q commit -A -l - << EOF
  > commit 2
  > 
  > .. feature::
  > 
  >    Adds a new feature.
  > EOF

  $ hg releasenotes -r . $TESTTMP/relnotes-override-section
  $ cat $TESTTMP/relnotes-override-section
  Feature Additions
  =================
  
  * Adds a new feature.

  $ cd ..

Testing output for the --check (-c) flag

  $ hg init check-flag
  $ cd check-flag

  $ touch a
  $ hg -q commit -A -l - << EOF
  > .. asf::
  > 
  >    First paragraph under this admonition.
  > EOF

Suggest similar admonition in place of the invalid one.

  $ hg releasenotes -r . -c
  Invalid admonition 'asf' present in changeset 4026fe9e1c20

  $ touch b
  $ hg -q commit -A -l - << EOF
  > .. fixes::
  > 
  >    First paragraph under this admonition.
  > EOF

  $ hg releasenotes -r . -c
  Invalid admonition 'fixes' present in changeset 0e7130d2705c
  (did you mean fix?)

  $ cd ..

Usage of --list flag

  $ hg init relnotes-list
  $ cd relnotes-list
  $ hg releasenotes -l
  feature: New Features
  bc: Backwards Compatibility Changes
  fix: Bug Fixes
  perf: Performance Improvements
  api: API Changes

  $ cd ..

Raise error on simultaneous usage of flags

  $ hg init relnotes-raise-error
  $ cd relnotes-raise-error
  $ hg releasenotes -r . -l
  abort: cannot use both '--list' and '--rev'
  [255]

  $ hg releasenotes -l -c
  abort: cannot use both '--list' and '--check'
  [255]

Display release notes for specified revs if no file is mentioned

  $ hg init relnotes-nofile
  $ cd relnotes-nofile

  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix:: Title of First Fix
  > 
  >    First paragraph of fix 1.
  > EOF

  $ hg releasenote -r .
  Bug Fixes
  =========
  
  Title of First Fix
  ------------------
  
  First paragraph of fix 1.

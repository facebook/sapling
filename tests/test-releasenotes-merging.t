#require fuzzywuzzy

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > releasenotes=
  > EOF

  $ hg init simple-repo
  $ cd simple-repo

A fix directive from commit message is added to release notes

  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix::
  > 
  >    Fix from commit message.
  > EOF

  $ cat >> $TESTTMP/single-fix-bullet << EOF
  > Bug Fixes
  > =========
  > 
  > * Fix from release notes.
  > EOF

  $ hg releasenotes -r . $TESTTMP/single-fix-bullet

  $ cat $TESTTMP/single-fix-bullet
  Bug Fixes
  =========
  
  * Fix from release notes.
  
  * Fix from commit message.

Processing again ignores the already added bullet.

  $ hg releasenotes -r . $TESTTMP/single-fix-bullet

  $ cat $TESTTMP/single-fix-bullet
  Bug Fixes
  =========
  
  * Fix from release notes.
  
  * Fix from commit message.

  $ cd ..

Sections are unioned

  $ hg init subsections
  $ cd subsections
  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > Commit 1
  > 
  > .. feature:: Commit Message Feature
  > 
  >    This describes a feature from a commit message.
  > EOF

  $ cat >> $TESTTMP/single-feature-section << EOF
  > New Features
  > ============
  > 
  > Notes Feature
  > -------------
  > 
  > This describes a feature from a release notes file.
  > EOF

  $ hg releasenotes -r . $TESTTMP/single-feature-section

  $ cat $TESTTMP/single-feature-section
  New Features
  ============
  
  Notes Feature
  -------------
  
  This describes a feature from a release notes file.
  
  Commit Message Feature
  ----------------------
  
  This describes a feature from a commit message.

Doing it again won't add another section

  $ hg releasenotes -r . $TESTTMP/single-feature-section
  Commit Message Feature already exists in feature section; ignoring

  $ cat $TESTTMP/single-feature-section
  New Features
  ============
  
  Notes Feature
  -------------
  
  This describes a feature from a release notes file.
  
  Commit Message Feature
  ----------------------
  
  This describes a feature from a commit message.

  $ cd ..

Bullets from rev merge with those from notes file.

  $ hg init bullets
  $ cd bullets
  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix::
  > 
  >    this is fix1.
  > EOF

  $ touch fix2
  $ hg -q commit -A -l - << EOF
  > commit 2
  > 
  > .. fix::
  > 
  >    this is fix2.
  > EOF

  $ hg releasenotes -r 'all()' $TESTTMP/relnotes-bullet-problem
  $ cat $TESTTMP/relnotes-bullet-problem
  Bug Fixes
  =========
  
  * this is fix1.
  
  * this is fix2.
  $ touch fix3
  $ hg -q commit -A -l - << EOF
  > commit 3
  > 
  > .. fix::
  > 
  >    this is fix3.
  > EOF

  $ hg releasenotes -r . $TESTTMP/relnotes-bullet-problem
  $ cat $TESTTMP/relnotes-bullet-problem
  Bug Fixes
  =========
  
  * this is fix1.
  
  * this is fix2.
  
  * this is fix3.

  $ cd ..

Ignores commit messages containing issueNNNN based on issue number.

  $ hg init simple-fuzzrepo
  $ cd simple-fuzzrepo
  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix::
  > 
  >    Resolved issue4567.
  > EOF

  $ cat >> $TESTTMP/issue-number-notes << EOF
  > Bug Fixes
  > =========
  > 
  > * Fixed issue1234 related to XYZ.
  > 
  > * Fixed issue4567 related to ABC.
  > 
  > * Fixed issue3986 related to PQR.
  > EOF

  $ hg releasenotes -r . $TESTTMP/issue-number-notes
  "issue4567" already exists in notes; ignoring

  $ cat $TESTTMP/issue-number-notes
  Bug Fixes
  =========
  
  * Fixed issue1234 related to XYZ.
  
  * Fixed issue4567 related to ABC.
  
  * Fixed issue3986 related to PQR.

  $ cd ..

Adds short commit messages (words < 10) without
comparison unless there is an exact match.

  $ hg init tempdir
  $ cd tempdir
  $ touch feature1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. feature::
  > 
  >    Adds a new feature 1.
  > EOF

  $ hg releasenotes -r . $TESTTMP/short-sentence-notes

  $ touch feature2
  $ hg -q commit -A -l - << EOF
  > commit 2
  > 
  > .. feature::
  > 
  >    Adds a new feature 2.
  > EOF

  $ hg releasenotes -r . $TESTTMP/short-sentence-notes
  $ cat $TESTTMP/short-sentence-notes
  New Features
  ============
  
  * Adds a new feature 1.
  
  * Adds a new feature 2.

  $ cd ..

Ignores commit messages based on fuzzy comparison.

  $ hg init fuzznotes
  $ cd fuzznotes
  $ touch fix1
  $ hg -q commit -A -l - << EOF
  > commit 1
  > 
  > .. fix::
  > 
  >    This is a fix with another line.
  >    And it is a big one.
  > EOF

  $ cat >> $TESTTMP/fuzz-ignore-notes << EOF
  > Bug Fixes
  > =========
  > 
  > * Fixed issue4567 by improving X.
  > 
  > * This is the first line. This is next line with one newline.
  > 
  >   This is another line written after two newlines. This is going to be a big one.
  > 
  > * This fixes another problem.
  > EOF

  $ hg releasenotes -r . $TESTTMP/fuzz-ignore-notes
  "This is a fix with another line. And it is a big one." already exists in notes file; ignoring

  $ cat $TESTTMP/fuzz-ignore-notes
  Bug Fixes
  =========
  
  * Fixed issue4567 by improving X.
  
  * This is the first line. This is next line with one newline.
  
    This is another line written after two newlines. This is going to be a big
    one.
  
  * This fixes another problem.

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

Processing again will no-op
TODO this is buggy

  $ hg releasenotes -r . $TESTTMP/single-fix-bullet

  $ cat $TESTTMP/single-fix-bullet
  Bug Fixes
  =========
  
  * Fix from release notes.
  
    Fix from commit message.
  
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

Bullets don't merge properly

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
  
    this is fix2.
  
  * this is fix3.


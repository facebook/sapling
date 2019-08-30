  $ setconfig extensions.treemanifest=!
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > share =
  > [format]
  > dirstate = 0
  > EOF

store and revlogv1 are required in source

  $ hg --config format.usestore=false init no-store
  $ hg -R no-store debugupgraderepo
  abort: cannot upgrade repository; requirement missing: store
  [255]

  $ hg init no-revlogv1
  $ cat > no-revlogv1/.hg/requires << EOF
  > dotencode
  > fncache
  > generaldelta
  > store
  > EOF

  $ hg -R no-revlogv1 debugupgraderepo
  abort: cannot upgrade repository; requirement missing: revlogv1
  [255]

Cannot upgrade shared repositories

  $ hg init share-parent
  $ hg -q share share-parent share-child

  $ hg -R share-child debugupgraderepo
  abort: cannot upgrade repository; unsupported source requirement: shared
  [255]

An upgrade of a repository created with recommended settings only suggests optimizations

  $ hg init empty
  $ cd empty
  $ hg debugformat
  format-variant repo
  fncache:        yes
  dotencode:      yes
  generaldelta:   yes
  plain-cl-delta: yes
  compression:    zlib
  $ hg debugformat --verbose
  format-variant repo config default
  fncache:        yes    yes     yes
  dotencode:      yes    yes     yes
  generaldelta:   yes    yes     yes
  plain-cl-delta: yes    yes     yes
  compression:    zlib   zlib    zlib
  $ hg debugformat --verbose --config format.usegfncache=no
  format-variant repo config default
  fncache:        yes    yes     yes
  dotencode:      yes    yes     yes
  generaldelta:   yes    yes     yes
  plain-cl-delta: yes    yes     yes
  compression:    zlib   zlib    zlib
  $ hg debugformat --verbose --config format.usegfncache=no --color=debug
  format-variant repo config default
  [formatvariant.name.uptodate|fncache:       ][formatvariant.repo.uptodate| yes][formatvariant.config.default|    yes][formatvariant.default|     yes]
  [formatvariant.name.uptodate|dotencode:     ][formatvariant.repo.uptodate| yes][formatvariant.config.default|    yes][formatvariant.default|     yes]
  [formatvariant.name.uptodate|generaldelta:  ][formatvariant.repo.uptodate| yes][formatvariant.config.default|    yes][formatvariant.default|     yes]
  [formatvariant.name.uptodate|plain-cl-delta:][formatvariant.repo.uptodate| yes][formatvariant.config.default|    yes][formatvariant.default|     yes]
  [formatvariant.name.uptodate|compression:   ][formatvariant.repo.uptodate| zlib][formatvariant.config.default|   zlib][formatvariant.default|    zlib]
  $ hg debugformat -Tjson
  [
   {
    "config": true,
    "default": true,
    "name": "fncache",
    "repo": true
   },
   {
    "config": true,
    "default": true,
    "name": "dotencode",
    "repo": true
   },
   {
    "config": true,
    "default": true,
    "name": "generaldelta",
    "repo": true
   },
   {
    "config": true,
    "default": true,
    "name": "plain-cl-delta",
    "repo": true
   },
   {
    "config": "zlib",
    "default": "zlib",
    "name": "compression",
    "repo": "zlib"
   }
  ]
  $ hg debugupgraderepo
  (no feature deficiencies found in existing repository)
  performing an upgrade with "--run" will make the following changes:
  
  requirements
     preserved: dotencode, fncache, generaldelta, revlogv1, store
  
  additional optimizations are available by specifying "--optimize <name>":
  
  redeltaparent
     deltas within internal storage will be recalculated to choose an optimal base revision where this was not already done; the size of the repository may shrink and various operations may become faster; the first time this optimization is performed could slow down upgrade execution considerably; subsequent invocations should not run noticeably slower
  
  redeltamultibase
     deltas within internal storage will be recalculated against multiple base revision and the smallest difference will be used; the size of the repository may shrink significantly when there are many merges; this optimization will slow down execution in proportion to the number of merges in the repository and the amount of files in the repository; this slow down should not be significant unless there are tens of thousands of files and thousands of merges
  
  redeltaall
     deltas within internal storage will always be recalculated without reusing prior deltas; this will likely make execution run several times slower; this optimization is typically not needed
  
  redeltafulladd
     every revision will be re-added as if it was new content. It will go through the full storage mechanism giving extensions a chance to process it (eg. lfs). This is similar to "redeltaall" but even slower since more logic is involved.
  

--optimize can be used to add optimizations

  $ hg debugupgrade --optimize redeltaparent
  (no feature deficiencies found in existing repository)
  performing an upgrade with "--run" will make the following changes:
  
  requirements
     preserved: dotencode, fncache, generaldelta, revlogv1, store
  
  redeltaparent
     deltas within internal storage will choose a new base revision if needed
  
  additional optimizations are available by specifying "--optimize <name>":
  
  redeltamultibase
     deltas within internal storage will be recalculated against multiple base revision and the smallest difference will be used; the size of the repository may shrink significantly when there are many merges; this optimization will slow down execution in proportion to the number of merges in the repository and the amount of files in the repository; this slow down should not be significant unless there are tens of thousands of files and thousands of merges
  
  redeltaall
     deltas within internal storage will always be recalculated without reusing prior deltas; this will likely make execution run several times slower; this optimization is typically not needed
  
  redeltafulladd
     every revision will be re-added as if it was new content. It will go through the full storage mechanism giving extensions a chance to process it (eg. lfs). This is similar to "redeltaall" but even slower since more logic is involved.
  

Various sub-optimal detections work

  $ cat > .hg/requires << EOF
  > revlogv1
  > store
  > EOF

  $ hg debugformat
  format-variant repo
  fncache:         no
  dotencode:       no
  generaldelta:    no
  plain-cl-delta: yes
  compression:    zlib
  $ hg debugformat --verbose
  format-variant repo config default
  fncache:         no    yes     yes
  dotencode:       no    yes     yes
  generaldelta:    no    yes     yes
  plain-cl-delta: yes    yes     yes
  compression:    zlib   zlib    zlib
  $ hg debugformat --verbose --config format.usegeneraldelta=no
  format-variant repo config default
  fncache:         no    yes     yes
  dotencode:       no    yes     yes
  generaldelta:    no     no     yes
  plain-cl-delta: yes    yes     yes
  compression:    zlib   zlib    zlib
  $ hg debugformat --verbose --config format.usegeneraldelta=no --color=debug
  format-variant repo config default
  [formatvariant.name.mismatchconfig|fncache:       ][formatvariant.repo.mismatchconfig|  no][formatvariant.config.default|    yes][formatvariant.default|     yes]
  [formatvariant.name.mismatchconfig|dotencode:     ][formatvariant.repo.mismatchconfig|  no][formatvariant.config.default|    yes][formatvariant.default|     yes]
  [formatvariant.name.mismatchdefault|generaldelta:  ][formatvariant.repo.mismatchdefault|  no][formatvariant.config.special|     no][formatvariant.default|     yes]
  [formatvariant.name.uptodate|plain-cl-delta:][formatvariant.repo.uptodate| yes][formatvariant.config.default|    yes][formatvariant.default|     yes]
  [formatvariant.name.uptodate|compression:   ][formatvariant.repo.uptodate| zlib][formatvariant.config.default|   zlib][formatvariant.default|    zlib]
  $ hg debugupgraderepo
  repository lacks features recommended by current config options:
  
  fncache
     long and reserved filenames may not work correctly; repository performance is sub-optimal
  
  dotencode
     storage of filenames beginning with a period or space may not work correctly
  
  generaldelta
     deltas within internal storage are unable to choose optimal revisions; repository is larger and slower than it could be; interaction with other repositories may require extra network and CPU resources, making "hg push" and "hg pull" slower
  
  
  performing an upgrade with "--run" will make the following changes:
  
  requirements
     preserved: revlogv1, store
     added: dotencode, fncache, generaldelta
  
  fncache
     repository will be more resilient to storing certain paths and performance of certain operations should be improved
  
  dotencode
     repository will be better able to store files beginning with a space or period
  
  generaldelta
     repository storage will be able to create optimal deltas; new repository data will be smaller and read times should decrease; interacting with other repositories using this storage model should require less network and CPU resources, making "hg push" and "hg pull" faster
  
  additional optimizations are available by specifying "--optimize <name>":
  
  redeltaparent
     deltas within internal storage will be recalculated to choose an optimal base revision where this was not already done; the size of the repository may shrink and various operations may become faster; the first time this optimization is performed could slow down upgrade execution considerably; subsequent invocations should not run noticeably slower
  
  redeltamultibase
     deltas within internal storage will be recalculated against multiple base revision and the smallest difference will be used; the size of the repository may shrink significantly when there are many merges; this optimization will slow down execution in proportion to the number of merges in the repository and the amount of files in the repository; this slow down should not be significant unless there are tens of thousands of files and thousands of merges
  
  redeltaall
     deltas within internal storage will always be recalculated without reusing prior deltas; this will likely make execution run several times slower; this optimization is typically not needed
  
  redeltafulladd
     every revision will be re-added as if it was new content. It will go through the full storage mechanism giving extensions a chance to process it (eg. lfs). This is similar to "redeltaall" but even slower since more logic is involved.
  

  $ hg --config format.dotencode=false debugupgraderepo
  repository lacks features recommended by current config options:
  
  fncache
     long and reserved filenames may not work correctly; repository performance is sub-optimal
  
  generaldelta
     deltas within internal storage are unable to choose optimal revisions; repository is larger and slower than it could be; interaction with other repositories may require extra network and CPU resources, making "hg push" and "hg pull" slower
  
  repository lacks features used by the default config options:
  
  dotencode
     storage of filenames beginning with a period or space may not work correctly
  
  
  performing an upgrade with "--run" will make the following changes:
  
  requirements
     preserved: revlogv1, store
     added: fncache, generaldelta
  
  fncache
     repository will be more resilient to storing certain paths and performance of certain operations should be improved
  
  generaldelta
     repository storage will be able to create optimal deltas; new repository data will be smaller and read times should decrease; interacting with other repositories using this storage model should require less network and CPU resources, making "hg push" and "hg pull" faster
  
  additional optimizations are available by specifying "--optimize <name>":
  
  redeltaparent
     deltas within internal storage will be recalculated to choose an optimal base revision where this was not already done; the size of the repository may shrink and various operations may become faster; the first time this optimization is performed could slow down upgrade execution considerably; subsequent invocations should not run noticeably slower
  
  redeltamultibase
     deltas within internal storage will be recalculated against multiple base revision and the smallest difference will be used; the size of the repository may shrink significantly when there are many merges; this optimization will slow down execution in proportion to the number of merges in the repository and the amount of files in the repository; this slow down should not be significant unless there are tens of thousands of files and thousands of merges
  
  redeltaall
     deltas within internal storage will always be recalculated without reusing prior deltas; this will likely make execution run several times slower; this optimization is typically not needed
  
  redeltafulladd
     every revision will be re-added as if it was new content. It will go through the full storage mechanism giving extensions a chance to process it (eg. lfs). This is similar to "redeltaall" but even slower since more logic is involved.
  

  $ cd ..

Upgrading a repository that is already modern essentially no-ops

  $ hg init modern
  $ hg -R modern debugupgraderepo --run
  upgrade will perform the following actions:
  
  requirements
     preserved: dotencode, fncache, generaldelta, revlogv1, store
  
  beginning upgrade...
  repository locked and read-only
  creating temporary repository to stage migrated data: $TESTTMP/modern/.hg/upgrade.* (glob)
  (it is safe to interrupt this process any time before data migration completes)
  copying requires
  data fully migrated to temporary repository
  marking source repository as being upgraded; clients will be unable to read from repository
  starting in-place swap of repository data
  replaced files will be backed up at $TESTTMP/modern/.hg/upgradebackup.* (glob)
  replacing store...
  store replacement complete; repository was inconsistent for *s (glob)
  finalizing requirements file and making repository readable again
  removing temporary repository $TESTTMP/modern/.hg/upgrade.* (glob)
  copy of old repository backed up at $TESTTMP/modern/.hg/upgradebackup.* (glob)
  the old repository will not be deleted; remove it to free up disk space once the upgraded repository is verified

Upgrading a repository to generaldelta works

  $ hg --config format.usegeneraldelta=false init upgradegd
  $ cd upgradegd
  $ touch f0
  $ hg -q commit -A -m initial
  $ touch f1
  $ hg -q commit -A -m 'add f1'
  $ hg -q up -r 0
  $ touch f2
  $ hg -q commit -A -m 'add f2'

  $ hg debugupgraderepo --run
  upgrade will perform the following actions:
  
  requirements
     preserved: dotencode, fncache, revlogv1, store
     added: generaldelta
  
  generaldelta
     repository storage will be able to create optimal deltas; new repository data will be smaller and read times should decrease; interacting with other repositories using this storage model should require less network and CPU resources, making "hg push" and "hg pull" faster
  
  beginning upgrade...
  repository locked and read-only
  creating temporary repository to stage migrated data: $TESTTMP/upgradegd/.hg/upgrade.* (glob)
  (it is safe to interrupt this process any time before data migration completes)
  migrating 9 total revisions (3 in filelogs, 3 in manifests, 3 in changelog)
  migrating 341 bytes in store; 401 bytes tracked data
  migrating 3 filelogs containing 3 revisions (0 bytes in store; 0 bytes tracked data)
  finished migrating 3 filelog revisions across 3 filelogs; change in size: 0 bytes
  migrating 1 manifests containing 3 revisions (157 bytes in store; 220 bytes tracked data)
  finished migrating 3 manifest revisions across 1 manifests; change in size: 0 bytes
  migrating changelog containing 3 revisions (184 bytes in store; 181 bytes tracked data)
  finished migrating 3 changelog revisions; change in size: 0 bytes
  finished migrating 9 total revisions; total change in store size: 0 bytes
  copying phaseroots
  copying requires
  data fully migrated to temporary repository
  marking source repository as being upgraded; clients will be unable to read from repository
  starting in-place swap of repository data
  replaced files will be backed up at $TESTTMP/upgradegd/.hg/upgradebackup.* (glob)
  replacing store...
  store replacement complete; repository was inconsistent for *s (glob)
  finalizing requirements file and making repository readable again
  removing temporary repository $TESTTMP/upgradegd/.hg/upgrade.* (glob)
  copy of old repository backed up at $TESTTMP/upgradegd/.hg/upgradebackup.* (glob)
  the old repository will not be deleted; remove it to free up disk space once the upgraded repository is verified

Original requirements backed up

  $ cat .hg/upgradebackup.*/requires
  dotencode
  fncache
  revlogv1
  store

generaldelta added to original requirements files

  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  revlogv1
  store

store directory has files we expect

  $ ls .hg/store
  00changelog.d
  00changelog.i
  00manifest.i
  data
  fncache
  phaseroots
  requires
  undo
  undo.backupfiles
  undo.phaseroots

manifest should be generaldelta

  $ hg debugrevlog -m | grep flags
  flags  : inline, generaldelta

verify should be happy

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 3 total revisions

old store should be backed up

  $ ls .hg/upgradebackup.*/store
  00changelog.d
  00changelog.i
  00manifest.i
  data
  fncache
  phaseroots
  requires
  undo
  undo.backup.fncache
  undo.backupfiles
  undo.phaseroots

  $ cd ..

store files with special filenames aren't encoded during copy

  $ hg init store-filenames
  $ cd store-filenames
  $ touch foo
  $ hg -q commit -A -m initial
  $ touch .hg/store/.XX_special_filename

  $ hg debugupgraderepo --run
  upgrade will perform the following actions:
  
  requirements
     preserved: dotencode, fncache, generaldelta, revlogv1, store
  
  beginning upgrade...
  repository locked and read-only
  creating temporary repository to stage migrated data: $TESTTMP/store-filenames/.hg/upgrade.* (glob)
  (it is safe to interrupt this process any time before data migration completes)
  migrating 3 total revisions (1 in filelogs, 1 in manifests, 1 in changelog)
  migrating 109 bytes in store; 107 bytes tracked data
  migrating 1 filelogs containing 1 revisions (0 bytes in store; 0 bytes tracked data)
  finished migrating 1 filelog revisions across 1 filelogs; change in size: 0 bytes
  migrating 1 manifests containing 1 revisions (46 bytes in store; 45 bytes tracked data)
  finished migrating 1 manifest revisions across 1 manifests; change in size: 0 bytes
  migrating changelog containing 1 revisions (63 bytes in store; 62 bytes tracked data)
  finished migrating 1 changelog revisions; change in size: 0 bytes
  finished migrating 3 total revisions; total change in store size: 0 bytes
  copying .XX_special_filename
  copying phaseroots
  copying requires
  data fully migrated to temporary repository
  marking source repository as being upgraded; clients will be unable to read from repository
  starting in-place swap of repository data
  replaced files will be backed up at $TESTTMP/store-filenames/.hg/upgradebackup.* (glob)
  replacing store...
  store replacement complete; repository was inconsistent for *s (glob)
  finalizing requirements file and making repository readable again
  removing temporary repository $TESTTMP/store-filenames/.hg/upgrade.* (glob)
  copy of old repository backed up at $TESTTMP/store-filenames/.hg/upgradebackup.* (glob)
  the old repository will not be deleted; remove it to free up disk space once the upgraded repository is verified
  $ hg debugupgraderepo --run --optimize redeltafulladd
  upgrade will perform the following actions:
  
  requirements
     preserved: dotencode, fncache, generaldelta, revlogv1, store
  
  redeltafulladd
     each revision will be added as new content to the internal storage; this will likely drastically slow down execution time, but some extensions might need it
  
  beginning upgrade...
  repository locked and read-only
  creating temporary repository to stage migrated data: $TESTTMP/store-filenames/.hg/upgrade.* (glob)
  (it is safe to interrupt this process any time before data migration completes)
  migrating 3 total revisions (1 in filelogs, 1 in manifests, 1 in changelog)
  migrating 109 bytes in store; 107 bytes tracked data
  migrating 1 filelogs containing 1 revisions (0 bytes in store; 0 bytes tracked data)
  finished migrating 1 filelog revisions across 1 filelogs; change in size: 0 bytes
  migrating 1 manifests containing 1 revisions (46 bytes in store; 45 bytes tracked data)
  finished migrating 1 manifest revisions across 1 manifests; change in size: 0 bytes
  migrating changelog containing 1 revisions (63 bytes in store; 62 bytes tracked data)
  finished migrating 1 changelog revisions; change in size: 0 bytes
  finished migrating 3 total revisions; total change in store size: 0 bytes
  copying .XX_special_filename
  copying phaseroots
  copying requires
  data fully migrated to temporary repository
  marking source repository as being upgraded; clients will be unable to read from repository
  starting in-place swap of repository data
  replaced files will be backed up at $TESTTMP/store-filenames/.hg/upgradebackup.* (glob)
  replacing store...
  store replacement complete; repository was inconsistent for *s (glob)
  finalizing requirements file and making repository readable again
  removing temporary repository $TESTTMP/store-filenames/.hg/upgrade.* (glob)
  copy of old repository backed up at $TESTTMP/store-filenames/.hg/upgradebackup.* (glob)
  the old repository will not be deleted; remove it to free up disk space once the upgraded repository is verified

  $ cd ..

repository config is taken in account
-------------------------------------

  $ cat << EOF >> $HGRCPATH
  > [format]
  > maxchainlen = 1
  > EOF

  $ hg init localconfig
  $ cd localconfig
  $ cat << EOF > file
  > some content
  > with some length
  > to make sure we get a delta
  > after changes
  > very long
  > very long
  > very long
  > very long
  > very long
  > very long
  > very long
  > very long
  > very long
  > very long
  > very long
  > EOF
  $ hg -q commit -A -m A
  $ echo "new line" >> file
  $ hg -q commit -m B
  $ echo "new line" >> file
  $ hg -q commit -m C

  $ cat << EOF >> .hg/hgrc
  > [format]
  > maxchainlen = 9001
  > EOF
  $ hg config format
  format.dirstate=0
  format.maxchainlen=9001
  $ hg debugindex file
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      77     -1       0 bcc1d3df78b2 000000000000 000000000000
       1        77      21      0       1 af3e29f7a72e bcc1d3df78b2 000000000000
       2        98      84     -1       2 8daf79c5522b af3e29f7a72e 000000000000

  $ hg debugupgraderepo --run --optimize redeltaall
  upgrade will perform the following actions:
  
  requirements
     preserved: dotencode, fncache, generaldelta, revlogv1, store
  
  redeltaall
     deltas within internal storage will be fully recomputed; this will likely drastically slow down execution time
  
  beginning upgrade...
  repository locked and read-only
  creating temporary repository to stage migrated data: $TESTTMP/localconfig/.hg/upgrade.* (glob)
  (it is safe to interrupt this process any time before data migration completes)
  migrating 9 total revisions (3 in filelogs, 3 in manifests, 3 in changelog)
  migrating 497 bytes in store; 882 bytes tracked data
  migrating 1 filelogs containing 3 revisions (182 bytes in store; 573 bytes tracked data)
  finished migrating 3 filelog revisions across 1 filelogs; change in size: -63 bytes
  migrating 1 manifests containing 3 revisions (141 bytes in store; 138 bytes tracked data)
  finished migrating 3 manifest revisions across 1 manifests; change in size: 0 bytes
  migrating changelog containing 3 revisions (174 bytes in store; 171 bytes tracked data)
  finished migrating 3 changelog revisions; change in size: 0 bytes
  finished migrating 9 total revisions; total change in store size: -63 bytes
  copying phaseroots
  copying requires
  data fully migrated to temporary repository
  marking source repository as being upgraded; clients will be unable to read from repository
  starting in-place swap of repository data
  replaced files will be backed up at $TESTTMP/localconfig/.hg/upgradebackup.* (glob)
  replacing store...
  store replacement complete; repository was inconsistent for *s (glob)
  finalizing requirements file and making repository readable again
  removing temporary repository $TESTTMP/localconfig/.hg/upgrade.* (glob)
  copy of old repository backed up at $TESTTMP/localconfig/.hg/upgradebackup.* (glob)
  the old repository will not be deleted; remove it to free up disk space once the upgraded repository is verified
  $ hg debugindex file
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      77     -1       0 bcc1d3df78b2 000000000000 000000000000
       1        77      21      0       1 af3e29f7a72e bcc1d3df78b2 000000000000
       2        98      21      1       2 8daf79c5522b af3e29f7a72e 000000000000
  $ cd ..

  $ cat << EOF >> $HGRCPATH
  > [format]
  > maxchainlen = 9001
  > EOF

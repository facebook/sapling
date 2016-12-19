  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > share =
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

Do not yet support upgrading manifestv2 and treemanifest repos

  $ hg --config experimental.manifestv2=true init manifestv2
  $ hg -R manifestv2 debugupgraderepo
  abort: cannot upgrade repository; unsupported source requirement: manifestv2
  [255]

  $ hg --config experimental.treemanifest=true init treemanifest
  $ hg -R treemanifest debugupgraderepo
  abort: cannot upgrade repository; unsupported source requirement: treemanifest
  [255]

Cannot add manifestv2 or treemanifest requirement during upgrade

  $ hg init disallowaddedreq
  $ hg -R disallowaddedreq --config experimental.manifestv2=true --config experimental.treemanifest=true debugupgraderepo
  abort: cannot upgrade repository; do not support adding requirement: manifestv2, treemanifest
  [255]

An upgrade of a repository created with recommended settings only suggests optimizations

  $ hg init empty
  $ cd empty
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
  

Various sub-optimal detections work

  $ cat > .hg/requires << EOF
  > revlogv1
  > store
  > EOF

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
  


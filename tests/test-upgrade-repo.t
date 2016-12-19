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

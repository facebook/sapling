==================================================
Test obsmarkers interaction with bundle and strip
==================================================

Setup a repository with various case
====================================

Config setup
------------

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > # simpler log output
  > logtemplate = "{node|short}: {desc}\n"
  > 
  > [experimental]
  > # enable evolution
  > evolution=true
  > 
  > # include obsmarkers in bundle
  > stabilization.bundle-obsmarker = yes
  > 
  > [extensions]
  > # needed for some tests
  > strip =
  > [defaults]
  > # we'll query many hidden changeset
  > debugobsolete = --hidden
  > EOF

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

  $ getid() {
  >    hg log --hidden --template '{node}\n' --rev "$1"
  > }

  $ mktestrepo () {
  >     [ -n "$1" ] || exit 1
  >     cd $TESTTMP
  >     hg init $1
  >     cd $1
  >     mkcommit ROOT
  > }

Function to compare the expected bundled obsmarkers with the actually bundled
obsmarkers. It also check the obsmarkers backed up during strip.

  $ testrevs () {
  >     revs="$1"
  >     testname=`basename \`pwd\``
  >     revsname=`hg --hidden log -T '-{desc}' --rev "${revs}"`
  >     prefix="${TESTTMP}/${testname}${revsname}"
  >     markersfile="${prefix}-relevant-markers.txt"
  >     exclufile="${prefix}-exclusive-markers.txt"
  >     bundlefile="${prefix}-bundle.hg"
  >     contentfile="${prefix}-bundle-markers.hg"
  >     stripcontentfile="${prefix}-bundle-markers.hg"
  >     hg debugobsolete --hidden --rev "${revs}" | sed 's/^/    /' > "${markersfile}"
  >     hg debugobsolete --hidden --rev "${revs}" --exclusive | sed 's/^/    /' > "${exclufile}"
  >     echo '### Matched revisions###'
  >     hg log --hidden --rev "${revs}" | sort
  >     echo '### Relevant markers ###'
  >     cat "${markersfile}"
  >     printf "# bundling: "
  >     hg bundle --hidden --base "parents(roots(${revs}))" --rev "${revs}" "${bundlefile}"
  >     hg debugbundle --part-type obsmarkers "${bundlefile}" | sed 1,3d > "${contentfile}"
  >     echo '### Bundled markers ###'
  >     cat "${contentfile}"
  >     echo '### diff <relevant> <bundled> ###'
  >     cmp "${markersfile}" "${contentfile}" || diff -u "${markersfile}" "${contentfile}"
  >     echo '#################################'
  >     echo '### Exclusive markers ###'
  >     cat "${exclufile}"
  >     # if the matched revs do not have children, we also check the result of strip
  >     children=`hg log --hidden --rev "((${revs})::) - (${revs})"`
  >     if [ -z "$children" ];
  >     then
  >         printf "# stripping: "
  >         prestripfile="${prefix}-pre-strip.txt"
  >         poststripfile="${prefix}-post-strip.txt"
  >         strippedfile="${prefix}-stripped-markers.txt"
  >         hg debugobsolete --hidden | sort | sed 's/^/    /' > "${prestripfile}"
  >         hg strip --hidden --rev "${revs}"
  >         hg debugobsolete --hidden | sort | sed 's/^/    /' > "${poststripfile}"
  >         hg debugbundle --part-type obsmarkers .hg/strip-backup/* | sed 1,3d > "${stripcontentfile}"
  >         echo '### Backup markers ###'
  >         cat "${stripcontentfile}"
  >         echo '### diff <relevant> <backed-up> ###'
  >         cmp "${markersfile}" "${stripcontentfile}" || diff -u "${markersfile}" "${stripcontentfile}"
  >         echo '#################################'
  >         cat "${prestripfile}" "${poststripfile}" | sort | uniq -u > "${strippedfile}"
  >         echo '### Stripped markers ###'
  >         cat "${strippedfile}"
  >         echo '### diff <exclusive> <stripped> ###'
  >         cmp "${exclufile}" "${strippedfile}" || diff -u "${exclufile}" "${strippedfile}"
  >         echo '#################################'
  >         # restore and clean up repo for the next test
  >         hg unbundle .hg/strip-backup/* | sed 's/^/# unbundling: /'
  >         # clean up directory for the next test
  >         rm .hg/strip-backup/*
  >     fi
  > }

root setup
-------------

simple chain
============

.    A0
.   ⇠ø⇠◔ A1
.    |/
.    ●

setup
-----

  $ mktestrepo simple-chain
  $ mkcommit 'C-A0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete `getid 'desc("C-A0")'` a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1
  obsoleted 1 changesets
  $ hg debugobsolete a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 `getid 'desc("C-A1")'`

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  | x  84fcb0dfe17b: C-A0
  |/
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ testrevs 'desc("C-A0")'
  ### Matched revisions###
  84fcb0dfe17b: C-A0
  ### Relevant markers ###
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
  # stripping: saved backup bundle to $TESTTMP/simple-chain/.hg/strip-backup/84fcb0dfe17b-6454bbdc-backup.hg (glob)
  ### Backup markers ###
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: (run 'hg heads' to see heads)

  $ testrevs 'desc("C-A1")'
  ### Matched revisions###
  cf2c22470d67: C-A1
  ### Relevant markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/simple-chain/.hg/strip-backup/cf2c22470d67-fa0f07b0-backup.hg (glob)
  ### Backup markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: 2 new obsolescence markers
  # unbundling: obsoleted 1 changesets
  # unbundling: new changesets cf2c22470d67
  # unbundling: (run 'hg heads' to see heads)

  $ testrevs 'desc("C-A")'
  ### Matched revisions###
  84fcb0dfe17b: C-A0
  cf2c22470d67: C-A1
  ### Relevant markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 2 changesets found
  ### Bundled markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/simple-chain/.hg/strip-backup/cf2c22470d67-fce4fc64-backup.hg (glob)
  ### Backup markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1 cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 2 changesets with 2 changes to 2 files (+1 heads)
  # unbundling: 3 new obsolescence markers
  # unbundling: new changesets cf2c22470d67
  # unbundling: (run 'hg heads' to see heads)

chain with prune children
=========================

.  ⇠⊗ B0
.   |
.  ⇠ø⇠◔ A1
.     |
.     ●

setup
-----

  $ mktestrepo prune
  $ mkcommit 'C-A0'
  $ mkcommit 'C-B0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete `getid 'desc("C-A0")'` `getid 'desc("C-A1")'`
  obsoleted 1 changesets
  $ hg debugobsolete --record-parents `getid 'desc("C-B0")'`
  obsoleted 1 changesets
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  | x  29f93b1df87b: C-B0
  | |
  | x  84fcb0dfe17b: C-A0
  |/
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ testrevs 'desc("C-A0")'
  ### Matched revisions###
  84fcb0dfe17b: C-A0
  ### Relevant markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###

(The strip markers is considered exclusive to the pruned changeset even if it
is also considered "relevant" to its parent. This allows to strip prune
markers. This avoid leaving prune markers from dead-end that could be
problematic)

  $ testrevs 'desc("C-B0")'
  ### Matched revisions###
  29f93b1df87b: C-B0
  ### Relevant markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/prune/.hg/strip-backup/29f93b1df87b-7fb32101-backup.hg (glob)
  ### Backup markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files
  # unbundling: 1 new obsolescence markers
  # unbundling: (run 'hg update' to get a working copy)

  $ testrevs 'desc("C-A1")'
  ### Matched revisions###
  cf2c22470d67: C-A1
  ### Relevant markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/prune/.hg/strip-backup/cf2c22470d67-fa0f07b0-backup.hg (glob)
  ### Backup markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: 1 new obsolescence markers
  # unbundling: obsoleted 1 changesets
  # unbundling: new changesets cf2c22470d67
  # unbundling: (run 'hg heads' to see heads)

bundling multiple revisions

  $ testrevs 'desc("C-A")'
  ### Matched revisions###
  84fcb0dfe17b: C-A0
  cf2c22470d67: C-A1
  ### Relevant markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 2 changesets found
  ### Bundled markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

  $ testrevs 'desc("C-")'
  ### Matched revisions###
  29f93b1df87b: C-B0
  84fcb0dfe17b: C-A0
  cf2c22470d67: C-A1
  ### Relevant markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 3 changesets found
  ### Bundled markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/prune/.hg/strip-backup/cf2c22470d67-884c33b0-backup.hg (glob)
  ### Backup markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 3 changesets with 3 changes to 3 files (+1 heads)
  # unbundling: 3 new obsolescence markers
  # unbundling: new changesets cf2c22470d67
  # unbundling: (run 'hg heads' to see heads)

chain with precursors also pruned
=================================

.   A0 (also pruned)
.  ⇠ø⇠◔ A1
.     |
.     ●

setup
-----

  $ mktestrepo prune-inline
  $ mkcommit 'C-A0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete --record-parents `getid 'desc("C-A0")'`
  obsoleted 1 changesets
  $ hg debugobsolete `getid 'desc("C-A0")'` `getid 'desc("C-A1")'`
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  | x  84fcb0dfe17b: C-A0
  |/
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ testrevs 'desc("C-A0")'
  ### Matched revisions###
  84fcb0dfe17b: C-A0
  ### Relevant markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
  # stripping: saved backup bundle to $TESTTMP/prune-inline/.hg/strip-backup/84fcb0dfe17b-6454bbdc-backup.hg (glob)
  ### Backup markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: (run 'hg heads' to see heads)

  $ testrevs 'desc("C-A1")'
  ### Matched revisions###
  cf2c22470d67: C-A1
  ### Relevant markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/prune-inline/.hg/strip-backup/cf2c22470d67-fa0f07b0-backup.hg (glob)
  ### Backup markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: 1 new obsolescence markers
  # unbundling: new changesets cf2c22470d67
  # unbundling: (run 'hg heads' to see heads)

  $ testrevs 'desc("C-A")'
  ### Matched revisions###
  84fcb0dfe17b: C-A0
  cf2c22470d67: C-A1
  ### Relevant markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 2 changesets found
  ### Bundled markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/prune-inline/.hg/strip-backup/cf2c22470d67-fce4fc64-backup.hg (glob)
  ### Backup markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 2 changesets with 2 changes to 2 files (+1 heads)
  # unbundling: 3 new obsolescence markers
  # unbundling: new changesets cf2c22470d67
  # unbundling: (run 'hg heads' to see heads)

chain with missing prune
========================

.   ⊗ B
.   |
.  ⇠◌⇠◔ A1
.   |
.   ●

setup
-----

  $ mktestrepo missing-prune
  $ mkcommit 'C-A0'
  $ mkcommit 'C-B0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete `getid 'desc("C-A0")'` `getid 'desc("C-A1")'`
  obsoleted 1 changesets
  $ hg debugobsolete --record-parents `getid 'desc("C-B0")'`
  obsoleted 1 changesets

(it is annoying to create prune with parent data without the changeset, so we strip it after the fact)

  $ hg strip --hidden --rev 'desc("C-A0")::' --no-backup --config devel.strip-obsmarkers=no

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ testrevs 'desc("C-A1")'
  ### Matched revisions###
  cf2c22470d67: C-A1
  ### Relevant markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/missing-prune/.hg/strip-backup/cf2c22470d67-fa0f07b0-backup.hg (glob)
  ### Backup markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      29f93b1df87baee1824e014080d8adf145f81783 0 {84fcb0dfe17b256ebae52e05572993b9194c018a} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files
  # unbundling: 3 new obsolescence markers
  # unbundling: new changesets cf2c22470d67
  # unbundling: (run 'hg update' to get a working copy)

chain with precursors also pruned
=================================

.   A0 (also pruned)
.  ⇠◌⇠◔ A1
.     |
.     ●

setup
-----

  $ mktestrepo prune-inline-missing
  $ mkcommit 'C-A0'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-A1'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A0")'`
  $ hg debugobsolete --record-parents `getid 'desc("C-A0")'`
  obsoleted 1 changesets
  $ hg debugobsolete `getid 'desc("C-A0")'` `getid 'desc("C-A1")'`

(it is annoying to create prune with parent data without the changeset, so we strip it after the fact)

  $ hg strip --hidden --rev 'desc("C-A0")::' --no-backup --config devel.strip-obsmarkers=no

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  cf2c22470d67: C-A1
  |
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ testrevs 'desc("C-A1")'
  ### Matched revisions###
  cf2c22470d67: C-A1
  ### Relevant markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/prune-inline-missing/.hg/strip-backup/cf2c22470d67-fa0f07b0-backup.hg (glob)
  ### Backup markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      84fcb0dfe17b256ebae52e05572993b9194c018a 0 {ea207398892eb49e06441f10dda2a731f0450f20} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      84fcb0dfe17b256ebae52e05572993b9194c018a cf2c22470d67233004e934a31184ac2b35389914 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 84fcb0dfe17b256ebae52e05572993b9194c018a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files
  # unbundling: 3 new obsolescence markers
  # unbundling: new changesets cf2c22470d67
  # unbundling: (run 'hg update' to get a working copy)

Chain with fold and split
=========================

setup
-----

  $ mktestrepo split-fold
  $ mkcommit 'C-A'
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-B'
  created new head
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-C'
  created new head
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-D'
  created new head
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit 'C-E'
  created new head
  $ hg debugobsolete a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 `getid 'desc("C-A")'`
  $ hg debugobsolete `getid 'desc("C-A")'` `getid 'desc("C-B")'` `getid 'desc("C-C")'` # record split
  obsoleted 1 changesets
  $ hg debugobsolete `getid 'desc("C-A")'` `getid 'desc("C-D")'` # other divergent
  $ hg debugobsolete `getid 'desc("C-A")'` b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0
  $ hg debugobsolete b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 `getid 'desc("C-E")'`
  $ hg debugobsolete `getid 'desc("C-B")'` `getid 'desc("C-E")'`
  obsoleted 1 changesets
  $ hg debugobsolete `getid 'desc("C-C")'` `getid 'desc("C-E")'`
  obsoleted 1 changesets
  $ hg debugobsolete `getid 'desc("C-D")'` `getid 'desc("C-E")'`
  obsoleted 1 changesets
  $ hg debugobsolete c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 `getid 'desc("C-E")'`

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log --hidden -G
  o  2f20ff6509f0: C-E
  |
  | x  06dc9da25ef0: C-D
  |/
  | x  27ec657ca21d: C-C
  |/
  | x  a9b9da38ed96: C-B
  |/
  | x  9ac430e15fca: C-A
  |/
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------

  $ testrevs 'desc("C-A")'
  ### Matched revisions###
  9ac430e15fca: C-A
  ### Relevant markers ###
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/9ac430e15fca-81204eba-backup.hg (glob)
  ### Backup markers ###
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: (run 'hg heads' to see heads)

  $ testrevs 'desc("C-B")'
  ### Matched revisions###
  a9b9da38ed96: C-B
  ### Relevant markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/a9b9da38ed96-7465d6e9-backup.hg (glob)
  ### Backup markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: (run 'hg heads' to see heads)

  $ testrevs 'desc("C-C")'
  ### Matched revisions###
  27ec657ca21d: C-C
  ### Relevant markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/27ec657ca21d-d5dd1c7c-backup.hg (glob)
  ### Backup markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: (run 'hg heads' to see heads)

  $ testrevs 'desc("C-D")'
  ### Matched revisions###
  06dc9da25ef0: C-D
  ### Relevant markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/06dc9da25ef0-9b1c0a91-backup.hg (glob)
  ### Backup markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: (run 'hg heads' to see heads)

  $ testrevs 'desc("C-E")'
  ### Matched revisions###
  2f20ff6509f0: C-E
  ### Relevant markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/2f20ff6509f0-8adeb22d-backup.hg (glob)
  ### Backup markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files (+1 heads)
  # unbundling: 6 new obsolescence markers
  # unbundling: obsoleted 3 changesets
  # unbundling: new changesets 2f20ff6509f0
  # unbundling: (run 'hg heads' to see heads)

Bundle multiple revisions

* each part of the split

  $ testrevs 'desc("C-B") + desc("C-C")'
  ### Matched revisions###
  27ec657ca21d: C-C
  a9b9da38ed96: C-B
  ### Relevant markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 2 changesets found
  ### Bundled markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/a9b9da38ed96-0daf625a-backup.hg (glob)
  ### Backup markers ###
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 2 changesets with 2 changes to 2 files (+2 heads)
  # unbundling: (run 'hg heads' to see heads)

* top one and other divergent

  $ testrevs 'desc("C-E") + desc("C-D")'
  ### Matched revisions###
  06dc9da25ef0: C-D
  2f20ff6509f0: C-E
  ### Relevant markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 2 changesets found
  ### Bundled markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/2f20ff6509f0-bf1b80f4-backup.hg (glob)
  ### Backup markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 2 changesets with 2 changes to 2 files (+2 heads)
  # unbundling: 7 new obsolescence markers
  # unbundling: obsoleted 2 changesets
  # unbundling: new changesets 2f20ff6509f0
  # unbundling: (run 'hg heads' to see heads)

* top one and initial precursors

  $ testrevs 'desc("C-E") + desc("C-A")'
  ### Matched revisions###
  2f20ff6509f0: C-E
  9ac430e15fca: C-A
  ### Relevant markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 2 changesets found
  ### Bundled markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/9ac430e15fca-36b6476a-backup.hg (glob)
  ### Backup markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 2 changesets with 2 changes to 2 files (+2 heads)
  # unbundling: 6 new obsolescence markers
  # unbundling: obsoleted 3 changesets
  # unbundling: new changesets 2f20ff6509f0
  # unbundling: (run 'hg heads' to see heads)

* top one and one of the split

  $ testrevs 'desc("C-E") + desc("C-C")'
  ### Matched revisions###
  27ec657ca21d: C-C
  2f20ff6509f0: C-E
  ### Relevant markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 2 changesets found
  ### Bundled markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/2f20ff6509f0-5fdfcd7d-backup.hg (glob)
  ### Backup markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 2 changesets with 2 changes to 2 files (+2 heads)
  # unbundling: 7 new obsolescence markers
  # unbundling: obsoleted 2 changesets
  # unbundling: new changesets 2f20ff6509f0
  # unbundling: (run 'hg heads' to see heads)

* all

  $ testrevs 'desc("C-")'
  ### Matched revisions###
  06dc9da25ef0: C-D
  27ec657ca21d: C-C
  2f20ff6509f0: C-E
  9ac430e15fca: C-A
  a9b9da38ed96: C-B
  ### Relevant markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 5 changesets found
  ### Bundled markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/split-fold/.hg/strip-backup/a9b9da38ed96-eeb4258f-backup.hg (glob)
  ### Backup markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      27ec657ca21dd27c36c99fa75586f72ff0d442f1 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 06dc9da25ef03e1ff7864dded5fcba42eff2a3f0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c a9b9da38ed96f8c6c14f429441f625a344eb4696 27ec657ca21dd27c36c99fa75586f72ff0d442f1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      9ac430e15fca923b0ba027ca85d4d75c5c9cb73c b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0 9ac430e15fca923b0ba027ca85d4d75c5c9cb73c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      a9b9da38ed96f8c6c14f429441f625a344eb4696 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
      c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0 2f20ff6509f0e013e90c5c8efd996131c918b0ca 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 5 changesets with 5 changes to 5 files (+4 heads)
  # unbundling: 9 new obsolescence markers
  # unbundling: new changesets 2f20ff6509f0
  # unbundling: (run 'hg heads' to see heads)

changeset pruned on its own
===========================

. ⊗ B
. |
. ◕ A
. |
. ●

setup
-----

  $ mktestrepo lonely-prune
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit 'C-A'
  $ mkcommit 'C-B'
  $ hg debugobsolete --record-parent `getid 'desc("C-B")'`
  obsoleted 1 changesets

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log --hidden -G
  x  cefb651fc2fd: C-B
  |
  o  9ac430e15fca: C-A
  |
  @  ea207398892e: ROOT
  
  $ hg debugobsolete
  cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual testing
--------------
  $ testrevs 'desc("C-A")'
  ### Matched revisions###
  9ac430e15fca: C-A
  ### Relevant markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
  $ testrevs 'desc("C-B")'
  ### Matched revisions###
  cefb651fc2fd: C-B
  ### Relevant markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 1 changesets found
  ### Bundled markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/lonely-prune/.hg/strip-backup/cefb651fc2fd-345c8dfa-backup.hg (glob)
  ### Backup markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 1 changesets with 1 changes to 1 files
  # unbundling: 1 new obsolescence markers
  # unbundling: (run 'hg update' to get a working copy)
  $ testrevs 'desc("C-")'
  ### Matched revisions###
  9ac430e15fca: C-A
  cefb651fc2fd: C-B
  ### Relevant markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # bundling: 2 changesets found
  ### Bundled markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <bundled> ###
  #################################
  ### Exclusive markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # stripping: saved backup bundle to $TESTTMP/lonely-prune/.hg/strip-backup/9ac430e15fca-b9855b02-backup.hg (glob)
  ### Backup markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <relevant> <backed-up> ###
  #################################
  ### Stripped markers ###
      cefb651fc2fdc7bb75e588781de5e432c134e8a5 0 {9ac430e15fca923b0ba027ca85d4d75c5c9cb73c} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  ### diff <exclusive> <stripped> ###
  #################################
  # unbundling: adding changesets
  # unbundling: adding manifests
  # unbundling: adding file changes
  # unbundling: added 2 changesets with 2 changes to 2 files
  # unbundling: 1 new obsolescence markers
  # unbundling: new changesets 9ac430e15fca
  # unbundling: (run 'hg update' to get a working copy)

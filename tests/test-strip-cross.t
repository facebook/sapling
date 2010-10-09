test stripping of filelogs where the linkrev doesn't always increase

  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'hgext.mq =' >> $HGRCPATH
  $ hg init orig
  $ cd orig
  $ commit()
  > {
  >     hg up -qC null
  >     count=1
  >     for i in "$@"; do
  >         for f in $i; do
  >             echo $count > $f
  >         done
  >         count=`expr $count + 1`
  >     done
  >     hg commit -qAm "$*"
  > }

2 1 0 2 0 1 2

  $ commit '201 210'
  $ commit '102 120' '210'
  $ commit '021'
  $ commit '201' '021 120'
  $ commit '012 021' '102 201' '120 210'
  $ commit 'manifest-file'
  $ commit '102 120' '012 210' '021 201'
  $ commit '201 210' '021 120' '012 102'
  $ HGUSER=another-user; export HGUSER
  $ commit 'manifest-file'
  $ commit '012' 'manifest-file'
  $ cd ..
  $ hg clone -q -U -r -1 -r -2 -r -3 -r -4 -r -6 orig crossed
  $ for i in crossed/.hg/store/00manifest.i crossed/.hg/store/data/*.i; do
  >     echo $i
  >     hg debugindex $i
  >     echo
  > done
  crossed/.hg/store/00manifest.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0     112      0       0 6f105cbb914d 000000000000 000000000000
       1       112      56      1       3 1b55917b3699 000000000000 000000000000
       2       168     123      1       1 8f3d04e263e5 000000000000 000000000000
       3       291     122      1       2 f0ef8726ac4f 000000000000 000000000000
       4       413      87      4       4 0b76e38b4070 000000000000 000000000000
  
  crossed/.hg/store/data/012.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       0 b8e02f643373 000000000000 000000000000
       1         3       3      1       1 5d9299349fc0 000000000000 000000000000
       2         6       3      2       2 2661d26c6496 000000000000 000000000000
  
  crossed/.hg/store/data/021.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       0 b8e02f643373 000000000000 000000000000
       1         3       3      1       2 5d9299349fc0 000000000000 000000000000
       2         6       3      2       1 2661d26c6496 000000000000 000000000000
  
  crossed/.hg/store/data/102.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       1 b8e02f643373 000000000000 000000000000
       1         3       3      1       0 5d9299349fc0 000000000000 000000000000
       2         6       3      2       2 2661d26c6496 000000000000 000000000000
  
  crossed/.hg/store/data/120.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       1 b8e02f643373 000000000000 000000000000
       1         3       3      1       2 5d9299349fc0 000000000000 000000000000
       2         6       3      2       0 2661d26c6496 000000000000 000000000000
  
  crossed/.hg/store/data/201.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       2 b8e02f643373 000000000000 000000000000
       1         3       3      1       0 5d9299349fc0 000000000000 000000000000
       2         6       3      2       1 2661d26c6496 000000000000 000000000000
  
  crossed/.hg/store/data/210.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       2 b8e02f643373 000000000000 000000000000
       1         3       3      1       1 5d9299349fc0 000000000000 000000000000
       2         6       3      2       0 2661d26c6496 000000000000 000000000000
  
  crossed/.hg/store/data/manifest-file.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       3 b8e02f643373 000000000000 000000000000
       1         3       3      1       4 5d9299349fc0 000000000000 000000000000
  
  $ for i in 0 1 2 3 4; do
  >     hg clone -q -U --pull crossed $i
  >     echo "% Trying to strip revision $i"
  >     hg --cwd $i strip $i
  >     echo "% Verifying"
  >     hg --cwd $i verify
  >     echo
  > done
  % Trying to strip revision 0
  saved backup bundle to $TESTTMP/0/.hg/strip-backup/*-backup.hg (glob)
  % Verifying
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  7 files, 4 changesets, 15 total revisions
  
  % Trying to strip revision 1
  saved backup bundle to $TESTTMP/1/.hg/strip-backup/*-backup.hg (glob)
  % Verifying
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  7 files, 4 changesets, 14 total revisions
  
  % Trying to strip revision 2
  saved backup bundle to $TESTTMP/2/.hg/strip-backup/*-backup.hg (glob)
  % Verifying
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  7 files, 4 changesets, 14 total revisions
  
  % Trying to strip revision 3
  saved backup bundle to $TESTTMP/3/.hg/strip-backup/*-backup.hg (glob)
  % Verifying
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  7 files, 4 changesets, 19 total revisions
  
  % Trying to strip revision 4
  saved backup bundle to $TESTTMP/4/.hg/strip-backup/*-backup.hg (glob)
  % Verifying
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  7 files, 4 changesets, 19 total revisions
  

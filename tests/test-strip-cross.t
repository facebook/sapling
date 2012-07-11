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
  $ cd crossed
  $ hg debugindex --manifest
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0     112  .....       0 6f105cbb914d 000000000000 000000000000 (re)
       1       112      56  .....       3 1b55917b3699 000000000000 000000000000 (re)
       2       168     123  .....       1 8f3d04e263e5 000000000000 000000000000 (re)
       3       291     122  .....       2 f0ef8726ac4f 000000000000 000000000000 (re)
       4       413      87  .....       4 0b76e38b4070 000000000000 000000000000 (re)

  $ for i in 012 021 102 120 201 210 manifest-file; do
  >     echo $i
  >     hg debugindex $i
  >     echo
  > done
  012
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       0 b8e02f643373 000000000000 000000000000 (re)
       1         3       3  .....       1 5d9299349fc0 000000000000 000000000000 (re)
       2         6       3  .....       2 2661d26c6496 000000000000 000000000000 (re)
  
  021
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       0 b8e02f643373 000000000000 000000000000 (re)
       1         3       3  .....       2 5d9299349fc0 000000000000 000000000000 (re)
       2         6       3  .....       1 2661d26c6496 000000000000 000000000000 (re)
  
  102
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       1 b8e02f643373 000000000000 000000000000 (re)
       1         3       3  .....       0 5d9299349fc0 000000000000 000000000000 (re)
       2         6       3  .....       2 2661d26c6496 000000000000 000000000000 (re)
  
  120
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       1 b8e02f643373 000000000000 000000000000 (re)
       1         3       3  .....       2 5d9299349fc0 000000000000 000000000000 (re)
       2         6       3  .....       0 2661d26c6496 000000000000 000000000000 (re)
  
  201
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       2 b8e02f643373 000000000000 000000000000 (re)
       1         3       3  .....       0 5d9299349fc0 000000000000 000000000000 (re)
       2         6       3  .....       1 2661d26c6496 000000000000 000000000000 (re)
  
  210
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       2 b8e02f643373 000000000000 000000000000 (re)
       1         3       3  .....       1 5d9299349fc0 000000000000 000000000000 (re)
       2         6       3  .....       0 2661d26c6496 000000000000 000000000000 (re)
  
  manifest-file
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       3 b8e02f643373 000000000000 000000000000 (re)
       1         3       3  .....       4 5d9299349fc0 000000000000 000000000000 (re)
  
  $ cd ..
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
  

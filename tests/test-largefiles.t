  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > largefiles=
  > EOF

Create the repo with a couple of revisions of both large and normal
files.

  $ hg init a
  $ cd a
  $ mkdir sub
  $ echo normal1 > normal1
  $ echo normal2 > sub/normal2
  $ echo large1 > large1
  $ echo large2 > sub/large2
  $ hg add normal1 sub/normal2
  $ hg add --large large1 sub/large2
  $ hg commit -m "add files"
  $ echo normal11 > normal1
  $ echo normal22 > sub/normal2
  $ echo large11 > large1
  $ echo large22 > sub/large2
  $ hg commit -m "edit files"

Verify that committing new versions of largefiles results in correct
largefile contents, and also that non-largefiles are not affected
badly.

  $ cat normal1
  normal11
  $ cat large1
  large11
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22

Verify removing largefiles and normal files works on largefile repos.
 
  $ hg remove normal1 large1
  $ hg commit -m "remove files"
  $ ls
  sub

Test copying largefiles.

  $ hg cp sub/normal2 normal1
  $ hg cp sub/large2 large1
  $ hg commit -m "copy files"
  $ cat normal1
  normal22
  $ cat large1
  large22

Test moving largefiles and verify that normal files are also unaffected.

  $ hg mv normal1 normal3
  $ hg mv large1 large3
  $ hg mv sub/normal2 sub/normal4
  $ hg mv sub/large2 sub/large4
  $ hg commit -m "move files"
  $ cat normal3
  normal22
  $ cat large3
  large22
  $ cat sub/normal4
  normal22
  $ cat sub/large4
  large22

Test archiving the various revisions.  These hit corner cases known with
archiving.

  $ hg archive -r 0 ../archive0
  $ hg archive -r 1 ../archive1
  $ hg archive -r 2 ../archive2
  $ hg archive -r 3 ../archive3
  $ hg archive -r 4 ../archive4
  $ cd ../archive0
  $ cat normal1 
  normal1
  $ cat large1
  large1
  $ cat sub/normal2
  normal2
  $ cat sub/large2
  large2
  $ cd ../archive1
  $ cat normal1
  normal11
  $ cat large1
  large11
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22
  $ cd ../archive2
  $ ls
  sub
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22
  $ cd ../archive3
  $ cat normal1
  normal22
  $ cat large1
  large22
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22
  $ cd ../archive4
  $ cat normal3
  normal22
  $ cat large3
  large22
  $ cat sub/normal4
  normal22
  $ cat sub/large4
  large22

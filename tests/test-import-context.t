Test applying context diffs

  $ cat > writepatterns.py <<EOF
  > import sys
  > 
  > path = sys.argv[1]
  > lasteol = sys.argv[2] == '1'
  > patterns = sys.argv[3:]
  > 
  > fp = file(path, 'wb')
  > for i, pattern in enumerate(patterns):
  >     count = int(pattern[0:-1])
  >     char = pattern[-1] + '\n'
  >     if not lasteol and i == len(patterns) - 1:
  >         fp.write((char*count)[:-1])
  >     else:
  >         fp.write(char*count)
  > fp.close()
  > EOF
  $ cat > cat.py <<EOF
  > import sys
  > sys.stdout.write(repr(file(sys.argv[1], 'rb').read()) + '\n')
  > EOF

Initialize the test repository

  $ hg init repo
  $ cd repo
  $ python ../writepatterns.py a 0 5A 1B 5C 1D
  $ python ../writepatterns.py b 1 1A 1B
  $ python ../writepatterns.py c 1 5A
  $ python ../writepatterns.py d 1 5A 1B
  $ hg add
  adding a
  adding b
  adding c
  adding d
  $ hg ci -m addfiles

Add file, missing a last end of line

  $ hg import --no-commit - <<EOF
  > *** /dev/null	2010-10-16 18:05:49.000000000 +0200
  > --- b/newnoeol	2010-10-16 18:23:26.000000000 +0200
  > ***************
  > *** 0 ****
  > --- 1,2 ----
  > + a
  > + b
  > \ No newline at end of file
  > *** a/a	Sat Oct 16 16:35:51 2010
  > --- b/a	Sat Oct 16 16:35:51 2010
  > ***************
  > *** 3,12 ****
  >   A
  >   A
  >   A
  > ! B
  >   C
  >   C
  >   C
  >   C
  >   C
  > ! D
  > \ No newline at end of file
  > --- 3,13 ----
  >   A
  >   A
  >   A
  > ! E
  >   C
  >   C
  >   C
  >   C
  >   C
  > ! F
  > ! F
  > 
  > *** a/b	2010-10-16 18:40:38.000000000 +0200
  > --- /dev/null	2010-10-16 18:05:49.000000000 +0200
  > ***************
  > *** 1,2 ****
  > - A
  > - B
  > --- 0 ----
  > *** a/c	Sat Oct 16 21:34:26 2010
  > --- b/c	Sat Oct 16 21:34:27 2010
  > ***************
  > *** 3,5 ****
  > --- 3,7 ----
  >   A
  >   A
  >   A
  > + B
  > + B
  > *** a/d	Sat Oct 16 21:47:20 2010
  > --- b/d	Sat Oct 16 21:47:22 2010
  > ***************
  > *** 2,6 ****
  >   A
  >   A
  >   A
  > - A
  > - B
  > --- 2,4 ----
  > EOF
  applying patch from stdin
  $ hg st
  M a
  M c
  M d
  A newnoeol
  R b

What's in a

  $ python ../cat.py a
  'A\nA\nA\nA\nA\nE\nC\nC\nC\nC\nC\nF\nF\n'
  $ python ../cat.py newnoeol
  'a\nb'
  $ python ../cat.py c
  'A\nA\nA\nA\nA\nB\nB\n'
  $ python ../cat.py d
  'A\nA\nA\nA\n'

  $ cd ..

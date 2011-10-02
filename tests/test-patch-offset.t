
  $ cat > writepatterns.py <<EOF
  > import sys
  > 
  > path = sys.argv[1]
  > patterns = sys.argv[2:]
  > 
  > fp = file(path, 'wb')
  > for pattern in patterns:
  >     count = int(pattern[0:-1])
  >     char = pattern[-1] + '\n'
  >     fp.write(char*count)
  > fp.close()
  > EOF

prepare repo

  $ hg init a
  $ cd a

These initial lines of Xs were not in the original file used to generate
the patch.  So all the patch hunks need to be applied to a constant offset
within this file.  If the offset isn't tracked then the hunks can be
applied to the wrong lines of this file.

  $ python ../writepatterns.py a 34X 10A 1B 10A 1C 10A 1B 10A 1D 10A 1B 10A 1E 10A 1B 10A
  $ hg commit -Am adda
  adding a

This is a cleaner patch generated via diff
In this case it reproduces the problem when
the output of hg export does not
import patch

  $ hg import -v -m 'b' -d '2 0' - <<EOF
  > --- a/a	2009-12-08 19:26:17.000000000 -0800
  > +++ b/a	2009-12-08 19:26:17.000000000 -0800
  > @@ -9,7 +9,7 @@
  >  A
  >  A
  >  B
  > -A
  > +a
  >  A
  >  A
  >  A
  > @@ -53,7 +53,7 @@
  >  A
  >  A
  >  B
  > -A
  > +a
  >  A
  >  A
  >  A
  > @@ -75,7 +75,7 @@
  >  A
  >  A
  >  B
  > -A
  > +a
  >  A
  >  A
  >  A
  > EOF
  applying patch from stdin
  patching file a
  Hunk #1 succeeded at 43 (offset 34 lines).
  Hunk #2 succeeded at 87 (offset 34 lines).
  Hunk #3 succeeded at 109 (offset 34 lines).
  a
  created 189885cecb41

compare imported changes against reference file

  $ python ../writepatterns.py aref 34X 10A 1B 1a 9A 1C 10A 1B 10A 1D 10A 1B 1a 9A 1E 10A 1B 1a 9A
  $ diff aref a

  $ cat >findbranch.py <<EOF
  > import re, sys
  > 
  > head_re = re.compile('^#(?:(?:\\s+([A-Za-z][A-Za-z0-9_]*)(?:\\s.*)?)|(?:\\s*))$')
  > 
  > for line in sys.stdin:
  >     hmatch = head_re.match(line)
  >     if not hmatch:
  >         sys.exit(1)
  >     if hmatch.group(1) == 'Branch':
  >         sys.exit(0)
  > sys.exit(1)
  > EOF

  $ hg init a
  $ cd a
  $ echo "Rev 1" >rev
  $ hg add rev
  $ hg commit -m "No branch."
  $ hg branch abranch
  marked working directory as branch abranch
  $ echo "Rev  2" >rev
  $ hg commit -m "With branch."

  $ hg export 0 > ../r0.patch
  $ hg export 1 > ../r1.patch
  $ cd ..

  $ if python findbranch.py < r0.patch; then
  >     echo "Export of default branch revision has Branch header" 1>&2
  >     exit 1
  > fi

  $ if python findbranch.py < r1.patch; then
  >     :  # Do nothing
  > else
  >     echo "Export of branch revision is missing Branch header" 1>&2
  >     exit 1
  > fi

Make sure import still works with branch information in patches.

  $ hg init b
  $ cd b
  $ hg import ../r0.patch
  applying ../r0.patch
  $ hg import ../r1.patch
  applying ../r1.patch
  $ cd ..

  $ hg init c
  $ cd c
  $ hg import --exact ../r0.patch
  applying ../r0.patch
  $ hg import --exact ../r1.patch
  applying ../r1.patch

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
  $ if hg export 0 | python ../findbranch.py; then
  >     echo "Export of default branch revision has Branch header" 1>&2
  >     exit 1
  > fi
  $ if hg export 1 | python ../findbranch.py; then
  >     :  # Do nothing
  > else
  >     echo "Export of branch revision is missing Branch header" 1>&2
  >     exit 1
  > fi

Make sure import still works with branch information in patches.

  $ cd ..
  $ hg init b
  $ cd b
  $ hg -R ../a export 0 | hg import -
  applying patch from stdin
  $ hg -R ../a export 1 | hg import -
  applying patch from stdin
  $ cd ..
  $ rm -rf b
  $ hg init b
  $ cd b
  $ hg -R ../a export 0 | hg import --exact -
  applying patch from stdin
  $ hg -R ../a export 1 | hg import --exact -
  applying patch from stdin

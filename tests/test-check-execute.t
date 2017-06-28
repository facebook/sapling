#require test-repo execbit

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "`dirname "$TESTDIR"`"

look for python scripts without the execute bit

  $ syshg files 'set:**.py and not exec() and grep(r"^#!.*?python")'
  [1]

look for python scripts with execute bit but not shebang

  $ syshg files 'set:**.py and exec() and not grep(r"^#!.*?python")'
  [1]

look for shell scripts with execute bit but not shebang

  $ syshg files 'set:**.sh and exec() and not grep(r"^#!.*(ba)?sh")'
  [1]

look for non scripts with no shebang

  $ syshg files 'set:exec() and not **.sh and not **.py and not grep(r"^#!")'
  [1]

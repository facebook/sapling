run only on case-insensitive filesystems, because collision check at
"hg update" is done only on case-insensitive filesystems

  $ "$TESTDIR/hghave" icasefs || exit 80

setup repository, and target files

  $ HGENCODING=cp932
  $ export HGENCODING
  $ hg init t
  $ cd t
  $ python << EOF
  > names = ["\x83\x41", # cp932(0x83, 0x41='A'), UNICODE(0x30a2)
  >          "\x83\x5A", # cp932(0x83, 0x5A='Z'), UNICODE(0x30bb)
  >          "\x83\x61", # cp932(0x83, 0x61='a'), UNICODE(0x30c2)
  >          "\x83\x7A", # cp932(0x83, 0x7A='z'), UNICODE(0x30db)
  >         ]
  > for num, name in zip(range(len(names)), names):
  >     # file for getting target filename of "hg add"
  >     f = file(str(num), 'w'); f.write(name); f.close()
  >     # target file of "hg add"
  >     f = file(name, 'w'); f.write(name); f.close()
  > EOF

test filename collison check at "hg add"

  $ hg add --config ui.portablefilenames=abort `cat 0`
  $ hg add --config ui.portablefilenames=abort `cat 1`
  $ hg add --config ui.portablefilenames=abort `cat 2`
  $ hg add --config ui.portablefilenames=abort `cat 3`
  $ hg status -a
  A \x83A (esc)
  A \x83Z (esc)
  A \x83a (esc)
  A \x83z (esc)

test filename collision check at "hg update"

  $ hg commit -m 'revision 0'
  $ hg update null
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ hg update tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved

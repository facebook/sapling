
  $ cat > unix2mac.py <<EOF
  > import sys
  > 
  > for path in sys.argv[1:]:
  >     data = file(path, 'rb').read()
  >     data = data.replace('\n', '\r')
  >     file(path, 'wb').write(data)
  > EOF
  $ cat > print.py <<EOF
  > import sys
  > print(sys.stdin.read().replace('\n', '<LF>').replace('\r', '<CR>').replace('\0', '<NUL>'))
  > EOF
  $ hg init
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'pretxncommit.cr = python:hgext.win32text.forbidcr' >> .hg/hgrc
  $ echo 'pretxnchangegroup.cr = python:hgext.win32text.forbidcr' >> .hg/hgrc
  $ cat .hg/hgrc
  [hooks]
  pretxncommit.cr = python:hgext.win32text.forbidcr
  pretxnchangegroup.cr = python:hgext.win32text.forbidcr

  $ echo hello > f
  $ hg add f
  $ hg ci -m 1

  $ python unix2mac.py f
  $ hg ci -m 2
  Attempt to commit or push text file(s) using CR line endings
  in dea860dc51ec: f
  transaction abort!
  rollback completed
  abort: pretxncommit.cr hook failed
  [255]
  $ hg cat f | python print.py
  hello<LF>
  $ cat f | python print.py
  hello<CR>

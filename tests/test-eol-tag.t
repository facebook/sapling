http://mercurial.selenic.com/bts/issue2493

Testing tagging with the EOL extension

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > eol =
  > 
  > [eol]
  > native = CRLF
  > EOF

setup repository

  $ hg init repo
  $ cd repo
  $ cat > .hgeol <<EOF
  > [patterns]
  > ** = native
  > EOF
  $ printf "first\r\nsecond\r\nthird\r\n" > a.txt
  $ hg commit --addremove -m 'checkin'
  adding .hgeol
  adding a.txt

Tag:

  $ hg tag 1.0

Rewrite .hgtags file as it would look on a new checkout:

  $ hg update -q null
  $ hg update -q

Touch .hgtags file again:

  $ hg tag 2.0

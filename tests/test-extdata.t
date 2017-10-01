  $ hg init repo
  $ cd repo
  $ for n in 0 1 2 3; do
  >   echo $n > $n
  >   hg ci -qAm $n
  > done

test revset support

  $ cat <<'EOF' >> .hg/hgrc
  > [extdata]
  > filedata = file:extdata.txt
  > shelldata = shell:cat extdata.txt | grep 2
  > EOF
  $ cat <<'EOF' > extdata.txt
  > 2
  > 3
  > EOF

  $ hg log -qr "extdata(filedata)"
  2:f6ed99a58333
  3:9de260b1e88e
  $ hg log -qr "extdata(shelldata)"
  2:f6ed99a58333

test weight of extdata() revset

  $ hg debugrevspec -p optimized "extdata(filedata) & 3"
  * optimized:
  (andsmally
    (func
      (symbol 'extdata')
      (symbol 'filedata'))
    (symbol '3'))
  3

test bad extdata() revset source

  $ hg log -qr "extdata()"
  hg: parse error: extdata takes at least 1 string argument
  [255]
  $ hg log -qr "extdata(unknown)"
  abort: unknown extdata source 'unknown'
  [255]

we don't fix up relative file URLs, but we do run shell commands in repo root

  $ mkdir sub
  $ cd sub
  $ hg log -qr "extdata(filedata)"
  abort: error: No such file or directory
  [255]
  $ hg log -qr "extdata(shelldata)"
  2:f6ed99a58333

  $ cd ..

  $ hg init repo
  $ cd repo
  $ for n in 0 1 2 3 4 5 6 7 8 9 10 11; do
  >   echo $n > $n
  >   hg ci -qAm $n
  > done

test revset support

  $ cat <<'EOF' >> .hg/hgrc
  > [extdata]
  > filedata = file:extdata.txt
  > notes = notes.txt
  > shelldata = shell:cat extdata.txt | grep 2
  > emptygrep = shell:cat extdata.txt | grep empty
  > EOF
  $ cat <<'EOF' > extdata.txt
  > 2 another comment on 2
  > 3
  > EOF
  $ cat <<'EOF' > notes.txt
  > f6ed this change is great!
  > e834 this is buggy :(
  > 0625 first post
  > bogusnode gives no error
  > a ambiguous node gives no error
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

test non-zero exit of shell command

  $ hg log -qr "extdata(emptygrep)"
  $ hg log -qr "extdata(emptygrep)" --debug
  extdata command 'cat extdata.txt | grep empty' exited with status * (glob)

test bad extdata() revset source

  $ hg log -qr "extdata()"
  hg: parse error: extdata takes at least 1 string argument
  [255]
  $ hg log -qr "extdata(unknown)"
  abort: unknown extdata source 'unknown'
  [255]

test template support:

  $ hg log -r:3 -T "{node|short}{if(extdata('notes'), ' # {extdata('notes')}')}\n"
  06254b906311 # first post
  e8342c9a2ed1 # this is buggy :(
  f6ed99a58333 # this change is great!
  9de260b1e88e

test template cache:

  $ hg log -r:3 -T '{rev} "{extdata("notes")}" "{extdata("shelldata")}"\n'
  0 "first post" ""
  1 "this is buggy :(" ""
  2 "this change is great!" "another comment on 2"
  3 "" ""

test bad extdata() template source

  $ hg log -T "{extdata()}\n"
  hg: parse error: extdata expects one argument
  [255]
  $ hg log -T "{extdata('unknown')}\n"
  abort: unknown extdata source 'unknown'
  [255]

we don't fix up relative file URLs, but we do run shell commands in repo root

  $ mkdir sub
  $ cd sub
  $ hg log -qr "extdata(filedata)"
  abort: error: The system cannot find the file specified (windows !)
  abort: error: No such file or directory (no-windows !)
  [255]
  $ hg log -qr "extdata(shelldata)"
  2:f6ed99a58333

  $ cd ..

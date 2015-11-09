Tests of the file helper tool

  $ f -h
  ?sage: f [options] [filenames] (glob)
  
  ?ptions: (glob)
    -h, --help            show this help message and exit
    -t, --type            show file type (file or directory)
    -m, --mode            show file mode
    -l, --links           show number of links
    -s, --size            show size of file
    -n NEWER, --newer=NEWER
                          check if file is newer (or same)
    -r, --recurse         recurse into directories
    -S, --sha1            show sha1 hash of the content
    -M, --md5             show md5 hash of the content
    -D, --dump            dump file content
    -H, --hexdump         hexdump file content
    -B BYTES, --bytes=BYTES
                          number of characters to dump
    -L LINES, --lines=LINES
                          number of lines to dump
    -q, --quiet           no default output

  $ mkdir dir
  $ cd dir

  $ f --size
  size=0

  $ echo hello | f --md5 --size
  size=6, md5=b1946ac92492d2347c6235b4d2611184

  $ f foo
  foo: file not found

  $ echo foo > foo
  $ f foo
  foo:

#if symlink
  $ f foo --mode
  foo: mode=644
#endif

#if no-windows
  $ python $TESTDIR/seq.py 10 > bar
#else
Convert CRLF -> LF for consistency
  $ python $TESTDIR/seq.py 10 | sed "s/$//" > bar
#endif

#if unix-permissions symlink
  $ chmod +x bar
  $ f bar --newer foo --mode --type --size --dump --links --bytes 7
  bar: file, size=21, mode=755, links=1, newer than foo
  >>>
  1
  2
  3
  4
  <<< no trailing newline
#endif

#if unix-permissions
  $ ln bar baz
  $ f bar -n baz -l --hexdump -t --sha1 --lines=9 -B 20
  bar: file, links=2, newer than baz, sha1=612ca68d0305c821750a
  0000: 31 0a 32 0a 33 0a 34 0a 35 0a 36 0a 37 0a 38 0a |1.2.3.4.5.6.7.8.|
  0010: 39 0a                                           |9.|
  $ rm baz
#endif

#if unix-permissions symlink
  $ ln -s yadda l
  $ f . --recurse -MStmsB4
  .: directory with 3 files, mode=755
  ./bar: file, size=21, mode=755, md5=3b03, sha1=612c
  ./foo: file, size=4, mode=644, md5=d3b0, sha1=f1d2
  ./l: link, size=5, md5=2faa, sha1=af93
#endif

  $ f --quiet bar -DL 3
  1
  2
  3

  $ cd ..

Yadda is a symlink
#if symlink
  $ f -qr dir -HB 17
  dir: directory with 3 files
  dir/bar:
  0000: 31 0a 32 0a 33 0a 34 0a 35 0a 36 0a 37 0a 38 0a |1.2.3.4.5.6.7.8.|
  0010: 39                                              |9|
  dir/foo:
  0000: 66 6f 6f 0a                                     |foo.|
  dir/l:
  0000: 79 61 64 64 61                                  |yadda|
#else
  $ f -qr dir -HB 17
  dir: directory with 2 files (esc)
  dir/bar: (glob)
  0000: 31 0a 32 0a 33 0a 34 0a 35 0a 36 0a 37 0a 38 0a |1.2.3.4.5.6.7.8.|
  0010: 39                                              |9|
  dir/foo: (glob)
  0000: 66 6f 6f 0a                                     |foo.|
#endif


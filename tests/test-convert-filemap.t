
  $ HGMERGE=true; export HGMERGE
  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'graphlog =' >> $HGRCPATH
  $ echo 'convert =' >> $HGRCPATH
  $ glog()
  > {
  >     hg glog --template '{rev} "{desc}" files: {files}\n' "$@"
  > }
  $ hg init source
  $ cd source
  $ echo foo > foo
  $ echo baz > baz
  $ mkdir -p dir/subdir
  $ echo dir/file >> dir/file
  $ echo dir/file2 >> dir/file2
  $ echo dir/subdir/file3 >> dir/subdir/file3
  $ echo dir/subdir/file4 >> dir/subdir/file4
  $ hg ci -d '0 0' -qAm '0: add foo baz dir/'
  $ echo bar > bar
  $ echo quux > quux
  $ hg copy foo copied
  $ hg ci -d '1 0' -qAm '1: add bar quux; copy foo to copied'
  $ echo >> foo
  $ hg ci -d '2 0' -m '2: change foo'
  $ hg up -qC 1
  $ echo >> bar
  $ echo >> quux
  $ hg ci -d '3 0' -m '3: change bar quux'
  created new head
  $ hg up -qC 2
  $ hg merge -qr 3
  $ echo >> bar
  $ echo >> baz
  $ hg ci -d '4 0' -m '4: first merge; change bar baz'
  $ echo >> bar
  $ echo 1 >> baz
  $ echo >> quux
  $ hg ci -d '5 0' -m '5: change bar baz quux'
  $ hg up -qC 4
  $ echo >> foo
  $ echo 2 >> baz
  $ hg ci -d '6 0' -m '6: change foo baz'
  created new head
  $ hg up -qC 5
  $ hg merge -qr 6
  $ echo >> bar
  $ hg ci -d '7 0' -m '7: second merge; change bar'
  $ echo >> foo
  $ hg ci -m '8: change foo'
  $ glog
  @  8 "8: change foo" files: foo
  |
  o    7 "7: second merge; change bar" files: bar baz
  |\
  | o  6 "6: change foo baz" files: baz foo
  | |
  o |  5 "5: change bar baz quux" files: bar baz quux
  |/
  o    4 "4: first merge; change bar baz" files: bar baz
  |\
  | o  3 "3: change bar quux" files: bar quux
  | |
  o |  2 "2: change foo" files: foo
  |/
  o  1 "1: add bar quux; copy foo to copied" files: bar copied quux
  |
  o  0 "0: add foo baz dir/" files: baz dir/file dir/file2 dir/subdir/file3 dir/subdir/file4 foo
  

final file versions in this repo:

  $ hg manifest --debug
  9463f52fe115e377cf2878d4fc548117211063f2 644   bar
  94c1be4dfde2ee8d78db8bbfcf81210813307c3d 644   baz
  7711d36246cc83e61fb29cd6d4ef394c63f1ceaf 644   copied
  3e20847584beff41d7cd16136b7331ab3d754be0 644   dir/file
  75e6d3f8328f5f6ace6bf10b98df793416a09dca 644   dir/file2
  5fe139720576e18e34bcc9f79174db8897c8afe9 644   dir/subdir/file3
  57a1c1511590f3de52874adfa04effe8a77d64af 644   dir/subdir/file4
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  bc3eca3f47023a3e70ca0d8cc95a22a6827db19d 644   quux
  $ hg debugrename copied
  copied renamed from foo:2ed2a3912a0b24502043eae84ee4b279c18b90dd
  $ echo
  
  $ cd ..
  $ splitrepo()
  > {
  >     msg="$1"
  >     files="$2"
  >     opts=$3
  >     echo "% $files: $msg"
  >     prefix=`echo "$files" | sed -e 's/ /-/g'`
  >     fmap="$prefix.fmap"
  >     repo="$prefix.repo"
  >     for i in $files; do
  >         echo "include $i" >> "$fmap"
  >     done
  >     hg -q convert $opts --filemap "$fmap" --datesort source "$repo"
  >     hg up -q -R "$repo"
  >     glog -R "$repo"
  >     hg -R "$repo" manifest --debug
  > }
  $ splitrepo 'skip unwanted merges; use 1st parent in 1st merge, 2nd in 2nd' foo
  % foo: skip unwanted merges; use 1st parent in 1st merge, 2nd in 2nd
  @  3 "8: change foo" files: foo
  |
  o  2 "6: change foo baz" files: foo
  |
  o  1 "2: change foo" files: foo
  |
  o  0 "0: add foo baz dir/" files: foo
  
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  $ splitrepo 'merges are not merges anymore' bar
  % bar: merges are not merges anymore
  @  4 "7: second merge; change bar" files: bar
  |
  o  3 "5: change bar baz quux" files: bar
  |
  o  2 "4: first merge; change bar baz" files: bar
  |
  o  1 "3: change bar quux" files: bar
  |
  o  0 "1: add bar quux; copy foo to copied" files: bar
  
  9463f52fe115e377cf2878d4fc548117211063f2 644   bar
  $ splitrepo '1st merge is not a merge anymore; 2nd still is' baz
  % baz: 1st merge is not a merge anymore; 2nd still is
  @    4 "7: second merge; change bar" files: baz
  |\
  | o  3 "6: change foo baz" files: baz
  | |
  o |  2 "5: change bar baz quux" files: baz
  |/
  o  1 "4: first merge; change bar baz" files: baz
  |
  o  0 "0: add foo baz dir/" files: baz
  
  94c1be4dfde2ee8d78db8bbfcf81210813307c3d 644   baz
  $ splitrepo 'we add additional merges when they are interesting' 'foo quux'
  % foo quux: we add additional merges when they are interesting
  @  8 "8: change foo" files: foo
  |
  o    7 "7: second merge; change bar" files:
  |\
  | o  6 "6: change foo baz" files: foo
  | |
  o |  5 "5: change bar baz quux" files: quux
  |/
  o    4 "4: first merge; change bar baz" files:
  |\
  | o  3 "3: change bar quux" files: quux
  | |
  o |  2 "2: change foo" files: foo
  |/
  o  1 "1: add bar quux; copy foo to copied" files: quux
  |
  o  0 "0: add foo baz dir/" files: foo
  
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  bc3eca3f47023a3e70ca0d8cc95a22a6827db19d 644   quux
  $ splitrepo 'partial conversion' 'bar quux' '-r 3'
  % bar quux: partial conversion
  @  1 "3: change bar quux" files: bar quux
  |
  o  0 "1: add bar quux; copy foo to copied" files: bar quux
  
  b79105bedc55102f394e90a789c9c380117c1b4a 644   bar
  db0421cc6b685a458c8d86c7d5c004f94429ea23 644   quux
  $ splitrepo 'complete the partial conversion' 'bar quux'
  % bar quux: complete the partial conversion
  @  4 "7: second merge; change bar" files: bar
  |
  o  3 "5: change bar baz quux" files: bar quux
  |
  o  2 "4: first merge; change bar baz" files: bar
  |
  o  1 "3: change bar quux" files: bar quux
  |
  o  0 "1: add bar quux; copy foo to copied" files: bar quux
  
  9463f52fe115e377cf2878d4fc548117211063f2 644   bar
  bc3eca3f47023a3e70ca0d8cc95a22a6827db19d 644   quux
  $ rm -r foo.repo
  $ splitrepo 'partial conversion' 'foo' '-r 3'
  % foo: partial conversion
  @  0 "0: add foo baz dir/" files: foo
  
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   foo
  $ splitrepo 'complete the partial conversion' 'foo'
  % foo: complete the partial conversion
  @  3 "8: change foo" files: foo
  |
  o  2 "6: change foo baz" files: foo
  |
  o  1 "2: change foo" files: foo
  |
  o  0 "0: add foo baz dir/" files: foo
  
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  $ splitrepo 'copied file; source not included in new repo' copied
  % copied: copied file; source not included in new repo
  @  0 "1: add bar quux; copy foo to copied" files: copied
  
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   copied
  $ hg --cwd copied.repo debugrename copied
  copied not renamed
  $ splitrepo 'copied file; source included in new repo' 'foo copied'
  % foo copied: copied file; source included in new repo
  @  4 "8: change foo" files: foo
  |
  o  3 "6: change foo baz" files: foo
  |
  o  2 "2: change foo" files: foo
  |
  o  1 "1: add bar quux; copy foo to copied" files: copied
  |
  o  0 "0: add foo baz dir/" files: foo
  
  7711d36246cc83e61fb29cd6d4ef394c63f1ceaf 644   copied
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  $ hg --cwd foo-copied.repo debugrename copied
  copied renamed from foo:2ed2a3912a0b24502043eae84ee4b279c18b90dd
  $ cat > renames.fmap <<EOF
  > include dir
  > exclude dir/file2
  > rename dir dir2
  > include foo
  > include copied
  > rename foo foo2
  > rename copied copied2
  > exclude dir/subdir
  > include dir/subdir/file3
  > EOF
  $ hg -q convert --filemap renames.fmap --datesort source renames.repo
  $ hg up -q -R renames.repo
  $ glog -R renames.repo
  @  4 "8: change foo" files: foo2
  |
  o  3 "6: change foo baz" files: foo2
  |
  o  2 "2: change foo" files: foo2
  |
  o  1 "1: add bar quux; copy foo to copied" files: copied2
  |
  o  0 "0: add foo baz dir/" files: dir2/file dir2/subdir/file3 foo2
  
  $ hg -R renames.repo manifest --debug
  d43feacba7a4f1f2080dde4a4b985bd8a0236d46 644   copied2
  3e20847584beff41d7cd16136b7331ab3d754be0 644   dir2/file
  5fe139720576e18e34bcc9f79174db8897c8afe9 644   dir2/subdir/file3
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo2
  $ hg --cwd renames.repo debugrename copied2
  copied2 renamed from foo2:2ed2a3912a0b24502043eae84ee4b279c18b90dd

copied:

  $ hg --cwd source cat copied
  foo

copied2:

  $ hg --cwd renames.repo cat copied2
  foo

filemap errors

  $ cat > errors.fmap <<EOF
  > include dir/ # beware that comments changes error line numbers!
  > exclude /dir
  > rename dir//dir /dir//dir/ "out of sync"
  > include
  > EOF
  $ hg -q convert --filemap errors.fmap source errors.repo
  errors.fmap:1: superfluous / in exclude 'dir/'
  errors.fmap:3: superfluous / in include '/dir'
  errors.fmap:3: superfluous / in rename '/dir'
  errors.fmap:3: superfluous / in exclude 'dir//dir'
  errors.fmap:4: unknown directive 'out of sync'
  errors.fmap:5: path to exclude is missing
  abort: errors in filemap
  [255]

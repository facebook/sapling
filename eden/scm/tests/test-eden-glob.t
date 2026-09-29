
#require eden mononoke

  $ export SL_REPO_IDENTITY=hg

setup backing repo

  $ newclientrepo crepo1 serverrepo
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
  $ for i in html www/html .dps i/.mean slowly; do
  >   mkdir -p $i
  > done
  $ for i in foo.bcmap html/baz.bcmap www/html/baz.bcmap baz.txt foo.txt bar.rs throw.dot .dps/very.dot .more.dot .stop.dot i/.mean/slow.dot slowly/.and.by.slow.dot throw.dot; do
  >   touch $i
  > done
  $ sl commit -Am "many files now" -q
  $ sl push --to master --create -q
  $ sl push --to theB -r $B --create -q
  $ newclientrepo clientrepo serverrepo

test raw edenapi queries
  $ sl debugapi -e suffix_query -i "{'Hg': '$(sl whereami)'}" -i "['.bcmap']" -i "['html']"
  [{"file_path": "html/baz.bcmap"}]

test eden glob
  $ eden debug logging eden/fs/service=DBG4 > /dev/null
  $ eden glob '**/*.bcmap' --list-only-files
  foo.bcmap
  html/baz.bcmap
  www/html/baz.bcmap
  $ cd html
  $ eden glob '**/*.bcmap' --list-only-files
  baz.bcmap
  $ cd ../www/html
  $ eden glob '**/*.bcmap' --list-only-files
  baz.bcmap
  $ cd ../..

  $ eden glob '**/*.txt' --list-only-files
  baz.txt
  foo.txt
  $ mkdir depth1
  $ cd depth1
# return nothing due to not being in repo root
  $ eden glob '**/*.rs' --list-only-files
# Add repo flag to use root instead of cwd
  $ eden glob '**/*.rs' --list-only-files --repo $TESTTMP/clientrepo
  bar.rs
  $ mkdir depth2
  $ cd depth2
  $ eden glob '**/*.dot' --list-only-files --repo $TESTTMP/clientrepo
  throw.dot
  $ cd ../..
  $ eden glob '**/*.dot' --include-dot-files --list-only-files
  .dps/very.dot
  .more.dot
  .stop.dot
  i/.mean/slow.dot
  slowly/.and.by.slow.dot
  throw.dot

Test local files
  $ eden glob '**/*.local' --list-only-files
  $ touch local.local
  $ eden glob '**/*.local' --list-only-files
  local.local
# Test that local files do not show up when using revision
  $ eden glob '**/*.local' --list-only-files --revision 0000000000000000000000000000000000000000

# Test that local file dtype changes register
  $ sl checkout $A -q
  $ touch bar.rs
  $ sl add bar.rs
  $ sl amend 2> /dev/null
  $ sl checkout 072da8606ee7 > /dev/null
  $ eden glob **/*.rs --dtype --list-only-files
  bar.rs Regular
  $ mv bar.rs barlink.rs
  $ ln -s barlink.rs bar.rs
  $ eden glob **/*.rs --dtype --list-only-files
  bar.rs Symlink
  barlink.rs Regular

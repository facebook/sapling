  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/githelp.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > githelp=$TESTTMP/githelp.py
  > EOF

  $ hg init repo
  $ cd repo
  $ echo foo > test_file
  $ mkdir dir
  $ echo foo > dir/file
  $ echo foo > removed_file
  $ echo foo > deleted_file
  $ hg add -q .
  $ hg commit -m 'bar'
  $ hg bookmark both
  $ touch both
  $ touch untracked_file
  $ hg remove removed_file
  $ rm deleted_file

githelp on a single command should succeed
  $ hg githelp -- commit
  hg commit

githelp on a command with options should succeed
  $ hg githelp -- commit -pm "abc"
  hg record -m 'abc'

githelp on a command with standalone unrecognized option should succeed with warning
  $ hg githelp -- commit -p -v
  ignoring unknown option -v
  hg record

githelp on a command with unrecognized option packed with other options should fail with error
  $ hg githelp -- commit -pv
  abort: unknown option v packed with other options
  Please try passing the option as it's own flag: -v
  
  If this is a valid git command, please log a task for the source_control oncall.
  
  [255]


githelp for git rebase --skip
  $ hg githelp -- git rebase --skip
  hg revert --all -r .
  hg rebase --continue

githelp for git commit --amend (hg commit --amend pulls up an editor)
  $ hg githelp -- commit --amend
  hg commit --amend

githelp for git commit --amend --no-edit (hg amend does not pull up an editor)
  $ hg githelp -- commit --amend --no-edit
  hg amend

githelp for git checkout -- . (checking out a directory)
  $ hg githelp -- checkout -- .
  note: use --no-backup to avoid creating .orig files
  
  hg revert .


githelp for git checkout "HEAD^" (should still work to pass a rev)
  $ hg githelp -- checkout "HEAD^"
  hg update .^

githelp checkout: args after -- should be treated as paths no matter what
  $ hg githelp -- checkout -- HEAD
  note: use --no-backup to avoid creating .orig files
  
  hg revert HEAD


githelp for git checkout with rev and path
  $ hg githelp -- checkout "HEAD^" -- file.txt
  note: use --no-backup to avoid creating .orig files
  
  hg revert -r .^ file.txt


githelp for git with rev and path, without separator
  $ hg githelp -- checkout "HEAD^" file.txt
  note: use --no-backup to avoid creating .orig files
  
  hg revert -r .^ file.txt


githelp for checkout with a file as first argument
  $ hg githelp -- checkout test_file
  note: use --no-backup to avoid creating .orig files
  
  hg revert test_file


githelp for checkout with a removed file as first argument
  $ hg githelp -- checkout removed_file
  note: use --no-backup to avoid creating .orig files
  
  hg revert removed_file


githelp for checkout with a deleted file as first argument
  $ hg githelp -- checkout deleted_file
  note: use --no-backup to avoid creating .orig files
  
  hg revert deleted_file


githelp for checkout with a untracked file as first argument
  $ hg githelp -- checkout untracked_file
  note: use --no-backup to avoid creating .orig files
  
  hg revert untracked_file


githelp for checkout with a directory as first argument
  $ hg githelp -- checkout dir
  note: use --no-backup to avoid creating .orig files
  
  hg revert dir


githelp for checkout when not in repo root
  $ cd dir
  $ hg githelp -- checkout file
  note: use --no-backup to avoid creating .orig files
  
  hg revert file

  $ cd ..

githelp for checkout with an argument that is both a file and a revision
  $ hg githelp -- checkout both
  hg update both

githelp for checkout with the -p option
  $ hg githelp -- git checkout -p xyz
  hg revert -i -r xyz

  $ hg githelp -- git checkout -p xyz -- abc
  note: use --no-backup to avoid creating .orig files
  
  hg revert -i -r xyz abc

githelp for checkout with the -f option and a rev
  $ hg githelp -- git checkout -f xyz
  hg update -C xyz
  $ hg githelp -- git checkout --force xyz
  hg update -C xyz

githelp for checkout with the -f option without an arg
  $ hg githelp -- git checkout -f
  hg revert --all
  $ hg githelp -- git checkout --force
  hg revert --all

githelp for grep with pattern and path
  $ hg githelp -- grep shrubbery flib/intern/
  hg grep shrubbery flib/intern/

githelp for reset, checking ~ in git becomes ~1 in mercurial
  $ hg githelp -- reset HEAD~
  hg reset .~1
  $ hg githelp -- reset "HEAD^"
  hg reset .^
  $ hg githelp -- reset HEAD~3
  hg reset .~3

githelp for git show --name-status
  $ hg githelp -- git show --name-status
  hg log --style status -r tip

githelp for git show --pretty=format: --name-status
  $ hg githelp -- git show --pretty=format: --name-status
  hg stat --change tip

githelp for show with no arguments
  $ hg githelp -- show
  hg show

githelp for show with a path
  $ hg githelp -- show test_file
  hg diff -r ".^" test_file

githelp for show with not a path:
  $ hg githelp -- show rev
  hg show rev

githelp for show with too many arguments
  $ hg githelp -- show argone argtwo
  hg show argone

githelp for show with --unified options
  $ hg githelp -- show --unified=10
  hg show --config diff.unified=10
  $ hg githelp -- show -U100
  hg show --config diff.unified=100

githelp for show with a path and --unified
  $ hg githelp -- show -U20 test_file
  hg diff -r ".^" --unified=20 test_file

githelp for stash drop without name
  $ hg githelp -- git stash drop
  hg shelve -d <shelve name>

githelp for stash drop with name
  $ hg githelp -- git stash drop xyz
  hg shelve -d xyz

githelp for whatchanged should show deprecated message
  $ hg githelp -- whatchanged -p
  This command has been deprecated in the git project, thus isn't supported by this tool.
  

githelp for git branch -m renaming
  $ hg githelp -- git branch -m old new
  hg bookmark -m old new

When the old name is omitted, git branch -m new renames the current branch.
  $ hg githelp -- git branch -m new
  hg bookmark -m `hg log -T"{activebookmark}" -r .` new

githelp for apply with no options
  $ hg githelp -- apply
  hg import

githelp for apply with directory strip custom
  $ hg githelp -- apply -p 5
  hg import -p 5

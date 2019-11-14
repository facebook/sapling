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
  $ hg githelp -- git commit
  hg commit

githelp should fail nicely if we don't give it arguments
  $ hg githelp
  abort: missing git command - usage: hg githelp -- <git command>
  [255]
  $ hg githelp -- git
  abort: missing git command - usage: hg githelp -- <git command>
  [255]

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
  
  If this is a valid git command, please search/ask in the Source Control @ FB group (and don't forget to tell us what the git command does).
  [255]

githelp with a customized footer for invalid commands
  $ hg --config githelp.unknown.footer="This is a custom footer." githelp -- commit -pv
  abort: unknown option v packed with other options
  Please try passing the option as it's own flag: -v
  
  This is a custom footer.
  [255]

githelp for git rebase --skip
  $ hg githelp -- git rebase --skip
  hg revert --all -r .
  hg rebase --continue

githelp for git rebase --interactive
  $ hg githelp -- git rebase -i master
  note: if you don't need to rebase use 'hg histedit'. It just edits history.
  
  also note: 'hg histedit' will automatically detect your stack, so no second argument is necessary.
  
  hg rebase --interactive -d master

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
  hg show . test_file

githelp for show with not a path:
  $ hg githelp -- show rev
  hg show rev

githelp for show with many arguments
  $ hg githelp -- show argone argtwo
  hg show argone argtwo
  $ hg githelp -- show test_file argone argtwo
  hg show . test_file argone argtwo

githelp for show with --unified options
  $ hg githelp -- show --unified=10
  hg show --config diff.unified=10
  $ hg githelp -- show -U100
  hg show --config diff.unified=100

githelp for show with a path and --unified
  $ hg githelp -- show -U20 test_file
  hg show . test_file --config diff.unified=20

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

Branch deletion in git strips commits
  $ hg githelp -- git branch -d
  hg hide -B
  $ hg githelp -- git branch -d feature
  hg hide -B feature
  $ hg githelp -- git branch --delete experiment1 experiment2
  hg hide -B experiment1 -B experiment2

githelp for reuse message using the shorthand
  $ hg githelp -- git commit -C deadbeef
  hg commit -M deadbeef

githelp for reuse message using the the long version
  $ hg githelp -- git commit --reuse-message deadbeef
  hg commit -M deadbeef

githelp for apply with no options
  $ hg githelp -- apply
  hg import --no-commit

githelp for apply with directory strip custom
  $ hg githelp -- apply -p 5
  hg import --no-commit -p 5

git merge-base
  $ hg githelp -- git merge-base --is-ancestor
  ignoring unknown option --is-ancestor
  NOTE: ancestors() is part of the revset language.
  Learn more about revsets with 'hg help revsets'
  
  hg log -T '{node}\n' -r 'ancestor(A,B)'

githelp for git blame (tweakdefaults disabled)
  $ hg githelp -- git blame
  hg annotate -udl

githelp for git blame (tweakdefaults enabled)
  $ hg --config extensions.tweakdefaults= githelp -- git blame
  hg annotate -pudl


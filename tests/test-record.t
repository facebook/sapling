Set up a repo

  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interactive = true
  > [extensions]
  > record =
  > EOF

  $ hg init a
  $ cd a

Record help

  $ hg record -h
  hg record [OPTION]... [FILE]...
  
  interactively select changes to commit
  
      If a list of files is omitted, all changes reported by 'hg status' will be
      candidates for recording.
  
      See 'hg help dates' for a list of formats valid for -d/--date.
  
      You will be prompted for whether to record changes to each modified file,
      and for files with multiple changes, for each change to use. For each
      query, the following responses are possible:
  
        y - record this change
        n - skip this change
        e - edit this change manually
  
        s - skip remaining changes to this file
        f - record remaining changes to this file
  
        d - done, skip remaining changes and files
        a - record all changes to all remaining files
        q - quit, recording no changes
  
        ? - display help
  
      This command is not available when committing a merge.
  
  (use "hg help -e record" to show help for the record extension)
  
  options ([+] can be repeated):
  
   -A --addremove           mark new/missing files as added/removed before
                            committing
      --close-branch        mark a branch head as closed
      --amend               amend the parent of the working directory
   -s --secret              use the secret phase for committing
   -e --edit                invoke editor on commit messages
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -m --message TEXT        use text as commit message
   -l --logfile FILE        read commit message from file
   -d --date DATE           record the specified date as commit date
   -u --user USER           record the specified user as committer
   -S --subrepos            recurse into subrepositories
   -w --ignore-all-space    ignore white space when comparing lines
   -b --ignore-space-change ignore changes in the amount of white space
   -B --ignore-blank-lines  ignore changes whose lines are all blank
  
  (some details hidden, use --verbose to show complete help)

Select no files

  $ touch empty-rw
  $ hg add empty-rw

  $ hg record empty-rw<<EOF
  > n
  > EOF
  diff --git a/empty-rw b/empty-rw
  new file mode 100644
  examine changes to 'empty-rw'? [Ynesfdaq?] n
  
  no changes to record

  $ hg tip -p
  changeset:   -1:000000000000
  tag:         tip
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  



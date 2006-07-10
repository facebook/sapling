" vim600: set foldmethod=marker:
"
" Vim plugin to assist in working with CVS-controlled files.
"
" Last Change:   2006/02/22
" Version:       1.76
" Maintainer:    Bob Hiestand <bob.hiestand@gmail.com>
" License:       This file is placed in the public domain.
" Credits: {{{1
"                Mathieu Clabaut for many suggestions and improvements.
"
"                Suresh Govindachar and Jeeva Chelladhurai for finding waaaay
"                too many bugs.
"
"                Suresh Govindachar (again!) for finding the
"                fully-folded-last-line-delete bug.
"
"                Albrecht Gass for the Delete-on-Hide behavior suggestion.
"
"                Joe MacDonald for finding the CVS log message header bug and
"                pointing out that buffer refreshes are needed after CVS
"                \%(un\)\?edit.
"
"                Srinath Avadhanula for the suggestion and original patch for
"                the CVSCommitOnWrite option and mapping hot key.
"
"                John Sivak for helping to debug Windows issues and suggesting
"                the CVSEditors and CVSWatchers commands.
"
"                Igor Levko for the patch to recognize numerical sticky tags.
"
"                Domink Strasser for the patch to correct the status line for
"                CVSAdd'd files.
"
"                Weerapong Sirikanya for finding a bug with CVSCommit and
"                autochdir.
"
"                David Gotz for finding a bug with CVSVimDiff buffer splitting
"                and original buffer restoration.
"
"                CJ van den Berg for the patch to not change working directory
"                when editing a non-CVS file.
"
"                Luca Gerli for noticing bad behavior for keywords in files
"                after commit if split windows are used.

" Section: Documentation {{{1
"
" Provides functions to invoke various CVS commands on the current file
" (either the current buffer, or, in the case of an directory buffer, the file
" on the current line).  The output of the commands is captured in a new
" scratch window.  For convenience, if the functions are invoked on a CVS
" output window, the original file is used for the cvs operation instead after
" the window is split.  This is primarily useful when running CVSCommit and
" you need to see the changes made, so that CVSDiff is usable and shows up in
" another window.
"
" Command documentation {{{2
"
" CVSAdd           Performs "cvs add" on the current file.
"
" CVSAnnotate      Performs "cvs annotate" on the current file.  If an
"                  argument is given, the argument is used as a revision
"                  number to display.  If not given an argument, it uses the
"                  most recent version of the file on the current branch.
"                  Additionally, if the current buffer is a CVSAnnotate buffer
"                  already, the version number on the current line is used.
"
"                  If the 'CVSCommandAnnotateParent' variable is set to a
"                  non-zero value, the version previous to the one on the
"                  current line is used instead.  This allows one to navigate
"                  back to examine the previous version of a line.
"
" CVSCommit[!]     If called with arguments, this performs "cvs commit" using
"                  the arguments as the log message.
"
"                  If '!' is used, an empty log message is committed.
"
"                  If called with no arguments, this is a two-step command.
"                  The first step opens a buffer to accept a log message.
"                  When that buffer is written, it is automatically closed and
"                  the file is committed using the information from that log
"                  message.  The commit can be abandoned if the log message
"                  buffer is deleted or wiped before being written.
"
" CVSDiff          With no arguments, this performs "cvs diff" on the current
"                  file.  With one argument, "cvs diff" is performed on the
"                  current file against the specified revision.  With two
"                  arguments, cvs diff is performed between the specified
"                  revisions of the current file.  This command uses the
"                  'CVSCommandDiffOpt' variable to specify diff options.  If
"                  that variable does not exist, then 'wbBc' is assumed.  If
"                  you wish to have no options, then set it to the empty
"                  string.
"
" CVSEdit          Performs "cvs edit" on the current file.
"
" CVSEditors       Performs "cvs editors" on the current file.
"
" CVSGotoOriginal  Returns the current window to the source buffer if the
"                  current buffer is a CVS output buffer.
"
" CVSLog           Performs "cvs log" on the current file.
"
" CVSRevert        Replaces the modified version of the current file with the
"                  most recent version from the repository.
"
" CVSReview        Retrieves a particular version of the current file.  If no
"                  argument is given, the most recent version of the file on
"                  the current branch is retrieved.  The specified revision is
"                  retrieved into a new buffer.
"
" CVSStatus        Performs "cvs status" on the current file.
"
" CVSUnedit        Performs "cvs unedit" on the current file.
"
" CVSUpdate        Performs "cvs update" on the current file.
"
" CVSVimDiff       With no arguments, this prompts the user for a revision and
"                  then uses vimdiff to display the differences between the
"                  current file and the specified revision.  If no revision is
"                  specified, the most recent version of the file on the
"                  current branch is used.  With one argument, that argument
"                  is used as the revision as above.  With two arguments, the
"                  differences between the two revisions is displayed using
"                  vimdiff.
"
"                  With either zero or one argument, the original buffer is used
"                  to perform the vimdiff.  When the other buffer is closed, the
"                  original buffer will be returned to normal mode.
"
"                  Once vimdiff mode is started using the above methods,
"                  additional vimdiff buffers may be added by passing a single
"                  version argument to the command.  There may be up to 4
"                  vimdiff buffers total.
"
"                  Using the 2-argument form of the command resets the vimdiff
"                  to only those 2 versions.  Additionally, invoking the
"                  command on a different file will close the previous vimdiff
"                  buffers.
"
" CVSWatch         Takes an argument which must be one of [on|off|add|remove].
"                  Performs "cvs watch" with the given argument on the current
"                  file.
"
" CVSWatchers      Performs "cvs watchers" on the current file.
"
" CVSWatchAdd      Alias for "CVSWatch add"
"
" CVSWatchOn       Alias for "CVSWatch on"
"
" CVSWatchOff      Alias for "CVSWatch off"
"
" CVSWatchRemove   Alias for "CVSWatch remove"
"
" Mapping documentation: {{{2
"
" By default, a mapping is defined for each command.  User-provided mappings
" can be used instead by mapping to <Plug>CommandName, for instance:
"
" nnoremap ,ca <Plug>CVSAdd
"
" The default mappings are as follow:
"
"   <Leader>ca CVSAdd
"   <Leader>cn CVSAnnotate
"   <Leader>cc CVSCommit
"   <Leader>cd CVSDiff
"   <Leader>ce CVSEdit
"   <Leader>ci CVSEditors
"   <Leader>cg CVSGotoOriginal
"   <Leader>cG CVSGotoOriginal!
"   <Leader>cl CVSLog
"   <Leader>cr CVSReview
"   <Leader>cs CVSStatus
"   <Leader>ct CVSUnedit
"   <Leader>cu CVSUpdate
"   <Leader>cv CVSVimDiff
"   <Leader>cwv CVSWatchers
"   <Leader>cwa CVSWatchAdd
"   <Leader>cwn CVSWatchOn
"   <Leader>cwa CVSWatchOff
"   <Leader>cwr CVSWatchRemove
"
" Options documentation: {{{2
"
" Several variables are checked by the script to determine behavior as follow:
"
" CVSCommandAnnotateParent
"   This variable, if set to a non-zero value, causes the zero-argument form
"   of CVSAnnotate when invoked on a CVSAnnotate buffer to go to the version
"   previous to that displayed on the current line.  If not set, it defaults
"   to 0.
"
" CVSCommandCommitOnWrite
"   This variable, if set to a non-zero value, causes the pending cvs commit
"   to take place immediately as soon as the log message buffer is written.
"   If set to zero, only the CVSCommit mapping will cause the pending commit
"   to occur.  If not set, it defaults to 1.
"
" CVSCommandDeleteOnHide
"   This variable, if set to a non-zero value, causes the temporary CVS result
"   buffers to automatically delete themselves when hidden.
"
" CVSCommandDiffOpt
"   This variable, if set, determines the options passed to the diff command
"   of CVS.  If not set, it defaults to 'wbBc'.
"
" CVSCommandDiffSplit
"   This variable overrides the CVSCommandSplit variable, but only for buffers
"   created with CVSVimDiff.
"
" CVSCommandEdit
"   This variable controls whether the original buffer is replaced ('edit') or
"   split ('split').  If not set, it defaults to 'edit'.
"
" CVSCommandEnableBufferSetup
"   This variable, if set to a non-zero value, activates CVS buffer management
"   mode.  This mode means that two buffer variables, 'CVSRevision' and
"   'CVSBranch', are set if the file is CVS-controlled.  This is useful for
"   displaying version information in the status bar.
"
" CVSCommandInteractive
"   This variable, if set to a non-zero value, causes appropriate functions (for
"   the moment, only CVSReview) to query the user for a revision to use
"   instead of the current revision if none is specified.
"
" CVSCommandNameMarker
"   This variable, if set, configures the special attention-getting characters
"   that appear on either side of the cvs buffer type in the buffer name.
"   This has no effect unless 'CVSCommandNameResultBuffers' is set to a true
"   value.  If not set, it defaults to '_'.  
"
" CVSCommandNameResultBuffers
"   This variable, if set to a true value, causes the cvs result buffers to be
"   named in the old way ('<source file name> _<cvs command>_').  If not set
"   or set to a false value, the result buffer is nameless.
"
" CVSCommandSplit
"   This variable controls the orientation of the various window splits that
"   may occur (such as with CVSVimDiff, when using a CVS command on a CVS
"   command buffer, or when the 'CVSCommandEdit' variable is set to 'split'.
"   If set to 'horizontal', the resulting windows will be on stacked on top of
"   one another.  If set to 'vertical', the resulting windows will be
"   side-by-side.  If not set, it defaults to 'horizontal' for all but
"   CVSVimDiff windows.
"
" Event documentation {{{2
"   For additional customization, cvscommand.vim uses User event autocommand
"   hooks.  Each event is in the CVSCommand group, and different patterns
"   match the various hooks.
"
"   For instance, the following could be added to the vimrc to provide a 'q'
"   mapping to quit a CVS buffer:
"
"   augroup CVSCommand
"     au CVSCommand User CVSBufferCreated silent! nmap <unique> <buffer> q :bwipeout<cr> 
"   augroup END
"
"   The following hooks are available:
"
"   CVSBufferCreated           This event is fired just after a cvs command
"                              result buffer is created and filled with the
"                              result of a cvs command.  It is executed within
"                              the context of the new buffer.
"
"   CVSBufferSetup             This event is fired just after CVS buffer setup
"                              occurs, if enabled.
"
"   CVSPluginInit              This event is fired when the CVSCommand plugin
"                              first loads.
"
"   CVSPluginFinish            This event is fired just after the CVSCommand
"                              plugin loads.
"
"   CVSVimDiffFinish           This event is fired just after the CVSVimDiff
"                              command executes to allow customization of,
"                              for instance, window placement and focus.
"
" Section: Plugin header {{{1

" loaded_cvscommand is set to 1 when the initialization begins, and 2 when it
" completes.  This allows various actions to only be taken by functions after
" system initialization.

if exists("loaded_cvscommand")
   finish
endif
let loaded_cvscommand = 1

if v:version < 602
  echohl WarningMsg|echomsg "CVSCommand 1.69 or later requires VIM 6.2 or later"|echohl None
  finish
endif

" Section: Event group setup {{{1

augroup CVSCommand
augroup END

" Section: Plugin initialization {{{1
silent do CVSCommand User CVSPluginInit

" Section: Script variable initialization {{{1

let s:CVSCommandEditFileRunning = 0
unlet! s:vimDiffRestoreCmd
unlet! s:vimDiffSourceBuffer
unlet! s:vimDiffBufferCount
unlet! s:vimDiffScratchList

" Section: Utility functions {{{1

" Function: s:CVSResolveLink() {{{2
" Fully resolve the given file name to remove shortcuts or symbolic links.

function! s:CVSResolveLink(fileName)
  let resolved = resolve(a:fileName)
  if resolved != a:fileName
    let resolved = s:CVSResolveLink(resolved)
  endif
  return resolved
endfunction

" Function: s:CVSChangeToCurrentFileDir() {{{2
" Go to the directory in which the current CVS-controlled file is located.
" If this is a CVS command buffer, first switch to the original file.

function! s:CVSChangeToCurrentFileDir(fileName)
  let oldCwd=getcwd()
  let fileName=s:CVSResolveLink(a:fileName)
  let newCwd=fnamemodify(fileName, ':h')
  if strlen(newCwd) > 0
    execute 'cd' escape(newCwd, ' ')
  endif
  return oldCwd
endfunction

" Function: s:CVSGetOption(name, default) {{{2
" Grab a user-specified option to override the default provided.  Options are
" searched in the window, buffer, then global spaces.

function! s:CVSGetOption(name, default)
  if exists("s:" . a:name . "Override")
    execute "return s:".a:name."Override"
  elseif exists("w:" . a:name)
    execute "return w:".a:name
  elseif exists("b:" . a:name)
    execute "return b:".a:name
  elseif exists("g:" . a:name)
    execute "return g:".a:name
  else
    return a:default
  endif
endfunction

" Function: s:CVSEditFile(name, origBuffNR) {{{2
" Wrapper around the 'edit' command to provide some helpful error text if the
" current buffer can't be abandoned.  If name is provided, it is used;
" otherwise, a nameless scratch buffer is used.
" Returns: 0 if successful, -1 if an error occurs.

function! s:CVSEditFile(name, origBuffNR)
  "Name parameter will be pasted into expression.
  let name = escape(a:name, ' *?\')

  let editCommand = s:CVSGetOption('CVSCommandEdit', 'edit')
  if editCommand != 'edit'
    if s:CVSGetOption('CVSCommandSplit', 'horizontal') == 'horizontal'
      if name == ""
        let editCommand = 'rightbelow new'
      else
        let editCommand = 'rightbelow split ' . name
      endif
    else
      if name == ""
        let editCommand = 'vert rightbelow new'
      else
        let editCommand = 'vert rightbelow split ' . name
      endif
    endif
  else
    if name == ""
      let editCommand = 'enew'
    else
      let editCommand = 'edit ' . name
    endif
  endif

  " Protect against useless buffer set-up
  let s:CVSCommandEditFileRunning = s:CVSCommandEditFileRunning + 1
  try
    execute editCommand
  finally
    let s:CVSCommandEditFileRunning = s:CVSCommandEditFileRunning - 1
  endtry

  let b:CVSOrigBuffNR=a:origBuffNR
  let b:CVSCommandEdit='split'
endfunction

" Function: s:CVSCreateCommandBuffer(cmd, cmdName, statusText, filename) {{{2
" Creates a new scratch buffer and captures the output from execution of the
" given command.  The name of the scratch buffer is returned.

function! s:CVSCreateCommandBuffer(cmd, cmdName, statusText, origBuffNR)
  let fileName=bufname(a:origBuffNR)

  let resultBufferName=''

  if s:CVSGetOption("CVSCommandNameResultBuffers", 0)
    let nameMarker = s:CVSGetOption("CVSCommandNameMarker", '_')
    if strlen(a:statusText) > 0
      let bufName=a:cmdName . ' -- ' . a:statusText
    else
      let bufName=a:cmdName
    endif
    let bufName=fileName . ' ' . nameMarker . bufName . nameMarker
    let counter=0
    let resultBufferName = bufName
    while buflisted(resultBufferName)
      let counter=counter + 1
      let resultBufferName=bufName . ' (' . counter . ')'
    endwhile
  endif

  let cvsCommand = s:CVSGetOption("CVSCommandCVSExec", "cvs") . " " . a:cmd
  let cvsOut = system(cvsCommand)
  " HACK:  diff command does not return proper error codes
  if v:shell_error && a:cmdName != 'cvsdiff'
    if strlen(cvsOut) == 0
      echoerr "CVS command failed"
    else
      echoerr "CVS command failed:  " . cvsOut
    endif
    return -1
  endif
  if strlen(cvsOut) == 0
    " Handle case of no output.  In this case, it is important to check the
    " file status, especially since cvs edit/unedit may change the attributes
    " of the file with no visible output.

    echomsg "No output from CVS command"
    checktime
    return -1
  endif

  if s:CVSEditFile(resultBufferName, a:origBuffNR) == -1
    return -1
  endif

  set buftype=nofile
  set noswapfile
  set filetype=

  if s:CVSGetOption("CVSCommandDeleteOnHide", 0)
    set bufhidden=delete
  endif

  silent 0put=cvsOut

  " The last command left a blank line at the end of the buffer.  If the
  " last line is folded (a side effect of the 'put') then the attempt to
  " remove the blank line will kill the last fold.
  "
  " This could be fixed by explicitly detecting whether the last line is
  " within a fold, but I prefer to simply unfold the result buffer altogether.

  if has('folding')
    normal zR
  endif

  $d
  1

  " Define the environment and execute user-defined hooks.

  let b:CVSSourceFile=fileName
  let b:CVSCommand=a:cmdName
  if a:statusText != ""
    let b:CVSStatusText=a:statusText
  endif

  silent do CVSCommand User CVSBufferCreated
  return bufnr("%")
endfunction

" Function: s:CVSBufferCheck(cvsBuffer) {{{2
" Attempts to locate the original file to which CVS operations were applied
" for a given buffer.

function! s:CVSBufferCheck(cvsBuffer)
  let origBuffer = getbufvar(a:cvsBuffer, "CVSOrigBuffNR")
  if origBuffer
    if bufexists(origBuffer)
      return origBuffer
    else
      " Original buffer no longer exists.
      return -1 
    endif
  else
    " No original buffer
    return a:cvsBuffer
  endif
endfunction

" Function: s:CVSCurrentBufferCheck() {{{2
" Attempts to locate the original file to which CVS operations were applied
" for the current buffer.

function! s:CVSCurrentBufferCheck()
  return s:CVSBufferCheck(bufnr("%"))
endfunction

" Function: s:CVSToggleDeleteOnHide() {{{2
" Toggles on and off the delete-on-hide behavior of CVS buffers

function! s:CVSToggleDeleteOnHide()
  if exists("g:CVSCommandDeleteOnHide")
    unlet g:CVSCommandDeleteOnHide
  else
    let g:CVSCommandDeleteOnHide=1
  endif
endfunction

" Function: s:CVSDoCommand(cvscmd, cmdName, statusText) {{{2
" General skeleton for CVS function execution.
" Returns: name of the new command buffer containing the command results

function! s:CVSDoCommand(cmd, cmdName, statusText)
  let cvsBufferCheck=s:CVSCurrentBufferCheck()
  if cvsBufferCheck == -1 
    echo "Original buffer no longer exists, aborting."
    return -1
  endif

  let fileName=bufname(cvsBufferCheck)
  if isdirectory(fileName)
    let fileName=fileName . "/" . getline(".")
  endif
  let realFileName = fnamemodify(s:CVSResolveLink(fileName), ':t')
  let oldCwd=s:CVSChangeToCurrentFileDir(fileName)
  try
    if !filereadable('CVS/Root')
      throw fileName . ' is not a CVS-controlled file.'
    endif
    let fullCmd = a:cmd . ' "' . realFileName . '"'
    let resultBuffer=s:CVSCreateCommandBuffer(fullCmd, a:cmdName, a:statusText, cvsBufferCheck)
    return resultBuffer
  catch
    echoerr v:exception
    return -1
  finally
    execute 'cd' escape(oldCwd, ' ')
  endtry
endfunction


" Function: s:CVSGetStatusVars(revision, branch, repository) {{{2
"
" Obtains a CVS revision number and branch name.  The 'revisionVar',
" 'branchVar'and 'repositoryVar' arguments, if non-empty, contain the names of variables to hold
" the corresponding results.
"
" Returns: string to be exec'd that sets the multiple return values.

function! s:CVSGetStatusVars(revisionVar, branchVar, repositoryVar)
  let cvsBufferCheck=s:CVSCurrentBufferCheck()
  if cvsBufferCheck == -1 
    return ""
  endif
  let fileName=bufname(cvsBufferCheck)
  let realFileName = fnamemodify(s:CVSResolveLink(fileName), ':t')
  let oldCwd=s:CVSChangeToCurrentFileDir(fileName)
  try
    if !filereadable('CVS/Root')
      return ""
    endif
    let cvsCommand = s:CVSGetOption("CVSCommandCVSExec", "cvs") . " status " . escape(realFileName, ' *?\')
    let statustext=system(cvsCommand)
    if(v:shell_error)
      return ""
    endif
    let revision=substitute(statustext, '^\_.*Working revision:\s*\(\d\+\%(\.\d\+\)\+\|New file!\)\_.*$', '\1', "")

    " We can still be in a CVS-controlled directory without this being a CVS
    " file
    if match(revision, '^New file!$') >= 0 
      let revision="NEW"
    elseif match(revision, '^\d\+\.\d\+\%(\.\d\+\.\d\+\)*$') >=0
    else
      return ""
    endif

    let returnExpression = "let " . a:revisionVar . "='" . revision . "'"

    if a:branchVar != ""
      let branch=substitute(statustext, '^\_.*Sticky Tag:\s\+\(\d\+\%(\.\d\+\)\+\|\a[A-Za-z0-9-_]*\|(none)\).*$', '\1', "")
      let returnExpression=returnExpression . " | let " . a:branchVar . "='" . branch . "'"
    endif

    if a:repositoryVar != ""
      let repository=substitute(statustext, '^\_.*Repository revision:\s*\(\d\+\%(\.\d\+\)\+\|New file!\|No revision control file\)\_.*$', '\1', "")
      let repository=substitute(repository, '^New file!\|No revision control file$', 'NEW', "")
      let returnExpression=returnExpression . " | let " . a:repositoryVar . "='" . repository . "'"
    endif

    return returnExpression
  finally
    execute 'cd' escape(oldCwd, ' ')
  endtry
endfunction

" Function: s:CVSSetupBuffer() {{{2
" Attempts to set the b:CVSBranch, b:CVSRevision and b:CVSRepository variables.

function! s:CVSSetupBuffer()
  if (exists("b:CVSBufferSetup") && b:CVSBufferSetup)
    " This buffer is already set up.
    return
  endif

  if !s:CVSGetOption("CVSCommandEnableBufferSetup", 0)
        \ || @% == ""
        \ || s:CVSCommandEditFileRunning > 0
        \ || exists("b:CVSOrigBuffNR")
    unlet! b:CVSRevision
    unlet! b:CVSBranch
    unlet! b:CVSRepository
    return
  endif

  if !filereadable(expand("%"))
    return -1
  endif

  let revision=""
  let branch=""
  let repository=""

  exec s:CVSGetStatusVars('revision', 'branch', 'repository')
  if revision != ""
    let b:CVSRevision=revision
  else
    unlet! b:CVSRevision
  endif
  if branch != ""
    let b:CVSBranch=branch
  else
    unlet! b:CVSBranch
  endif
  if repository != ""
     let b:CVSRepository=repository
  else
     unlet! b:CVSRepository
  endif
  silent do CVSCommand User CVSBufferSetup
  let b:CVSBufferSetup=1
endfunction

" Function: s:CVSMarkOrigBufferForSetup(cvsbuffer) {{{2
" Resets the buffer setup state of the original buffer for a given CVS buffer.
" Returns:  The CVS buffer number in a passthrough mode.

function! s:CVSMarkOrigBufferForSetup(cvsBuffer)
  checktime
  if a:cvsBuffer != -1
    let origBuffer = s:CVSBufferCheck(a:cvsBuffer)
    "This should never not work, but I'm paranoid
    if origBuffer != a:cvsBuffer
      call setbufvar(origBuffer, "CVSBufferSetup", 0)
    endif
  endif
  return a:cvsBuffer
endfunction

" Function: s:CVSOverrideOption(option, [value]) {{{2
" Provides a temporary override for the given CVS option.  If no value is
" passed, the override is disabled.

function! s:CVSOverrideOption(option, ...)
  if a:0 == 0
    unlet! s:{a:option}Override
  else
    let s:{a:option}Override = a:1
  endif
endfunction

" Function: s:CVSWipeoutCommandBuffers() {{{2
" Clears all current CVS buffers of the specified type for a given source.

function! s:CVSWipeoutCommandBuffers(originalBuffer, cvsCommand)
  let buffer = 1
  while buffer <= bufnr('$')
    if getbufvar(buffer, 'CVSOrigBuffNR') == a:originalBuffer
      if getbufvar(buffer, 'CVSCommand') == a:cvsCommand
        execute 'bw' buffer
      endif
    endif
    let buffer = buffer + 1
  endwhile
endfunction

" Section: Public functions {{{1

" Function: CVSGetRevision() {{{2
" Global function for retrieving the current buffer's CVS revision number.
" Returns: Revision number or an empty string if an error occurs.

function! CVSGetRevision()
  let revision=""
  exec s:CVSGetStatusVars('revision', '', '')
  return revision
endfunction

" Function: CVSDisableBufferSetup() {{{2
" Global function for deactivating the buffer autovariables.

function! CVSDisableBufferSetup()
  let g:CVSCommandEnableBufferSetup=0
  silent! augroup! CVSCommandPlugin
endfunction

" Function: CVSEnableBufferSetup() {{{2
" Global function for activating the buffer autovariables.

function! CVSEnableBufferSetup()
  let g:CVSCommandEnableBufferSetup=1
  augroup CVSCommandPlugin
    au!
    au BufEnter * call s:CVSSetupBuffer()
  augroup END

  " Only auto-load if the plugin is fully loaded.  This gives other plugins a
  " chance to run.
  if g:loaded_cvscommand == 2
    call s:CVSSetupBuffer()
  endif
endfunction

" Function: CVSGetStatusLine() {{{2
" Default (sample) status line entry for CVS files.  This is only useful if
" CVS-managed buffer mode is on (see the CVSCommandEnableBufferSetup variable
" for how to do this).

function! CVSGetStatusLine()
  if exists('b:CVSSourceFile')
    " This is a result buffer
    let value='[' . b:CVSCommand . ' ' . b:CVSSourceFile
    if exists('b:CVSStatusText')
      let value=value . ' ' . b:CVSStatusText
    endif
    let value = value . ']'
    return value
  endif

  if exists('b:CVSRevision')
        \ && b:CVSRevision != ''
        \ && exists('b:CVSBranch')
        \ && b:CVSBranch != ''
        \ && exists('b:CVSRepository')
        \ && b:CVSRepository != ''
        \ && exists('g:CVSCommandEnableBufferSetup')
        \ && g:CVSCommandEnableBufferSetup
    if b:CVSRevision == b:CVSRepository
      return '[CVS ' . b:CVSBranch . '/' . b:CVSRevision . ']'
    else
      return '[CVS ' . b:CVSBranch . '/' . b:CVSRevision . '/' . b:CVSRepository . ']'
    endif
  else
    return ''
  endif
endfunction

" Section: CVS command functions {{{1

" Function: s:CVSAdd() {{{2
function! s:CVSAdd()
  return s:CVSMarkOrigBufferForSetup(s:CVSDoCommand('add', 'cvsadd', ''))
endfunction

" Function: s:CVSAnnotate(...) {{{2
function! s:CVSAnnotate(...)
  if a:0 == 0
    if &filetype == "CVSAnnotate"
      " This is a CVSAnnotate buffer.  Perform annotation of the version
      " indicated by the current line.
      let revision = substitute(getline("."),'\(^[0-9.]*\).*','\1','')
      let revmin = substitute(revision,'^[0-9.]*\.\([0-9]\+\)','\1','')
      let revmaj = substitute(revision,'^\([0-9.]*\)\.[0-9]\+','\1','')
      if s:CVSGetOption('CVSCommandAnnotateParent', 0) != 0
        let revmin = revmin - 1
      endif
      if revmin == 0
        " Jump to ancestor branch
        let revision = substitute(revmaj,'^\([0-9.]*\)\.[0-9]\+','\1','')
      else
        let revision=revmaj . "." .  revmin
      endif
    else
      let revision=CVSGetRevision()
      if revision == ""
        echoerr "Unable to obtain CVS version information."
        return -1
      endif
    endif
  else
    let revision=a:1
  endif

  if revision == "NEW"
    echo "No annotatation available for new file."
    return -1
  endif

  let resultBuffer=s:CVSDoCommand('-q annotate -r ' . revision, 'cvsannotate', revision) 
  if resultBuffer !=  -1
    set filetype=CVSAnnotate
    " Remove header lines from standard error
    silent v/^\d\+\%(\.\d\+\)\+/d
  endif

  return resultBuffer
endfunction

" Function: s:CVSCommit() {{{2
function! s:CVSCommit(...)
  " Handle the commit message being specified.  If a message is supplied, it
  " is used; if bang is supplied, an empty message is used; otherwise, the
  " user is provided a buffer from which to edit the commit message.
  if a:2 != "" || a:1 == "!"
    return s:CVSMarkOrigBufferForSetup(s:CVSDoCommand('commit -m "' . a:2 . '"', 'cvscommit', ''))
  endif

  let cvsBufferCheck=s:CVSCurrentBufferCheck()
  if cvsBufferCheck ==  -1
    echo "Original buffer no longer exists, aborting."
    return -1
  endif

  " Protect against windows' backslashes in paths.  They confuse exec'd
  " commands.

  let shellSlashBak = &shellslash
  try
    set shellslash

    let messageFileName = tempname()

    let fileName=bufname(cvsBufferCheck)
    let realFilePath=s:CVSResolveLink(fileName)
    let newCwd=fnamemodify(realFilePath, ':h')
    if strlen(newCwd) == 0
      " Account for autochdir being in effect, which will make this blank, but
      " we know we'll be in the current directory for the original file.
      let newCwd = getcwd()
    endif

    let realFileName=fnamemodify(realFilePath, ':t')

    if s:CVSEditFile(messageFileName, cvsBufferCheck) == -1
      return
    endif

    " Protect against case and backslash issues in Windows.
    let autoPattern = '\c' . messageFileName

    " Ensure existance of group
    augroup CVSCommit
    augroup END

    execute 'au CVSCommit BufDelete' autoPattern 'call delete("' . messageFileName . '")'
    execute 'au CVSCommit BufDelete' autoPattern 'au! CVSCommit * ' autoPattern

    " Create a commit mapping.  The mapping must clear all autocommands in case
    " it is invoked when CVSCommandCommitOnWrite is active, as well as to not
    " invoke the buffer deletion autocommand.

    execute 'nnoremap <silent> <buffer> <Plug>CVSCommit '.
          \ ':au! CVSCommit * ' . autoPattern . '<CR>'.
          \ ':g/^CVS:/d<CR>'.
          \ ':update<CR>'.
          \ ':call <SID>CVSFinishCommit("' . messageFileName . '",' .
          \                             '"' . newCwd . '",' .
          \                             '"' . realFileName . '",' .
          \                             cvsBufferCheck . ')<CR>'

    silent 0put ='CVS: ----------------------------------------------------------------------'
    silent put =\"CVS: Enter Log.  Lines beginning with `CVS:' are removed automatically\"
    silent put ='CVS: Type <leader>cc (or your own <Plug>CVSCommit mapping)'

    if s:CVSGetOption('CVSCommandCommitOnWrite', 1) == 1
      execute 'au CVSCommit BufWritePre' autoPattern 'g/^CVS:/d'
      execute 'au CVSCommit BufWritePost' autoPattern 'call s:CVSFinishCommit("' . messageFileName . '", "' . newCwd . '", "' . realFileName . '", ' . cvsBufferCheck . ') | au! * ' autoPattern
      silent put ='CVS: or write this buffer'
    endif

    silent put ='CVS: to finish this commit operation'
    silent put ='CVS: ----------------------------------------------------------------------'
    $
    let b:CVSSourceFile=fileName
    let b:CVSCommand='CVSCommit'
    set filetype=cvs
  finally
    let &shellslash = shellSlashBak
  endtry

endfunction

" Function: s:CVSDiff(...) {{{2
function! s:CVSDiff(...)
  if a:0 == 1
    let revOptions = '-r' . a:1
    let caption = a:1 . ' -> current'
  elseif a:0 == 2
    let revOptions = '-r' . a:1 . ' -r' . a:2
    let caption = a:1 . ' -> ' . a:2
  else
    let revOptions = ''
    let caption = ''
  endif

  let cvsdiffopt=s:CVSGetOption('CVSCommandDiffOpt', 'wbBc')

  if cvsdiffopt == ""
    let diffoptionstring=""
  else
    let diffoptionstring=" -" . cvsdiffopt . " "
  endif

  let resultBuffer = s:CVSDoCommand('diff ' . diffoptionstring . revOptions , 'cvsdiff', caption)
  if resultBuffer != -1 
    set filetype=diff
  endif
  return resultBuffer
endfunction

" Function: s:CVSEdit() {{{2
function! s:CVSEdit()
  return s:CVSDoCommand('edit', 'cvsedit', '')
endfunction

" Function: s:CVSEditors() {{{2
function! s:CVSEditors()
  return s:CVSDoCommand('editors', 'cvseditors', '')
endfunction

" Function: s:CVSGotoOriginal(["!]) {{{2
function! s:CVSGotoOriginal(...)
  let origBuffNR = s:CVSCurrentBufferCheck()
  if origBuffNR > 0
    let origWinNR = bufwinnr(origBuffNR)
    if origWinNR == -1
      execute 'buffer' origBuffNR
    else
      execute origWinNR . 'wincmd w'
    endif
    if a:0 == 1
      if a:1 == "!"
        let buffnr = 1
        let buffmaxnr = bufnr("$")
        while buffnr <= buffmaxnr
          if getbufvar(buffnr, "CVSOrigBuffNR") == origBuffNR
            execute "bw" buffnr
          endif
          let buffnr = buffnr + 1
        endwhile
      endif
    endif
  endif
endfunction

" Function: s:CVSFinishCommit(messageFile, targetDir, targetFile) {{{2
function! s:CVSFinishCommit(messageFile, targetDir, targetFile, origBuffNR)
  if filereadable(a:messageFile)
    let oldCwd=getcwd()
    if strlen(a:targetDir) > 0
      execute 'cd' escape(a:targetDir, ' ')
    endif
    let resultBuffer=s:CVSCreateCommandBuffer('commit -F "' . a:messageFile . '" "'. a:targetFile . '"', 'cvscommit', '', a:origBuffNR)
    execute 'cd' escape(oldCwd, ' ')
    execute 'bw' escape(a:messageFile, ' *?\')
    silent execute 'call delete("' . a:messageFile . '")'
    return s:CVSMarkOrigBufferForSetup(resultBuffer)
  else
    echoerr "Can't read message file; no commit is possible."
    return -1
  endif
endfunction

" Function: s:CVSLog() {{{2
function! s:CVSLog(...)
  if a:0 == 0
    let versionOption = ""
    let caption = ''
  else
    let versionOption=" -r" . a:1
    let caption = a:1
  endif

  let resultBuffer=s:CVSDoCommand('log' . versionOption, 'cvslog', caption)
  if resultBuffer != ""
    set filetype=rcslog
  endif
  return resultBuffer
endfunction

" Function: s:CVSRevert() {{{2
function! s:CVSRevert()
  return s:CVSMarkOrigBufferForSetup(s:CVSDoCommand('update -C', 'cvsrevert', ''))
endfunction

" Function: s:CVSReview(...) {{{2
function! s:CVSReview(...)
  if a:0 == 0
    let versiontag=""
    if s:CVSGetOption('CVSCommandInteractive', 0)
      let versiontag=input('Revision:  ')
    endif
    if versiontag == ""
      let versiontag="(current)"
      let versionOption=""
    else
      let versionOption=" -r " . versiontag . " "
    endif
  else
    let versiontag=a:1
    let versionOption=" -r " . versiontag . " "
  endif

  let resultBuffer = s:CVSDoCommand('-q update -p' . versionOption, 'cvsreview', versiontag)
  if resultBuffer > 0
    let &filetype=getbufvar(b:CVSOrigBuffNR, '&filetype')
  endif

  return resultBuffer
endfunction

" Function: s:CVSStatus() {{{2
function! s:CVSStatus()
  return s:CVSDoCommand('status', 'cvsstatus', '')
endfunction

" Function: s:CVSUnedit() {{{2
function! s:CVSUnedit()
  return s:CVSDoCommand('unedit', 'cvsunedit', '')
endfunction

" Function: s:CVSUpdate() {{{2
function! s:CVSUpdate()
  return s:CVSMarkOrigBufferForSetup(s:CVSDoCommand('update', 'update', ''))
endfunction

" Function: s:CVSVimDiff(...) {{{2
function! s:CVSVimDiff(...)
  let originalBuffer = s:CVSCurrentBufferCheck()
  let s:CVSCommandEditFileRunning = s:CVSCommandEditFileRunning + 1
  try
    " If there's already a VimDiff'ed window, restore it.
    " There may only be one CVSVimDiff original window at a time.

    if exists("s:vimDiffSourceBuffer") && s:vimDiffSourceBuffer != originalBuffer
      " Clear the existing vimdiff setup by removing the result buffers.
      call s:CVSWipeoutCommandBuffers(s:vimDiffSourceBuffer, 'vimdiff')
    endif

    " Split and diff
    if(a:0 == 2)
      " Reset the vimdiff system, as 2 explicit versions were provided.
      if exists('s:vimDiffSourceBuffer')
        call s:CVSWipeoutCommandBuffers(s:vimDiffSourceBuffer, 'vimdiff')
      endif
      let resultBuffer = s:CVSReview(a:1)
      if resultBuffer < 0
        echomsg "Can't open CVS revision " . a:1
        return resultBuffer
      endif
      let b:CVSCommand = 'vimdiff'
      diffthis
      let s:vimDiffBufferCount = 1
      let s:vimDiffScratchList = '{'. resultBuffer . '}'
      " If no split method is defined, cheat, and set it to vertical.
      try
        call s:CVSOverrideOption('CVSCommandSplit', s:CVSGetOption('CVSCommandDiffSplit', s:CVSGetOption('CVSCommandSplit', 'vertical')))
        let resultBuffer=s:CVSReview(a:2)
      finally
        call s:CVSOverrideOption('CVSCommandSplit')
      endtry
      if resultBuffer < 0
        echomsg "Can't open CVS revision " . a:1
        return resultBuffer
      endif
      let b:CVSCommand = 'vimdiff'
      diffthis
      let s:vimDiffBufferCount = 2
      let s:vimDiffScratchList = s:vimDiffScratchList . '{'. resultBuffer . '}'
    else
      " Add new buffer
      try
        " Force splitting behavior, otherwise why use vimdiff?
        call s:CVSOverrideOption("CVSCommandEdit", "split")
        call s:CVSOverrideOption("CVSCommandSplit", s:CVSGetOption('CVSCommandDiffSplit', s:CVSGetOption('CVSCommandSplit', 'vertical')))
        if(a:0 == 0)
          let resultBuffer=s:CVSReview()
        else
          let resultBuffer=s:CVSReview(a:1)
        endif
      finally
        call s:CVSOverrideOption("CVSCommandEdit")
        call s:CVSOverrideOption("CVSCommandSplit")
      endtry
      if resultBuffer < 0
        echomsg "Can't open current CVS revision"
        return resultBuffer
      endif
      let b:CVSCommand = 'vimdiff'
      diffthis

      if !exists('s:vimDiffBufferCount')
        " New instance of vimdiff.
        let s:vimDiffBufferCount = 2
        let s:vimDiffScratchList = '{' . resultBuffer . '}'

        " This could have been invoked on a CVS result buffer, not the
        " original buffer.
        wincmd W
        execute 'buffer' originalBuffer
        " Store info for later original buffer restore
        let s:vimDiffRestoreCmd = 
              \    "call setbufvar(".originalBuffer.", \"&diff\", ".getbufvar(originalBuffer, '&diff').")"
              \ . "|call setbufvar(".originalBuffer.", \"&foldcolumn\", ".getbufvar(originalBuffer, '&foldcolumn').")"
              \ . "|call setbufvar(".originalBuffer.", \"&foldenable\", ".getbufvar(originalBuffer, '&foldenable').")"
              \ . "|call setbufvar(".originalBuffer.", \"&foldmethod\", '".getbufvar(originalBuffer, '&foldmethod')."')"
              \ . "|call setbufvar(".originalBuffer.", \"&scrollbind\", ".getbufvar(originalBuffer, '&scrollbind').")"
              \ . "|call setbufvar(".originalBuffer.", \"&wrap\", ".getbufvar(originalBuffer, '&wrap').")"
              \ . "|if &foldmethod=='manual'|execute 'normal zE'|endif"
        diffthis
        wincmd w
      else
        " Adding a window to an existing vimdiff
        let s:vimDiffBufferCount = s:vimDiffBufferCount + 1
        let s:vimDiffScratchList = s:vimDiffScratchList . '{' . resultBuffer . '}'
      endif
    endif

    let s:vimDiffSourceBuffer = originalBuffer

    " Avoid executing the modeline in the current buffer after the autocommand.

    let currentBuffer = bufnr('%')
    let saveModeline = getbufvar(currentBuffer, '&modeline')
    try
      call setbufvar(currentBuffer, '&modeline', 0)
      silent do CVSCommand User CVSVimDiffFinish
    finally
      call setbufvar(currentBuffer, '&modeline', saveModeline)
    endtry
    return resultBuffer
  finally
    let s:CVSCommandEditFileRunning = s:CVSCommandEditFileRunning - 1
  endtry
endfunction

" Function: s:CVSWatch(onoff) {{{2
function! s:CVSWatch(onoff)
  if a:onoff !~ '^\c\%(on\|off\|add\|remove\)$'
    echoerr "Argument to CVSWatch must be one of [on|off|add|remove]"
    return -1
  end
  return s:CVSDoCommand('watch ' . tolower(a:onoff), 'cvswatch', '')
endfunction

" Function: s:CVSWatchers() {{{2
function! s:CVSWatchers()
  return s:CVSDoCommand('watchers', 'cvswatchers', '')
endfunction

" Section: Command definitions {{{1
" Section: Primary commands {{{2
com! CVSAdd call s:CVSAdd()
com! -nargs=? CVSAnnotate call s:CVSAnnotate(<f-args>)
com! -bang -nargs=? CVSCommit call s:CVSCommit(<q-bang>, <q-args>)
com! -nargs=* CVSDiff call s:CVSDiff(<f-args>)
com! CVSEdit call s:CVSEdit()
com! CVSEditors call s:CVSEditors()
com! -bang CVSGotoOriginal call s:CVSGotoOriginal(<q-bang>)
com! -nargs=? CVSLog call s:CVSLog(<f-args>)
com! CVSRevert call s:CVSRevert()
com! -nargs=? CVSReview call s:CVSReview(<f-args>)
com! CVSStatus call s:CVSStatus()
com! CVSUnedit call s:CVSUnedit()
com! CVSUpdate call s:CVSUpdate()
com! -nargs=* CVSVimDiff call s:CVSVimDiff(<f-args>)
com! -nargs=1 CVSWatch call s:CVSWatch(<f-args>)
com! CVSWatchAdd call s:CVSWatch('add')
com! CVSWatchOn call s:CVSWatch('on')
com! CVSWatchOff call s:CVSWatch('off')
com! CVSWatchRemove call s:CVSWatch('remove')
com! CVSWatchers call s:CVSWatchers()

" Section: CVS buffer management commands {{{2
com! CVSDisableBufferSetup call CVSDisableBufferSetup()
com! CVSEnableBufferSetup call CVSEnableBufferSetup()

" Allow reloading cvscommand.vim
com! CVSReload unlet! loaded_cvscommand | runtime plugin/cvscommand.vim

" Section: Plugin command mappings {{{1
nnoremap <silent> <Plug>CVSAdd :CVSAdd<CR>
nnoremap <silent> <Plug>CVSAnnotate :CVSAnnotate<CR>
nnoremap <silent> <Plug>CVSCommit :CVSCommit<CR>
nnoremap <silent> <Plug>CVSDiff :CVSDiff<CR>
nnoremap <silent> <Plug>CVSEdit :CVSEdit<CR>
nnoremap <silent> <Plug>CVSEditors :CVSEditors<CR>
nnoremap <silent> <Plug>CVSGotoOriginal :CVSGotoOriginal<CR>
nnoremap <silent> <Plug>CVSClearAndGotoOriginal :CVSGotoOriginal!<CR>
nnoremap <silent> <Plug>CVSLog :CVSLog<CR>
nnoremap <silent> <Plug>CVSRevert :CVSRevert<CR>
nnoremap <silent> <Plug>CVSReview :CVSReview<CR>
nnoremap <silent> <Plug>CVSStatus :CVSStatus<CR>
nnoremap <silent> <Plug>CVSUnedit :CVSUnedit<CR>
nnoremap <silent> <Plug>CVSUpdate :CVSUpdate<CR>
nnoremap <silent> <Plug>CVSVimDiff :CVSVimDiff<CR>
nnoremap <silent> <Plug>CVSWatchers :CVSWatchers<CR>
nnoremap <silent> <Plug>CVSWatchAdd :CVSWatchAdd<CR>
nnoremap <silent> <Plug>CVSWatchOn :CVSWatchOn<CR>
nnoremap <silent> <Plug>CVSWatchOff :CVSWatchOff<CR>
nnoremap <silent> <Plug>CVSWatchRemove :CVSWatchRemove<CR>

" Section: Default mappings {{{1
if !hasmapto('<Plug>CVSAdd')
  nmap <unique> <Leader>ca <Plug>CVSAdd
endif
if !hasmapto('<Plug>CVSAnnotate')
  nmap <unique> <Leader>cn <Plug>CVSAnnotate
endif
if !hasmapto('<Plug>CVSClearAndGotoOriginal')
  nmap <unique> <Leader>cG <Plug>CVSClearAndGotoOriginal
endif
if !hasmapto('<Plug>CVSCommit')
  nmap <unique> <Leader>cc <Plug>CVSCommit
endif
if !hasmapto('<Plug>CVSDiff')
  nmap <unique> <Leader>cd <Plug>CVSDiff
endif
if !hasmapto('<Plug>CVSEdit')
  nmap <unique> <Leader>ce <Plug>CVSEdit
endif
if !hasmapto('<Plug>CVSEditors')
  nmap <unique> <Leader>ci <Plug>CVSEditors
endif
if !hasmapto('<Plug>CVSGotoOriginal')
  nmap <unique> <Leader>cg <Plug>CVSGotoOriginal
endif
if !hasmapto('<Plug>CVSLog')
  nmap <unique> <Leader>cl <Plug>CVSLog
endif
if !hasmapto('<Plug>CVSRevert')
  nmap <unique> <Leader>cq <Plug>CVSRevert
endif
if !hasmapto('<Plug>CVSReview')
  nmap <unique> <Leader>cr <Plug>CVSReview
endif
if !hasmapto('<Plug>CVSStatus')
  nmap <unique> <Leader>cs <Plug>CVSStatus
endif
if !hasmapto('<Plug>CVSUnedit')
  nmap <unique> <Leader>ct <Plug>CVSUnedit
endif
if !hasmapto('<Plug>CVSUpdate')
  nmap <unique> <Leader>cu <Plug>CVSUpdate
endif
if !hasmapto('<Plug>CVSVimDiff')
  nmap <unique> <Leader>cv <Plug>CVSVimDiff
endif
if !hasmapto('<Plug>CVSWatchers')
  nmap <unique> <Leader>cwv <Plug>CVSWatchers
endif
if !hasmapto('<Plug>CVSWatchAdd')
  nmap <unique> <Leader>cwa <Plug>CVSWatchAdd
endif
if !hasmapto('<Plug>CVSWatchOn')
  nmap <unique> <Leader>cwn <Plug>CVSWatchOn
endif
if !hasmapto('<Plug>CVSWatchOff')
  nmap <unique> <Leader>cwf <Plug>CVSWatchOff
endif
if !hasmapto('<Plug>CVSWatchRemove')
  nmap <unique> <Leader>cwr <Plug>CVSWatchRemove
endif

" Section: Menu items {{{1
silent! aunmenu Plugin.CVS
amenu <silent> &Plugin.CVS.&Add        <Plug>CVSAdd
amenu <silent> &Plugin.CVS.A&nnotate   <Plug>CVSAnnotate
amenu <silent> &Plugin.CVS.&Commit     <Plug>CVSCommit
amenu <silent> &Plugin.CVS.&Diff       <Plug>CVSDiff
amenu <silent> &Plugin.CVS.&Edit       <Plug>CVSEdit
amenu <silent> &Plugin.CVS.Ed&itors    <Plug>CVSEditors
amenu <silent> &Plugin.CVS.&Log        <Plug>CVSLog
amenu <silent> &Plugin.CVS.Revert      <Plug>CVSRevert
amenu <silent> &Plugin.CVS.&Review     <Plug>CVSReview
amenu <silent> &Plugin.CVS.&Status     <Plug>CVSStatus
amenu <silent> &Plugin.CVS.Unedi&t     <Plug>CVSUnedit
amenu <silent> &Plugin.CVS.&Update     <Plug>CVSUpdate
amenu <silent> &Plugin.CVS.&VimDiff    <Plug>CVSVimDiff
amenu <silent> &Plugin.CVS.&Watchers   <Plug>CVSWatchers
amenu <silent> &Plugin.CVS.WatchAdd    <Plug>CVSWatchAdd
amenu <silent> &Plugin.CVS.WatchOn     <Plug>CVSWatchOn
amenu <silent> &Plugin.CVS.WatchOff    <Plug>CVSWatchOff
amenu <silent> &Plugin.CVS.WatchRemove <Plug>CVSWatchRemove

" Section: Autocommands to restore vimdiff state {{{1
function! s:CVSVimDiffRestore(vimDiffBuff)
  let s:CVSCommandEditFileRunning = s:CVSCommandEditFileRunning + 1
  try
    if exists("s:vimDiffSourceBuffer")
      if a:vimDiffBuff == s:vimDiffSourceBuffer
        " Original file is being removed.
        unlet! s:vimDiffSourceBuffer
        unlet! s:vimDiffBufferCount
        unlet! s:vimDiffRestoreCmd
        unlet! s:vimDiffScratchList
      elseif match(s:vimDiffScratchList, '{' . a:vimDiffBuff . '}') >= 0
        let s:vimDiffScratchList = substitute(s:vimDiffScratchList, '{' . a:vimDiffBuff . '}', '', '')
        let s:vimDiffBufferCount = s:vimDiffBufferCount - 1
        if s:vimDiffBufferCount == 1 && exists('s:vimDiffRestoreCmd')
          " All scratch buffers are gone, reset the original.
          " Only restore if the source buffer is still in Diff mode

          let sourceWinNR=bufwinnr(s:vimDiffSourceBuffer)
          if sourceWinNR != -1
            " The buffer is visible in at least one window
            let currentWinNR = winnr()
            while winbufnr(sourceWinNR) != -1
              if winbufnr(sourceWinNR) == s:vimDiffSourceBuffer
                execute sourceWinNR . 'wincmd w'
                if getwinvar('', "&diff")
                  execute s:vimDiffRestoreCmd
                endif
              endif
              let sourceWinNR = sourceWinNR + 1
            endwhile
            execute currentWinNR . 'wincmd w'
          else
            " The buffer is hidden.  It must be visible in order to set the
            " diff option.
            let currentBufNR = bufnr('')
            execute "hide buffer" s:vimDiffSourceBuffer
            if getwinvar('', "&diff")
              execute s:vimDiffRestoreCmd
            endif
            execute "hide buffer" currentBufNR
          endif

          unlet s:vimDiffRestoreCmd
          unlet s:vimDiffSourceBuffer
          unlet s:vimDiffBufferCount
          unlet s:vimDiffScratchList
        elseif s:vimDiffBufferCount == 0
          " All buffers are gone.
          unlet s:vimDiffSourceBuffer
          unlet s:vimDiffBufferCount
          unlet s:vimDiffScratchList
        endif
      endif
    endif
  finally
    let s:CVSCommandEditFileRunning = s:CVSCommandEditFileRunning - 1
  endtry
endfunction

augroup CVSVimDiffRestore
  au!
  au BufUnload * call s:CVSVimDiffRestore(expand("<abuf>"))
augroup END

" Section: Optional activation of buffer management {{{1

if s:CVSGetOption('CVSCommandEnableBufferSetup', 0)
  call CVSEnableBufferSetup()
endif

" Section: Plugin completion {{{1

let loaded_cvscommand=2
silent do CVSCommand User CVSPluginFinish

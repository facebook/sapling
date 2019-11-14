" vim600: set foldmethod=marker:
" =============================================================================
"  Name Of File: hg-menu.vim
"   Description: Interface to Mercurial Version Control.
"        Author: Steve Borho (modified Jeff Lanzarotta's RCS script)
"          Date: Wednesday, October 5, 2005
"       Version: 0.1.0
"     Copyright: None.
"         Usage: These command and gui menu displays useful hg functions
" Configuration: Your hg executable must be in your path.
" =============================================================================

" Section: Init {{{1
if exists("loaded_hg_menu")
  finish
endif
let loaded_hg_menu = 1

" Section: Menu Options {{{1
if has("gui")
"  amenu H&G.Commit\ File<Tab>,ci :!hg commit %<CR>:e!<CR>
"  amenu H&G.Commit\ All<Tab>,call :!hg commit<CR>:e!<CR>
"  amenu H&G.-SEP1-        <nul>
  amenu H&G.Add<Tab>\\add :!hg add %<CR><CR>
  amenu H&G.Forget\ Add<Tab>\\fgt :!hg forget %<CR><CR>
  amenu H&G.Show\ Differences<Tab>\\diff :call ShowResults("FileDiff", "hg\ diff")<CR><CR>
  amenu H&G.Revert\ to\ Last\ Version<Tab>\\revert :!hg revert %<CR>:e!<CR>
  amenu H&G.Show\ History<Tab>\\log :call ShowResults("FileLog", "hg\ log")<CR><CR>
  amenu H&G.Annotate<Tab>\\an :call ShowResults("annotate", "hg\ annotate")<CR><CR>
  amenu H&G.-SEP1-        <nul>
  amenu H&G.Repo\ Status<Tab>\\stat :call ShowResults("RepoStatus", "hg\ status")<CR><CR>
  amenu H&G.Pull<Tab>\\pull :!hg pull<CR>:e!<CR>
  amenu H&G.Update<Tab>\\upd :!hg update<CR>:e!<CR>
endif

" Section: Mappings {{{1
if(v:version >= 600)
  " The default Leader is \ 'backslash'
  map <Leader>add       :!hg add %<CR><CR>
  map <Leader>fgt       :!hg forget %<CR><CR>
  map <Leader>diff      :call ShowResults("FileDiff", "hg\ diff")<CR><CR>
  map <Leader>revert    :!hg revert %<CR>:e!<CR>
  map <Leader>log       :call ShowResults("FileLog", "hg\ log")<CR><CR>
  map <Leader>an        :call ShowResults("annotate", "hg\ annotate")<CR><CR>
  map <Leader>stat      :call ShowResults("RepoStatus", "hg\ status")<CR><CR>
  map <Leader>upd       :!hg update<CR>:e!<CR>
  map <Leader>pull      :!hg pull<CR>:e!<CR>
else
  " pre 6.0, the default Leader was a comma
  map ,add          :!hg add %<CR><CR>
  map ,fgt          :!hg forget %<CR><CR>
  map ,diff         :call ShowResults("FileDiff", "hg\ diff")<CR><CR>
  map ,revert       :!hg revert<CR>:e!<CR>
  map ,log          :call ShowResults("FileLog", "hg\ log")<CR><CR>
  map ,an           :call ShowResults("annotate", "hg\ annotate")<CR><CR>
  map ,stat         :call ShowResults("RepoStatus", "hg\ status")<CR><CR>
  map ,upd          :!hg update<CR>:e!<CR>
  map ,pull         :!hg pull<CR>:e!<CR>
endif

" Section: Functions {{{1
" Show the log results of the current file with a revision control system.
function! ShowResults(bufferName, cmdName)
  " Modify the shortmess option:
  " A  don't give the "ATTENTION" message when an existing swap file is
  "    found.
  set shortmess+=A

  " Get the name of the current buffer.
  let currentBuffer = bufname("%")

  " If a buffer with the name rlog exists, delete it.
  if bufexists(a:bufferName)
    execute 'bd! ' a:bufferName
  endif

  " Create a new buffer.
  execute 'new ' a:bufferName

  " Execute the command.
  execute 'r!' a:cmdName ' ' currentBuffer

  " Make is so that the file can't be edited.
  setlocal nomodified
  setlocal nomodifiable
  setlocal readonly

  " Go to the beginning of the buffer.
  execute "normal 1G"

  " Restore the shortmess option.
  set shortmess-=A
endfunction

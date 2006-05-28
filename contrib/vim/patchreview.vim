" Vim global plugin for doing single or multipatch code reviews"{{{

" Version       : 0.1                                          "{{{
" Last Modified : Thu 25 May 2006 10:15:11 PM PDT
" Author        : Manpreet Singh (junkblocker AT yahoo DOT com)
" Copyright     : 2006 by Manpreet Singh
" License       : This file is placed in the public domain.
"
" History       : 0.1 - First released
"}}}
" Documentation:                                                         "{{{
" ===========================================================================
" This plugin allows single or multipatch code reviews to be done in VIM. Vim
" has :diffpatch command to do single file reviews but can not handle patch
" files containing multiple patches. This plugin provides that missing
" functionality and doesn't require the original file to be open.
"
" Installing:                                                            "{{{
"
"  For a quick start...
"
"   Requirements:                                                        "{{{
"
"   1) (g)vim 7.0 or higher built with +diff option.
"   2) patch and patchutils ( http://cyberelk.net/tim/patchutils/ ) installed
"      for your OS. For windows it is availble from Cygwin (
"      http://www.cygwin.com ) or GnuWin32 ( http://gnuwin32.sourceforge.net/
"      ).
""}}}
"   Install:                                                            "{{{
"
"   1) Extract this in your $VIM/vimfiles or $HOME/.vim directory and restart
"      vim.
"
"   2) Make sure that you have filterdiff from patchutils and patch commands
"      installed.
"
"   3) Optinally, specify the locations to filterdiff and patch commands and
"      location of a temporary directory to use in your .vimrc.
"
"      let g:patchreview_filterdiff  = '/path/to/filterdiff'
"      let g:patchreview_patch       = '/path/to/patch'
"      let g:patchreview_tmpdir      = '/tmp/or/something'
"
"   4) Optionally, generate help tags to use help
"
"      :helptags ~/.vim/doc
"      or
"      :helptags c:\vim\vimfiles\doc
""}}}
""}}}
" Usage:                                                                 "{{{
"
"  :PatchReview path_to_submitted_patchfile [optional_source_directory]
"
"  after review is done
"
"  :PatchReviewCleanup
"
" See :help patchreview for details after you've created help tags.
""}}}
"}}}
" Code                                                                   "{{{

" Enabled only during development                                        "{{{
" unlet! g:loaded_patchreview " DEBUG
" unlet! g:patchreview_tmpdir " DEBUG
" unlet! g:patchreview_filterdiff " DEBUG
" unlet! g:patchreview_patch " DEBUG
"}}}

" load only once                                                         "{{{
if exists('g:loaded_patchreview')
  finish
endif
let g:loaded_patchreview=1
let s:msgbufname = 'Patch Review Messages'
"}}}

function! <SID>PR_wipeMsgBuf()                                           "{{{
  let s:winnum = bufwinnr(s:msgbufname)
  if s:winnum != -1 " If the window is already open, jump to it
    let s:cur_winnr = winnr()
    if winnr() != s:winnum
      exe s:winnum . 'wincmd w'
      exe 'bw'
      exe s:cur_winnr . 'wincmd w'
    endif
  endif
endfunction
"}}}

function! <SID>PR_echo(...)                                              "{{{
  " Usage: PR_echo(msg, [return_to_original_window_flag])
  "            default return_to_original_window_flag = 0
  "
  let s:cur_winnr = winnr()
  let s:winnum = bufwinnr(s:msgbufname)
  if s:winnum != -1 " If the window is already open, jump to it
    if winnr() != s:winnum
      exe s:winnum . 'wincmd w'
    endif
  else
    let s:bufnum = bufnr(s:msgbufname)
    if s:bufnum == -1
      let s:wcmd = s:msgbufname
    else
      let s:wcmd = '+buffer' . s:bufnum
    endif
    exe 'silent! botright 5split ' . s:wcmd
  endif
  setlocal modifiable
  setlocal buftype=nofile
  setlocal bufhidden=delete
  setlocal noswapfile
  setlocal nowrap
  setlocal nobuflisted
  if a:0 != 0
    silent! $put =a:1
  endif
  exe ':$'
  setlocal nomodifiable
  if a:0 > 1 && a:2
    exe s:cur_winnr . 'wincmd w'
  endif
endfunction
"}}}

function! <SID>PR_checkBinary(BinaryName)                                "{{{
  " Verify that BinaryName is specified or available
  if ! exists('g:patchreview_' . a:BinaryName)
    if executable(a:BinaryName)
      let g:patchreview_{a:BinaryName} = a:BinaryName
      return 1
    else
      call s:PR_echo('g:patchreview_' . a:BinaryName . ' is not defined and could not be found on path. Please define it in your .vimrc.')
      return 0
    endif
  elseif ! executable(g:patchreview_{a:BinaryName})
    call s:PR_echo('Specified g:patchreview_' . a:BinaryName . ' [' . g:patchreview_{a.BinaryName} . '] is not executable.')
    return 0
  else
    return 1
  endif
endfunction
"}}}

function! <SID>PR_GetTempDirLocation(Quiet)                              "{{{
  if exists('g:patchreview_tmpdir')
    if ! isdirectory(g:patchreview_tmpdir) || ! filewritable(g:patchreview_tmpdir)
      if ! a:Quiet
        call s:PR_echo('Temporary directory specified by g:patchreview_tmpdir [' . g:patchreview_tmpdir . '] is not accessible.')
        return 0
      endif
    endif
  elseif exists("$TMP") && isdirectory($TMP) && filewritable($TMP)
    let g:patchreview_tmpdir = $TMP
  elseif exists("$TEMP") && isdirectory($TEMP) && filewritable($TEMP)
    let g:patchreview_tmpdir = $TEMP
  elseif exists("$TMPDIR") && isdirectory($TMPDIR) && filewritable($TMPDIR)
    let g:patchreview_tmpdir = $TMPDIR
  else
    if ! a:Quiet
      call s:PR_echo('Could not figure out a temporary directory to use. Please specify g:patchreview_tmpdir in your .vimrc.')
      return 0
    endif
  endif
  let g:patchreview_tmpdir = g:patchreview_tmpdir . '/'
  let g:patchreview_tmpdir = substitute(g:patchreview_tmpdir, '\\', '/', 'g')
  let g:patchreview_tmpdir = substitute(g:patchreview_tmpdir, '/+$', '/', '')
  if has('win32')
    let g:patchreview_tmpdir = substitute(g:patchreview_tmpdir, '/', '\\', 'g')
  endif
  return 1
endfunction
"}}}

function! <SID>PatchReview(...)                                          "{{{
  " VIM 7+ required"{{{
  if version < 700
    call s:PR_echo('This plugin needs VIM 7 or higher')
    return
  endif
"}}}

  let s:save_shortmess = &shortmess
  set shortmess+=aW
  call s:PR_wipeMsgBuf()

  " Check passed arguments                                               "{{{
  if a:0 == 0
    call s:PR_echo('PatchReview command needs at least one argument specifying a patchfile path.')
    let &shortmess = s:save_shortmess
    return
  endif
  if a:0 >= 1 && a:0 <= 2
    let s:PatchFilePath = expand(a:1, ':p')
    if ! filereadable(s:PatchFilePath)
      call s:PR_echo('File [' . s:PatchFilePath . '] is not accessible.')
      let &shortmess = s:save_shortmess
      return
    endif
    if a:0 == 2
      let s:SrcDirectory = expand(a:2, ':p')
      if ! isdirectory(s:SrcDirectory)
        call s:PR_echo('[' . s:SrcDirectory . '] is not a directory')
        let &shortmess = s:save_shortmess
        return
      endif
      try
        exe 'cd ' . s:SrcDirectory
      catch /^.*E344.*/
        call s:PR_echo('Could not change to directory [' . s:SrcDirectory . ']')
        let &shortmess = s:save_shortmess
        return
      endtry
    endif
  else
    call s:PR_echo('PatchReview command needs at most two arguments: patchfile path and optional source directory path.')
    let &shortmess = s:save_shortmess
    return
  endif
"}}}

  " Verify that filterdiff and patch are specified or available          "{{{
  if ! s:PR_checkBinary('filterdiff') || ! s:PR_checkBinary('patch')
    let &shortmess = s:save_shortmess
    return
  endif

  let s:retval = s:PR_GetTempDirLocation(0)
  if ! s:retval
    let &shortmess = s:save_shortmess
    return
  endif
"}}}

  " Requirements met, now execute                                        "{{{
  let s:PatchFilePath = fnamemodify(s:PatchFilePath, ':p')
  call s:PR_echo('Patch file      : ' . s:PatchFilePath)
  call s:PR_echo('Source directory: ' . getcwd())
  call s:PR_echo('------------------')
  let s:theFilterDiffCommand = '' . g:patchreview_filterdiff . ' --list -s ' . s:PatchFilePath
  let s:theFilesString = system(s:theFilterDiffCommand)
  let s:theFilesList = split(s:theFilesString, '[\r\n]')
  for s:filewithchangetype in s:theFilesList
    if s:filewithchangetype !~ '^[!+-] '
      call s:PR_echo('*** Skipping review generation due to understood change for [' . s:filewithchangetype . ']', 1)
      continue
    endif
    unlet! s:RelativeFilePath
    let s:RelativeFilePath = substitute(s:filewithchangetype, '^. ', '', '')
    let s:RelativeFilePath = substitute(s:RelativeFilePath, '^[a-z][^\\\/]*[\\\/]' , '' , '')
    if s:filewithchangetype =~ '^! '
      let s:msgtype = 'Modification : '
    elseif s:filewithchangetype =~ '^+ '
      let s:msgtype = 'Addition     : '
    elseif s:filewithchangetype =~ '^- '
      let s:msgtype = 'Deletion     : '
    endif
    let s:bufnum = bufnr(s:RelativeFilePath)
    if buflisted(s:bufnum) && getbufvar(s:bufnum, '&mod')
      call s:PR_echo('Old buffer for file [' . s:RelativeFilePath . '] exists in modified state. Skipping review.', 1)
      continue
    endif
    let s:tmpname = substitute(s:RelativeFilePath, '/', '_', 'g')
    let s:tmpname = substitute(s:tmpname, '\\', '_', 'g')
    let s:tmpname = g:patchreview_tmpdir . 'PatchReview.' . s:tmpname . '.' . strftime('%Y%m%d%H%M%S')
    if has('win32')
      let s:tmpname = substitute(s:tmpname, '/', '\\', 'g')
    endif
    if ! exists('s:patchreview_tmpfiles')
      let s:patchreview_tmpfiles = []
    endif
    let s:patchreview_tmpfiles = s:patchreview_tmpfiles + [s:tmpname]

    let s:filterdiffcmd = '!' . g:patchreview_filterdiff . ' -i ' . s:RelativeFilePath . ' ' . s:PatchFilePath . ' > ' . s:tmpname
    silent! exe s:filterdiffcmd
    if s:filewithchangetype =~ '^+ '
      if has('win32')
        let s:inputfile = 'nul'
      else
        let s:inputfile = '/dev/null'
      endif
    else
      let s:inputfile = expand(s:RelativeFilePath, ':p')
    endif
    silent exe '!' . g:patchreview_patch . ' -o ' . s:tmpname . '.file ' . s:inputfile . ' < ' . s:tmpname
    let s:origtabpagenr = tabpagenr()
    silent! exe 'tabedit ' . s:RelativeFilePath
    silent! exe 'vert diffsplit ' . s:tmpname . '.file'
    if filereadable(s:tmpname . '.file.rej')
      silent! exe 'topleft 5split ' . s:tmpname . '.file.rej'
      call s:PR_echo(s:msgtype . '*** REJECTED *** ' . s:RelativeFilePath, 1)
    else
      call s:PR_echo(s:msgtype . ' ' . s:RelativeFilePath, 1)
    endif
    silent! exe 'tabn ' . s:origtabpagenr
  endfor
  call s:PR_echo('-----')
  call s:PR_echo('Done.')
  let &shortmess = s:save_shortmess
"}}}
endfunction
"}}}

function! <SID>PatchReviewCleanup()                                      "{{{
  let s:retval = s:PR_GetTempDirLocation(1)
  if s:retval && exists('g:patchreview_tmpdir') && isdirectory(g:patchreview_tmpdir) && filewritable(g:patchreview_tmpdir)
    let s:zefilestr = globpath(g:patchreview_tmpdir, 'PatchReview.*')
    let s:theFilesList = split(s:zefilestr, '\m[\r\n]\+')
    for s:thefile in s:theFilesList
      call delete(s:thefile)
    endfor
  endif
endfunction
"}}}

" Commands                                                               "{{{
"============================================================================
" :PatchReview
command! -nargs=* -complete=file PatchReview call s:PatchReview (<f-args>)


" :PatchReviewCleanup
command! -nargs=0 PatchReviewCleanup call s:PatchReviewCleanup ()
"}}}
"}}}

" vim: textwidth=78 nowrap tabstop=2 shiftwidth=2 softtabstop=2 expandtab
" vim: filetype=vim encoding=latin1 fileformat=unix foldlevel=0 foldmethod=marker
"}}}

" Vim syntax file
" Language: Mercurial unified tests
" Author: Steve Losh (steve@stevelosh.com)
"
" Place this file in ~/.vim/syntax/ and add the following line to your
" ~/.vimrc to enable:
" au BufNewFile,BufRead *.t set filetype=hgtest
"
" If you want folding you'll need the following line as well:
" let hgtest_fold=1
"
" You might also want to set the starting foldlevel for hgtest files:
" autocmd Syntax hgtest setlocal foldlevel=1

if exists("b:current_syntax")
  finish
endif

syn include @Shell syntax/sh.vim

syn match hgtestComment /^[^ ].*$/
syn region hgtestOutput start=/^  [^$>]/ start=/^  $/ end=/\v.(\n\n*[^ ])\@=/me=s end=/^  [$>]/me=e-3 end=/^$/ fold containedin=hgtestBlock
syn match hgtestCommandStart /^  \$ / containedin=hgtestCommand
syn region hgtestCommand start=/^  \$ /hs=s+4,rs=s+4 end=/^  [^>]/me=e-3 end=/^  $/me=e-2 containedin=hgtestBlock contains=@Shell keepend
syn region hgtestBlock start=/^  /ms=e-2 end=/\v.(\n\n*[^ ])\@=/me=s end=/^$/me=e-1 fold keepend

hi link hgtestCommandStart Keyword
hi link hgtestComment Normal
hi link hgtestOutput Comment

if exists("hgtest_fold")
  setlocal foldmethod=syntax
endif

syn sync match hgtestSync grouphere NONE "^$"
syn sync maxlines=200

" It's okay to set tab settings here, because an indent of two spaces is specified
" by the file format.
setlocal tabstop=2 softtabstop=2 shiftwidth=2 expandtab

let b:current_syntax = "hgtest"

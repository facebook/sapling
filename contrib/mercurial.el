;;; mercurial.el --- Emacs support for the Mercurial distributed SCM

;; Copyright (C) 2005 Bryan O'Sullivan

;; Author: Bryan O'Sullivan <bos@serpentine.com>

;; $Id$

;; mercurial.el ("this file") is free software; you can redistribute
;; it and/or modify it under the terms of version 2 of the GNU General
;; Public License as published by the Free Software Foundation.

;; This file is distributed in the hope that it will be useful, but
;; WITHOUT ANY WARRANTY; without even the implied warranty of
;; MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
;; General Public License for more details.

;; You should have received a copy of the GNU General Public License
;; along with this file, GNU Emacs, or XEmacs; see the file COPYING
;; (`C-h C-l').  If not, write to the Free Software Foundation, Inc.,
;; 59 Temple Place - Suite 330, Boston, MA 02111-1307, USA.

;;; Commentary:

;; This mode builds upon Emacs's VC mode to provide flexible
;; integration with the Mercurial distributed SCM tool.

;; To get going as quickly as possible, load this file into Emacs and
;; type `C-c h h'; this runs hg-help-overview, which prints a helpful
;; usage overview.

;; Much of the inspiration for mercurial.el comes from Rajesh
;; Vaidheeswarran's excellent p4.el, which does an admirably thorough
;; job for the commercial Perforce SCM product.  In fact, substantial
;; chunks of code are adapted from p4.el.

;; This code has been developed under XEmacs 21.5, and may will not
;; work as well under GNU Emacs (albeit tested under 21.2).  Patches
;; to enhance the portability of this code, fix bugs, and add features
;; are most welcome.  You can clone a Mercurial repository for this
;; package from http://www.serpentine.com/hg/hg-emacs

;; Please send problem reports and suggestions to bos@serpentine.com.


;;; Code:

(require 'advice)
(require 'cl)
(require 'diff-mode)
(require 'easymenu)
(require 'vc)


;;; XEmacs has view-less, while GNU Emacs has view.  Joy.

(condition-case nil
    (require 'view-less)
  (error nil))
(condition-case nil
    (require 'view)
  (error nil))


;;; Variables accessible through the custom system.

(defgroup hg nil
  "Mercurial distributed SCM."
  :group 'tools)

(defcustom hg-binary
  (dolist (path '("~/bin/hg"
		  "/usr/bin/hg"
		  "/usr/local/bin/hg"))
    (when (file-executable-p path)
      (return path)))
  "The path to Mercurial's hg executable."
  :type '(file :must-match t)
  :group 'hg)

(defcustom hg-mode-hook nil
  "Hook run when a buffer enters hg-mode."
  :type 'sexp
  :group 'hg)

(defcustom hg-global-prefix "\C-ch"
  "The global prefix for Mercurial keymap bindings."
  :type 'sexp
  :group 'hg)


;;; Other variables.

(defconst hg-running-xemacs (string-match "XEmacs" emacs-version)
  "Is mercurial.el running under XEmacs?")

(defvar hg-mode nil
  "Is this file managed by Mercurial?")

(defvar hg-output-buffer-name "*Hg*"
  "The name to use for Mercurial output buffers.")

(defvar hg-file-name-history nil)


;;; hg-mode keymap.

(defvar hg-prefix-map
  (let ((map (copy-keymap vc-prefix-map)))
    (set-keymap-name map 'hg-prefix-map)
    map)
  "This keymap overrides some default vc-mode bindings.")
(fset 'hg-prefix-map hg-prefix-map)
(define-key hg-prefix-map "=" 'hg-diff-file)
(define-key hg-prefix-map "c" 'hg-undo)
(define-key hg-prefix-map "g" 'hg-annotate)
(define-key hg-prefix-map "l" 'hg-log-file)
;; (define-key hg-prefix-map "r" 'hg-update)
(define-key hg-prefix-map "u" 'hg-revert-file)
(define-key hg-prefix-map "~" 'hg-version-other-window)

(defvar hg-mode-map (make-sparse-keymap))
(define-key hg-mode-map "\C-xv" 'hg-prefix-map)


;;; Global keymap.

(global-set-key "\C-xvi" 'hg-add-file)

(defvar hg-global-map (make-sparse-keymap))
(fset 'hg-global-map hg-global-map)
(global-set-key hg-global-prefix 'hg-global-map)
(define-key hg-global-map "," 'hg-incoming)
(define-key hg-global-map "." 'hg-outgoing)
(define-key hg-global-map "<" 'hg-pull)
(define-key hg-global-map "=" 'hg-diff)
(define-key hg-global-map ">" 'hg-push)
(define-key hg-global-map "?" 'hg-help-overview)
(define-key hg-global-map "A" 'hg-addremove)
(define-key hg-global-map "U" 'hg-revert)
(define-key hg-global-map "a" 'hg-add)
(define-key hg-global-map "c" 'hg-commit)
(define-key hg-global-map "h" 'hg-help-overview)
(define-key hg-global-map "i" 'hg-init)
(define-key hg-global-map "l" 'hg-log)
(define-key hg-global-map "r" 'hg-root)
(define-key hg-global-map "s" 'hg-status)
(define-key hg-global-map "u" 'hg-update)


;;; View mode keymap.

(defvar hg-view-mode-map
  (let ((map (copy-keymap (if (boundp 'view-minor-mode-map)
			      view-minor-mode-map
			    view-mode-map))))
    (set-keymap-name map 'hg-view-mode-map)
    map))
(fset 'hg-view-mode-map hg-view-mode-map)
(define-key hg-view-mode-map
  (if hg-running-xemacs [button2] [mouse-2])
  'hg-buffer-mouse-clicked)


;;; Convenience functions.

(defun hg-binary ()
  (if hg-binary
      hg-binary
    (error "No `hg' executable found!")))

(defun hg-replace-in-string (str regexp newtext &optional literal)
  "Replace all matches in STR for REGEXP with NEWTEXT string.
Return the new string.  Optional LITERAL non-nil means do a literal
replacement.

This function bridges yet another pointless impedance gap between
XEmacs and GNU Emacs."
  (if (fboundp 'replace-in-string)
      (replace-in-string str regexp newtext literal)
    (replace-regexp-in-string regexp newtext str nil literal)))

(defun hg-chomp (str)
  "Strip trailing newlines from a string."
  (hg-replace-in-string str "[\r\n]+$" ""))

(defun hg-run-command (command &rest args)
  "Run the shell command COMMAND, returning (EXIT-CODE . COMMAND-OUTPUT).
The list ARGS contains a list of arguments to pass to the command."
  (let* (exit-code
	 (output
	  (with-output-to-string
	    (with-current-buffer
		standard-output
	      (setq exit-code
		    (apply 'call-process command nil t nil args))))))
    (cons exit-code output)))

(defun hg-run (command &rest args)
  "Run the Mercurial command COMMAND, returning (EXIT-CODE . COMMAND-OUTPUT)."
  (apply 'hg-run-command (hg-binary) command args))

(defun hg-run0 (command &rest args)
  "Run the Mercurial command COMMAND, returning its output.
If the command does not exit with a zero status code, raise an error."
  (let ((res (apply 'hg-run-command (hg-binary) command args)))
    (if (not (eq (car res) 0))
	(error "Mercurial command failed %s - exit code %s"
	       (cons command args)
	       (car res))
      (cdr res))))

(defun hg-buffer-commands (pnt)
  "Use the properties of a character to do something sensible."
  (interactive "d")
  (let ((rev (get-char-property pnt 'rev))
	(file (get-char-property pnt 'file))
	(date (get-char-property pnt 'date))
	(user (get-char-property pnt 'user))
	(host (get-char-property pnt 'host))
	(prev-buf (current-buffer)))
    (cond
     (file
      (find-file-other-window file))
     (rev
      (hg-diff hg-view-file-name rev rev prev-buf))
     ((message "I don't know how to do that yet")))))

(defun hg-buffer-mouse-clicked (event)
  "Translate the mouse clicks in a HG log buffer to character events.
These are then handed off to `hg-buffer-commands'.

Handle frickin' frackin' gratuitous event-related incompatibilities."
  (interactive "e")
  (if hg-running-xemacs
      (progn
	(select-window (event-window event))
	(hg-buffer-commands (event-point event)))
    (select-window (posn-window (event-end event)))
    (hg-buffer-commands (posn-point (event-start event)))))

(unless (fboundp 'view-minor-mode)
  (defun view-minor-mode (prev-buffer exit-func)
    (view-mode)))

(defun hg-abbrev-file-name (file)
  (if hg-running-xemacs
      (abbreviate-file-name file t)
    (abbreviate-file-name file)))


;;; View mode bits.

(defun hg-exit-view-mode (buf)
  "Exit from hg-view-mode.
We delete the current window if entering hg-view-mode split the
current frame."
  (when (and (eq buf (current-buffer))
	     (> (length (window-list)) 1))
    (delete-window))
  (when (buffer-live-p buf)
    (kill-buffer buf)))

(defun hg-view-mode (prev-buffer &optional file-name)
  (goto-char (point-min))
  (set-buffer-modified-p nil)
  (toggle-read-only t)
  (view-minor-mode prev-buffer 'hg-exit-view-mode)
  (use-local-map hg-view-mode-map)
  (setq truncate-lines t)
  (when file-name
    (set (make-local-variable 'hg-view-file-name)
	 (hg-abbrev-file-name file-name))))
  
(defmacro hg-view-output (args &rest body)
  "Execute BODY in a clean buffer, then switch that buffer to view-mode.
ARGS is of the form (BUFFER-NAME &optional FILE), where BUFFER-NAME is
the name of the buffer to create, and FILE is the name of the file
being viewed."
  (let ((prev-buf (gensym "prev-buf-"))
	(v-b-name (car args))
	(v-m-rest (cdr args)))
    `(let ((view-buf-name ,v-b-name)
	   (,prev-buf (current-buffer)))
       (get-buffer-create view-buf-name)
       (kill-buffer view-buf-name)
       (pop-to-buffer view-buf-name)
       (save-excursion
	 ,@body)
       (hg-view-mode ,prev-buf ,@v-m-rest))))

(put 'hg-view-output 'lisp-indent-function 1)


;;; User interface functions.

(defun hg-help-overview ()
  "This is an overview of the Mercurial SCM mode for Emacs.

You can find the source code, license (GPL v2), and credits for this
code by typing `M-x find-library mercurial RET'.

The Mercurial mode user interface is based on that of the older VC
mode, so if you're already familiar with VC, the same keybindings and
functions will generally work.

Below is a list of common SCM tasks, with the key bindings needed to
perform them, and the command names.  This list is not exhaustive.

In the list below, `G/L' indicates whether a key binding is global (G)
or local (L).  Global keybindings work on any file inside a Mercurial
repository.  Local keybindings only apply to files under the control
of Mercurial.  Many commands take a prefix argument.


SCM Task                              G/L  Key Binding  Command Name
--------                              ---  -----------  ------------
Help overview (what you are reading)  G    C-c h h      hg-help-overview

Tell Mercurial to manage a file       G    C-x v i      hg-add-file
Commit changes to current file only   L    C-x C-q      vc-toggle-read-only
Undo changes to file since commit     L    C-x v u      hg-revert-file

Diff file vs last checkin             L    C-x v =      hg-diff-file

View file change history              L    C-x v l      hg-log-file
View annotated file                   L    C-x v a      hg-annotate

Diff repo vs last checkin             G    C-c h =      hg-diff
View status of files in repo          G    C-c h s      hg-status
Commit all changes                    G    C-c h c      hg-commit

Undo all changes since last commit    G    C-c h U      hg-revert
View repo change history              G    C-c h l      hg-log

See changes that can be pulled        G    C-c h ,      hg-incoming
Pull changes                          G    C-c h <      hg-pull
Update working directory after pull   G    C-c h u      hg-update
See changes that can be pushed        G    C-c h .      hg-outgoing
Push changes                          G    C-c h >      hg-push"
  (interactive)
  (hg-view-output ("Mercurial Help Overview")
    (insert (documentation 'hg-help-overview))))

(defun hg-add ()
  (interactive)
  (error "not implemented"))

(defun hg-add-file ()
  (interactive)
  (error "not implemented"))

(defun hg-addremove ()
  (interactive)
  (error "not implemented"))

(defun hg-annotate ()
  (interactive)
  (error "not implemented"))

(defun hg-commit ()
  (interactive)
  (error "not implemented"))

(defun hg-diff ()
  (interactive)
  (error "not implemented"))

(defun hg-diff-file ()
  (interactive)
  (error "not implemented"))

(defun hg-incoming ()
  (interactive)
  (error "not implemented"))

(defun hg-init ()
  (interactive)
  (error "not implemented"))

(defun hg-log-file ()
  (interactive)
  (error "not implemented"))

(defun hg-log ()
  (interactive)
  (error "not implemented"))

(defun hg-outgoing ()
  (interactive)
  (error "not implemented"))

(defun hg-pull ()
  (interactive)
  (error "not implemented"))

(defun hg-push ()
  (interactive)
  (error "not implemented"))

(defun hg-revert ()
  (interactive)
  (error "not implemented"))

(defun hg-revert-file ()
  (interactive)
  (error "not implemented"))

(defun hg-root (&optional path)
  (interactive)
  (unless path
    (setq path (if (and (interactive-p) current-prefix-arg)
		   (expand-file-name (read-file-name "Path name: "))
		 (or (buffer-file-name) "(none)"))))
  (let ((root (do ((prev nil dir)
		   (dir (file-name-directory path)
			(file-name-directory (directory-file-name dir))))
		  ((equal prev dir))
		(when (file-directory-p (concat dir ".hg"))
		  (return dir)))))
    (when (interactive-p)
      (if root
	  (message "The root of this repository is `%s'." root)
	(message "The path `%s' is not in a Mercurial repository."
		 (abbreviate-file-name path t))))
    root))

(defun hg-status ()
  (interactive)
  (error "not implemented"))

(defun hg-undo ()
  (interactive)
  (error "not implemented"))

(defun hg-version-other-window ()
  (interactive)
  (error "not implemented"))


(provide 'mercurial)


;;; Local Variables:
;;; mode: emacs-lisp
;;; prompt-to-byte-compile: nil
;;; end:

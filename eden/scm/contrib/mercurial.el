;;; mercurial.el --- Emacs support for the Mercurial distributed SCM

;; Copyright (C) 2005, 2006 Bryan O'Sullivan

;; Author: Bryan O'Sullivan <bos@serpentine.com>

;; mercurial.el is free software; you can redistribute it and/or
;; modify it under the terms of the GNU General Public License version
;; 2 or any later version.

;; mercurial.el is distributed in the hope that it will be useful, but
;; WITHOUT ANY WARRANTY; without even the implied warranty of
;; MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
;; General Public License for more details.

;; You should have received a copy of the GNU General Public License
;; along with mercurial.el, GNU Emacs, or XEmacs; see the file COPYING
;; (`C-h C-l').  If not, see <http://www.gnu.org/licenses/>.

;;; Commentary:

;; mercurial.el builds upon Emacs's VC mode to provide flexible
;; integration with the Mercurial distributed SCM tool.

;; To get going as quickly as possible, load mercurial.el into Emacs and
;; type `C-c h h'; this runs hg-help-overview, which prints a helpful
;; usage overview.

;; Much of the inspiration for mercurial.el comes from Rajesh
;; Vaidheeswarran's excellent p4.el, which does an admirably thorough
;; job for the commercial Perforce SCM product.  In fact, substantial
;; chunks of code are adapted from p4.el.

;; This code has been developed under XEmacs 21.5, and may not work as
;; well under GNU Emacs (albeit tested under 21.4).  Patches to
;; enhance the portability of this code, fix bugs, and add features
;; are most welcome.

;; As of version 22.3, GNU Emacs's VC mode has direct support for
;; Mercurial, so this package may not prove as useful there.

;; Please send problem reports and suggestions to bos@serpentine.com.


;;; Code:

(eval-when-compile (require 'cl))
(require 'diff-mode)
(require 'easymenu)
(require 'executable)
(require 'vc)

(defmacro hg-feature-cond (&rest clauses)
  "Test CLAUSES for feature at compile time.
Each clause is (FEATURE BODY...)."
  (dolist (x clauses)
    (let ((feature (car x))
	  (body (cdr x)))
      (when (or (eq feature t)
		(featurep feature))
	(return (cons 'progn body))))))


;;; XEmacs has view-less, while GNU Emacs has view.  Joy.

(hg-feature-cond
 (xemacs (require 'view-less))
 (t (require 'view)))


;;; Variables accessible through the custom system.

(defgroup mercurial nil
  "Mercurial distributed SCM."
  :group 'tools)

(defcustom hg-binary
    (or (executable-find "hg")
	(dolist (path '("~/bin/hg" "/usr/bin/hg" "/usr/local/bin/hg"))
	  (when (file-executable-p path)
	    (return path))))
  "The path to Mercurial's hg executable."
  :type '(file :must-match t)
  :group 'mercurial)

(defcustom hg-mode-hook nil
  "Hook run when a buffer enters hg-mode."
  :type 'sexp
  :group 'mercurial)

(defcustom hg-commit-mode-hook nil
  "Hook run when a buffer is created to prepare a commit."
  :type 'sexp
  :group 'mercurial)

(defcustom hg-pre-commit-hook nil
  "Hook run before a commit is performed.
If you want to prevent the commit from proceeding, raise an error."
  :type 'sexp
  :group 'mercurial)

(defcustom hg-log-mode-hook nil
  "Hook run after a buffer is filled with log information."
  :type 'sexp
  :group 'mercurial)

(defcustom hg-global-prefix "\C-ch"
  "The global prefix for Mercurial keymap bindings."
  :type 'sexp
  :group 'mercurial)

(defcustom hg-commit-allow-empty-message nil
  "Whether to allow changes to be committed with empty descriptions."
  :type 'boolean
  :group 'mercurial)

(defcustom hg-commit-allow-empty-file-list nil
  "Whether to allow changes to be committed without any modified files."
  :type 'boolean
  :group 'mercurial)

(defcustom hg-rev-completion-limit 100
  "The maximum number of revisions that hg-read-rev will offer to complete.
This affects memory usage and performance when prompting for revisions
in a repository with a lot of history."
  :type 'integer
  :group 'mercurial)

(defcustom hg-log-limit 50
  "The maximum number of revisions that hg-log will display."
  :type 'integer
  :group 'mercurial)

(defcustom hg-update-modeline t
  "Whether to update the modeline with the status of a file after every save.
Set this to nil on platforms with poor process management, such as Windows."
  :type 'boolean
  :group 'mercurial)

(defcustom hg-incoming-repository "default"
  "The repository from which changes are pulled from by default.
This should be a symbolic repository name, since it is used for all
repository-related commands."
  :type 'string
  :group 'mercurial)

(defcustom hg-outgoing-repository ""
  "The repository to which changes are pushed to by default.
This should be a symbolic repository name, since it is used for all
repository-related commands."
  :type 'string
  :group 'mercurial)


;;; Other variables.

(defvar hg-mode nil
  "Is this file managed by Mercurial?")
(make-variable-buffer-local 'hg-mode)
(put 'hg-mode 'permanent-local t)

(defvar hg-status nil)
(make-variable-buffer-local 'hg-status)
(put 'hg-status 'permanent-local t)

(defvar hg-prev-buffer nil)
(make-variable-buffer-local 'hg-prev-buffer)
(put 'hg-prev-buffer 'permanent-local t)

(defvar hg-root nil)
(make-variable-buffer-local 'hg-root)
(put 'hg-root 'permanent-local t)

(defvar hg-view-mode nil)
(make-variable-buffer-local 'hg-view-mode)
(put 'hg-view-mode 'permanent-local t)

(defvar hg-view-file-name nil)
(make-variable-buffer-local 'hg-view-file-name)
(put 'hg-view-file-name 'permanent-local t)

(defvar hg-output-buffer-name "*Hg*"
  "The name to use for Mercurial output buffers.")

(defvar hg-file-history nil)
(defvar hg-repo-history nil)
(defvar hg-rev-history nil)
(defvar hg-repo-completion-table nil)	; shut up warnings


;;; Random constants.

(defconst hg-commit-message-start
  "--- Enter your commit message.  Type `C-c C-c' to commit. ---\n")

(defconst hg-commit-message-end
  "--- Files in bold will be committed.  Click to toggle selection. ---\n")

(defconst hg-state-alist
  '((?M . modified)
    (?A . added)
    (?R . removed)
    (?! . deleted)
    (?C . normal)
    (?I . ignored)
    (?? . nil)))

;;; hg-mode keymap.

(defvar hg-prefix-map
  (let ((map (make-sparse-keymap)))
    (hg-feature-cond (xemacs (set-keymap-name map 'hg-prefix-map))) ; XEmacs
    (set-keymap-parent map vc-prefix-map)
    (define-key map "=" 'hg-diff)
    (define-key map "c" 'hg-undo)
    (define-key map "g" 'hg-annotate)
    (define-key map "i" 'hg-add)
    (define-key map "l" 'hg-log)
    (define-key map "n" 'hg-commit-start)
    ;; (define-key map "r" 'hg-update)
    (define-key map "u" 'hg-revert-buffer)
    (define-key map "~" 'hg-version-other-window)
    map)
  "This keymap overrides some default vc-mode bindings.")

(defvar hg-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map "\C-xv" hg-prefix-map)
    map))

(add-minor-mode 'hg-mode 'hg-mode hg-mode-map)


;;; Global keymap.

(defvar hg-global-map
  (let ((map (make-sparse-keymap)))
    (define-key map "," 'hg-incoming)
    (define-key map "." 'hg-outgoing)
    (define-key map "<" 'hg-pull)
    (define-key map "=" 'hg-diff-repo)
    (define-key map ">" 'hg-push)
    (define-key map "?" 'hg-help-overview)
    (define-key map "A" 'hg-addremove)
    (define-key map "U" 'hg-revert)
    (define-key map "a" 'hg-add)
    (define-key map "c" 'hg-commit-start)
    (define-key map "f" 'hg-forget)
    (define-key map "h" 'hg-help-overview)
    (define-key map "i" 'hg-init)
    (define-key map "l" 'hg-log-repo)
    (define-key map "r" 'hg-root)
    (define-key map "s" 'hg-status)
    (define-key map "u" 'hg-update)
    map))

(global-set-key hg-global-prefix hg-global-map)

;;; View mode keymap.

(defvar hg-view-mode-map
  (let ((map (make-sparse-keymap)))
    (hg-feature-cond (xemacs (set-keymap-name map 'hg-view-mode-map))) ; XEmacs
    (define-key map (hg-feature-cond (xemacs [button2])
				     (t [mouse-2]))
      'hg-buffer-mouse-clicked)
    map))

(add-minor-mode 'hg-view-mode "" hg-view-mode-map)


;;; Commit mode keymaps.

(defvar hg-commit-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map "\C-c\C-c" 'hg-commit-finish)
    (define-key map "\C-c\C-k" 'hg-commit-kill)
    (define-key map "\C-xv=" 'hg-diff-repo)
    map))

(defvar hg-commit-mode-file-map
  (let ((map (make-sparse-keymap)))
    (define-key map (hg-feature-cond (xemacs [button2])
				     (t [mouse-2]))
      'hg-commit-mouse-clicked)
    (define-key map " " 'hg-commit-toggle-file)
    (define-key map "\r" 'hg-commit-toggle-file)
    map))


;;; Convenience functions.

(defsubst hg-binary ()
  (if hg-binary
      hg-binary
    (error "No `hg' executable found!")))

(defsubst hg-replace-in-string (str regexp newtext &optional literal)
  "Replace all matches in STR for REGEXP with NEWTEXT string.
Return the new string.  Optional LITERAL non-nil means do a literal
replacement.

This function bridges yet another pointless impedance gap between
XEmacs and GNU Emacs."
  (hg-feature-cond
   (xemacs (replace-in-string str regexp newtext literal))
   (t (replace-regexp-in-string regexp newtext str nil literal))))

(defsubst hg-strip (str)
  "Strip leading and trailing blank lines from a string."
  (hg-replace-in-string (hg-replace-in-string str "[\r\n][ \t\r\n]*\\'" "")
			"\\`[ \t\r\n]*[\r\n]" ""))

(defsubst hg-chomp (str)
  "Strip trailing newlines from a string."
  (hg-replace-in-string str "[\r\n]+\\'" ""))

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

(defmacro hg-do-across-repo (path &rest body)
  (let ((root-name (make-symbol "root-"))
	(buf-name (make-symbol "buf-")))
    `(let ((,root-name (hg-root ,path)))
       (save-excursion
	 (dolist (,buf-name (buffer-list))
	   (set-buffer ,buf-name)
	   (when (and hg-status (equal (hg-root buffer-file-name) ,root-name))
	     ,@body))))))

(put 'hg-do-across-repo 'lisp-indent-function 1)

(defun hg-sync-buffers (path)
  "Sync buffers visiting PATH with their on-disk copies.
If PATH is not being visited, but is under the repository root, sync
all buffers visiting files in the repository."
  (let ((buf (find-buffer-visiting path)))
    (if buf
	(with-current-buffer buf
	  (vc-buffer-sync))
      (hg-do-across-repo path
	(vc-buffer-sync)))))

(defun hg-buffer-commands (pnt)
  "Use the properties of a character to do something sensible."
  (interactive "d")
  (let ((rev (get-char-property pnt 'rev))
	(file (get-char-property pnt 'file)))
    (cond
     (file
      (find-file-other-window file))
     (rev
      (hg-diff hg-view-file-name rev rev))
     ((message "I don't know how to do that yet")))))

(defsubst hg-event-point (event)
  "Return the character position of the mouse event EVENT."
  (hg-feature-cond (xemacs (event-point event))
		   (t (posn-point (event-start event)))))

(defsubst hg-event-window (event)
  "Return the window over which mouse event EVENT occurred."
  (hg-feature-cond (xemacs (event-window event))
		   (t (posn-window (event-start event)))))

(defun hg-buffer-mouse-clicked (event)
  "Translate the mouse clicks in a HG log buffer to character events.
These are then handed off to `hg-buffer-commands'.

Handle frickin' frackin' gratuitous event-related incompatibilities."
  (interactive "e")
  (select-window (hg-event-window event))
  (hg-buffer-commands (hg-event-point event)))

(defsubst hg-abbrev-file-name (file)
  "Portable wrapper around abbreviate-file-name."
  (hg-feature-cond (xemacs (abbreviate-file-name file t))
		   (t (abbreviate-file-name file))))

(defun hg-read-file-name (&optional prompt default)
  "Read a file or directory name, or a pattern, to use with a command."
  (save-excursion
    (while hg-prev-buffer
      (set-buffer hg-prev-buffer))
    (let ((path (or default
                    (buffer-file-name)
                    (expand-file-name default-directory))))
      (if (or (not path) current-prefix-arg)
          (expand-file-name
           (eval (list* 'read-file-name
                        (format "File, directory or pattern%s: "
                                (or prompt ""))
                        (and path (file-name-directory path))
                        nil nil
                        (and path (file-name-nondirectory path))
                        (hg-feature-cond
			 (xemacs (cons (quote 'hg-file-history) nil))
			 (t nil)))))
        path))))

(defun hg-read-number (&optional prompt default)
  "Read a integer value."
  (save-excursion
    (if (or (not default) current-prefix-arg)
        (string-to-number
         (eval (list* 'read-string
                      (or prompt "")
                      (if default (cons (format "%d" default) nil) nil))))
      default)))

(defun hg-read-config ()
  "Return an alist of (key . value) pairs of Mercurial config data.
Each key is of the form (section . name)."
  (let (items)
    (dolist (line (split-string (hg-chomp (hg-run0 "debugconfig")) "\n") items)
      (string-match "^\\([^=]*\\)=\\(.*\\)" line)
      (let* ((left (substring line (match-beginning 1) (match-end 1)))
	     (right (substring line (match-beginning 2) (match-end 2)))
	     (key (split-string left "\\."))
	     (value (hg-replace-in-string right "\\\\n" "\n" t)))
	(setq items (cons (cons (cons (car key) (cadr key)) value) items))))))

(defun hg-config-section (section config)
  "Return an alist of (name . value) pairs for SECTION of CONFIG."
  (let (items)
    (dolist (item config items)
      (when (equal (caar item) section)
	(setq items (cons (cons (cdar item) (cdr item)) items))))))

(defun hg-string-starts-with (sub str)
  "Indicate whether string STR starts with the substring or character SUB."
  (if (not (stringp sub))
      (and (> (length str) 0) (equal (elt str 0) sub))
    (let ((sub-len (length sub)))
      (and (<= sub-len (length str))
	   (string= sub (substring str 0 sub-len))))))

(defun hg-complete-repo (string predicate all)
  "Attempt to complete a repository name.
We complete on either symbolic names from Mercurial's config or real
directory names from the file system.  We do not penalize URLs."
  (or (if all
	  (all-completions string hg-repo-completion-table predicate)
	(try-completion string hg-repo-completion-table predicate))
      (let* ((str (expand-file-name string))
	     (dir (file-name-directory str))
	     (file (file-name-nondirectory str)))
	(if all
	    (let (completions)
	      (dolist (name (delete "./" (file-name-all-completions file dir))
			    completions)
		(let ((path (concat dir name)))
		  (when (file-directory-p path)
		    (setq completions (cons name completions))))))
	  (let ((comp (file-name-completion file dir)))
	    (if comp
		(hg-abbrev-file-name (concat dir comp))))))))

(defun hg-read-repo-name (&optional prompt initial-contents default)
  "Read the location of a repository."
  (save-excursion
    (while hg-prev-buffer
      (set-buffer hg-prev-buffer))
    (let (hg-repo-completion-table)
      (if current-prefix-arg
	  (progn
	    (dolist (path (hg-config-section "paths" (hg-read-config)))
	      (setq hg-repo-completion-table
		    (cons (cons (car path) t) hg-repo-completion-table))
	      (unless (hg-string-starts-with (hg-feature-cond
					      (xemacs directory-sep-char)
					      (t ?/))
					     (cdr path))
		(setq hg-repo-completion-table
		      (cons (cons (cdr path) t) hg-repo-completion-table))))
	    (completing-read (format "Repository%s: " (or prompt ""))
			     'hg-complete-repo
			     nil
			     nil
			     initial-contents
			     'hg-repo-history
			     default))
	default))))

(defun hg-read-rev (&optional prompt default)
  "Read a revision or tag, offering completions."
  (save-excursion
    (while hg-prev-buffer
      (set-buffer hg-prev-buffer))
    (let ((rev (or default "tip")))
      (if current-prefix-arg
	  (let ((revs (split-string
		       (hg-chomp
			(hg-run0 "-q" "log" "-l"
				 (format "%d" hg-rev-completion-limit)))
		       "[\n:]")))
	    (dolist (line (split-string (hg-chomp (hg-run0 "tags")) "\n"))
	      (setq revs (cons (car (split-string line "\\s-")) revs)))
	    (completing-read (format "Revision%s (%s): "
				     (or prompt "")
				     (or default "tip"))
			     (mapcar (lambda (x) (cons x x)) revs)
			     nil
			     nil
			     nil
			     'hg-rev-history
			     (or default "tip")))
	rev))))

(defun hg-parents-for-mode-line (root)
  "Format the parents of the working directory for the mode line."
  (let ((parents (split-string (hg-chomp
				(hg-run0 "--cwd" root "parents" "--template"
					 "{rev}\n")) "\n")))
    (mapconcat 'identity parents "+")))

(defun hg-buffers-visiting-repo (&optional path)
  "Return a list of buffers visiting the repository containing PATH."
  (let ((root-name (hg-root (or path (buffer-file-name))))
	bufs)
    (save-excursion
      (dolist (buf (buffer-list) bufs)
	(set-buffer buf)
	(let ((name (buffer-file-name)))
	  (when (and hg-status name (equal (hg-root name) root-name))
	    (setq bufs (cons buf bufs))))))))

(defun hg-update-mode-lines (path)
  "Update the mode lines of all buffers visiting the same repository as PATH."
  (let* ((root (hg-root path))
	 (parents (hg-parents-for-mode-line root)))
    (save-excursion
      (dolist (info (hg-path-status
		     root
		     (mapcar
		      (function
		       (lambda (buf)
			 (substring (buffer-file-name buf) (length root))))
		      (hg-buffers-visiting-repo root))))
	(let* ((name (car info))
	       (status (cdr info))
	       (buf (find-buffer-visiting (concat root name))))
	  (when buf
	    (set-buffer buf)
	    (hg-mode-line-internal status parents)))))))


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
  (hg-feature-cond (xemacs (view-minor-mode prev-buffer 'hg-exit-view-mode))
		   (t (view-mode-enter nil 'hg-exit-view-mode)))
  (setq hg-view-mode t)
  (setq truncate-lines t)
  (when file-name
    (setq hg-view-file-name
	  (hg-abbrev-file-name file-name))))

(defun hg-file-status (file)
  "Return status of FILE, or nil if FILE does not exist or is unmanaged."
  (let* ((s (hg-run "status" file))
	 (exit (car s))
	 (output (cdr s)))
    (if (= exit 0)
	(let ((state (and (>= (length output) 2)
			  (= (aref output 1) ? )
			  (assq (aref output 0) hg-state-alist))))
	  (if state
	      (cdr state)
	    'normal)))))

(defun hg-path-status (root paths)
  "Return status of PATHS in repo ROOT as an alist.
Each entry is a pair (FILE-NAME . STATUS)."
  (let ((s (apply 'hg-run "--cwd" root "status" "-marduc" paths))
	result)
    (dolist (entry (split-string (hg-chomp (cdr s)) "\n") (nreverse result))
      (let (state name)
	(cond ((= (aref entry 1) ? )
	       (setq state (assq (aref entry 0) hg-state-alist)
		     name (substring entry 2)))
	      ((string-match "\\(.*\\): " entry)
	       (setq name (match-string 1 entry))))
	(setq result (cons (cons name state) result))))))

(defmacro hg-view-output (args &rest body)
  "Execute BODY in a clean buffer, then quickly display that buffer.
If the buffer contains one line, its contents are displayed in the
minibuffer.  Otherwise, the buffer is displayed in view-mode.
ARGS is of the form (BUFFER-NAME &optional FILE), where BUFFER-NAME is
the name of the buffer to create, and FILE is the name of the file
being viewed."
  (let ((prev-buf (make-symbol "prev-buf-"))
	(v-b-name (car args))
	(v-m-rest (cdr args)))
    `(let ((view-buf-name ,v-b-name)
	   (,prev-buf (current-buffer)))
       (get-buffer-create view-buf-name)
       (kill-buffer view-buf-name)
       (get-buffer-create view-buf-name)
       (set-buffer view-buf-name)
       (save-excursion
	 ,@body)
       (case (count-lines (point-min) (point-max))
	 ((0)
	  (kill-buffer view-buf-name)
	  (message "(No output)"))
	 ((1)
	  (let ((msg (hg-chomp (buffer-substring (point-min) (point-max)))))
	    (kill-buffer view-buf-name)
	    (message "%s" msg)))
	 (t
	  (pop-to-buffer view-buf-name)
	  (setq hg-prev-buffer ,prev-buf)
	  (hg-view-mode ,prev-buf ,@v-m-rest))))))

(put 'hg-view-output 'lisp-indent-function 1)

;;; Context save and restore across revert and other operations.

(defun hg-position-context (pos)
  "Return information to help find the given position again."
  (let* ((end (min (point-max) (+ pos 98))))
    (list pos
	  (buffer-substring (max (point-min) (- pos 2)) end)
	  (- end pos))))

(defun hg-buffer-context ()
  "Return information to help restore a user's editing context.
This is useful across reverts and merges, where a context is likely
to have moved a little, but not really changed."
  (let ((point-context (hg-position-context (point)))
	(mark-context (let ((mark (mark-marker)))
			(and mark
			     ;; make sure active mark
			     (marker-buffer mark)
			     (marker-position mark)
			     (hg-position-context mark)))))
    (list point-context mark-context)))

(defun hg-find-context (ctx)
  "Attempt to find a context in the given buffer.
Always returns a valid, hopefully sane, position."
  (let ((pos (nth 0 ctx))
	(str (nth 1 ctx))
	(fixup (nth 2 ctx)))
    (save-excursion
      (goto-char (max (point-min) (- pos 15000)))
      (if (and (not (equal str ""))
	       (search-forward str nil t))
	  (- (point) fixup)
	(max pos (point-min))))))

(defun hg-restore-context (ctx)
  "Attempt to restore the user's editing context."
  (let ((point-context (nth 0 ctx))
	(mark-context (nth 1 ctx)))
    (goto-char (hg-find-context point-context))
    (when mark-context
      (set-mark (hg-find-context mark-context)))))


;;; Hooks.

(defun hg-mode-line-internal (status parents)
  (setq hg-status status
	hg-mode (and status (concat " Hg:"
				    parents
				    (cdr (assq status
					       '((normal . "")
						 (removed . "r")
						 (added . "a")
						 (deleted . "!")
						 (modified . "m"))))))))

(defun hg-mode-line (&optional force)
  "Update the modeline with the current status of a file.
An update occurs if optional argument FORCE is non-nil,
hg-update-modeline is non-nil, or we have not yet checked the state of
the file."
  (let ((root (hg-root)))
    (when (and root (or force hg-update-modeline (not hg-mode)))
      (let ((status (hg-file-status buffer-file-name))
	    (parents (hg-parents-for-mode-line root)))
	(hg-mode-line-internal status parents)
	status))))

(defun hg-mode (&optional toggle)
  "Minor mode for Mercurial distributed SCM integration.

The Mercurial mode user interface is based on that of VC mode, so if
you're already familiar with VC, the same keybindings and functions
will generally work.

Below is a list of many common SCM tasks.  In the list, `G/L\'
indicates whether a key binding is global (G) to a repository or
local (L) to a file.  Many commands take a prefix argument.

SCM Task                              G/L  Key Binding  Command Name
--------                              ---  -----------  ------------
Help overview (what you are reading)  G    C-c h h      hg-help-overview

Tell Mercurial to manage a file       G    C-c h a      hg-add
Commit changes to current file only   L    C-x v n      hg-commit-start
Undo changes to file since commit     L    C-x v u      hg-revert-buffer

Diff file vs last checkin             L    C-x v =      hg-diff

View file change history              L    C-x v l      hg-log
View annotated file                   L    C-x v a      hg-annotate

Diff repo vs last checkin             G    C-c h =      hg-diff-repo
View status of files in repo          G    C-c h s      hg-status
Commit all changes                    G    C-c h c      hg-commit-start

Undo all changes since last commit    G    C-c h U      hg-revert
View repo change history              G    C-c h l      hg-log-repo

See changes that can be pulled        G    C-c h ,      hg-incoming
Pull changes                          G    C-c h <      hg-pull
Update working directory after pull   G    C-c h u      hg-update
See changes that can be pushed        G    C-c h .      hg-outgoing
Push changes                          G    C-c h >      hg-push"
  (unless vc-make-backup-files
    (set (make-local-variable 'backup-inhibited) t))
  (run-hooks 'hg-mode-hook))

(defun hg-find-file-hook ()
  (ignore-errors
    (when (hg-mode-line)
      (hg-mode))))

(add-hook 'find-file-hooks 'hg-find-file-hook)

(defun hg-after-save-hook ()
  (ignore-errors
    (let ((old-status hg-status))
      (hg-mode-line)
      (if (and (not old-status) hg-status)
	  (hg-mode)))))

(add-hook 'after-save-hook 'hg-after-save-hook)


;;; User interface functions.

(defun hg-help-overview ()
  "This is an overview of the Mercurial SCM mode for Emacs.

You can find the source code, license (GPLv2+), and credits for this
code by typing `M-x find-library mercurial RET'."
  (interactive)
  (hg-view-output ("Mercurial Help Overview")
    (insert (documentation 'hg-help-overview))
    (let ((pos (point)))
      (insert (documentation 'hg-mode))
      (goto-char pos)
      (end-of-line 1)
      (delete-region pos (point)))
    (let ((hg-root-dir (hg-root)))
      (if (not hg-root-dir)
	  (error "error: %s: directory is not part of a Mercurial repository."
		 default-directory)
	(cd hg-root-dir)))))

(defun hg-fix-paths ()
  "Fix paths reported by some Mercurial commands."
  (save-excursion
    (goto-char (point-min))
    (while (re-search-forward " \\.\\.." nil t)
      (replace-match " " nil nil))))

(defun hg-add (path)
  "Add PATH to the Mercurial repository on the next commit.
With a prefix argument, prompt for the path to add."
  (interactive (list (hg-read-file-name " to add")))
  (let ((buf (current-buffer))
	(update (equal buffer-file-name path)))
    (hg-view-output (hg-output-buffer-name)
      (apply 'call-process (hg-binary) nil t nil (list "add" path))
      (hg-fix-paths)
      (goto-char (point-min))
      (cd (hg-root path)))
    (when update
      (unless vc-make-backup-files
	(set (make-local-variable 'backup-inhibited) t))
      (with-current-buffer buf
	(hg-mode-line)))))

(defun hg-addremove ()
  (interactive)
  (error "not implemented"))

(defun hg-annotate ()
  (interactive)
  (error "not implemented"))

(defun hg-commit-toggle-file (pos)
  "Toggle whether or not the file at POS will be committed."
  (interactive "d")
  (save-excursion
    (goto-char pos)
    (let (face
	  (inhibit-read-only t)
	  bol)
      (beginning-of-line)
      (setq bol (+ (point) 4))
      (setq face (get-text-property bol 'face))
      (end-of-line)
      (if (eq face 'bold)
	  (progn
	    (remove-text-properties bol (point) '(face nil))
	    (message "%s will not be committed"
		     (buffer-substring bol (point))))
	(add-text-properties bol (point) '(face bold))
	(message "%s will be committed"
		 (buffer-substring bol (point)))))))

(defun hg-commit-mouse-clicked (event)
  "Toggle whether or not the file at POS will be committed."
  (interactive "@e")
  (hg-commit-toggle-file (hg-event-point event)))

(defun hg-commit-kill ()
  "Kill the commit currently being prepared."
  (interactive)
  (when (or (not (buffer-modified-p)) (y-or-n-p "Really kill this commit? "))
    (let ((buf hg-prev-buffer))
      (kill-buffer nil)
      (switch-to-buffer buf))))

(defun hg-commit-finish ()
  "Finish preparing a commit, and perform the actual commit.
The hook hg-pre-commit-hook is run before anything else is done.  If
the commit message is empty and hg-commit-allow-empty-message is nil,
an error is raised.  If the list of files to commit is empty and
hg-commit-allow-empty-file-list is nil, an error is raised."
  (interactive)
  (let ((root hg-root))
    (save-excursion
      (run-hooks 'hg-pre-commit-hook)
      (goto-char (point-min))
      (search-forward hg-commit-message-start)
      (let (message files)
	(let ((start (point)))
	  (goto-char (point-max))
	  (search-backward hg-commit-message-end)
	  (setq message (hg-strip (buffer-substring start (point)))))
	(when (and (= (length message) 0)
		   (not hg-commit-allow-empty-message))
	  (error "Cannot proceed - commit message is empty"))
	(forward-line 1)
	(beginning-of-line)
	(while (< (point) (point-max))
	  (let ((pos (+ (point) 4)))
	    (end-of-line)
	    (when (eq (get-text-property pos 'face) 'bold)
	      (end-of-line)
	      (setq files (cons (buffer-substring pos (point)) files))))
	  (forward-line 1))
	(when (and (= (length files) 0)
		   (not hg-commit-allow-empty-file-list))
	  (error "Cannot proceed - no files to commit"))
	(setq message (concat message "\n"))
	(apply 'hg-run0 "--cwd" hg-root "commit" "-m" message files))
      (let ((buf hg-prev-buffer))
	(kill-buffer nil)
	(switch-to-buffer buf))
      (hg-update-mode-lines root))))

(defun hg-commit-mode ()
  "Mode for describing a commit of changes to a Mercurial repository.
This involves two actions: describing the changes with a commit
message, and choosing the files to commit.

To describe the commit, simply type some text in the designated area.

By default, all modified, added and removed files are selected for
committing.  Files that will be committed are displayed in bold face\;
those that will not are displayed in normal face.

To toggle whether a file will be committed, move the cursor over a
particular file and hit space or return.  Alternatively, middle click
on the file.

Key bindings
------------
\\[hg-commit-finish]		proceed with commit
\\[hg-commit-kill]		kill commit

\\[hg-diff-repo]		view diff of pending changes"
  (interactive)
  (use-local-map hg-commit-mode-map)
  (set-syntax-table text-mode-syntax-table)
  (setq local-abbrev-table text-mode-abbrev-table
	major-mode 'hg-commit-mode
	mode-name "Hg-Commit")
  (set-buffer-modified-p nil)
  (setq buffer-undo-list nil)
  (run-hooks 'text-mode-hook 'hg-commit-mode-hook))

(defun hg-commit-start ()
  "Prepare a commit of changes to the repository containing the current file."
  (interactive)
  (while hg-prev-buffer
    (set-buffer hg-prev-buffer))
  (let ((root (hg-root))
	(prev-buffer (current-buffer))
	modified-files)
    (unless root
      (error "Cannot commit outside a repository!"))
    (hg-sync-buffers root)
    (setq modified-files (hg-chomp (hg-run0 "--cwd" root "status" "-arm")))
    (when (and (= (length modified-files) 0)
	       (not hg-commit-allow-empty-file-list))
      (error "No pending changes to commit"))
    (let* ((buf-name (format "*Mercurial: Commit %s*" root)))
      (pop-to-buffer (get-buffer-create buf-name))
      (when (= (point-min) (point-max))
	(set (make-local-variable 'hg-root) root)
	(setq hg-prev-buffer prev-buffer)
	(insert "\n")
	(let ((bol (point)))
	  (insert hg-commit-message-end)
	  (add-text-properties bol (point) '(face bold-italic)))
	(let ((file-area (point)))
	  (insert modified-files)
	  (goto-char file-area)
	  (while (< (point) (point-max))
	    (let ((bol (point)))
	      (forward-char 1)
	      (insert "  ")
	      (end-of-line)
	      (add-text-properties (+ bol 4) (point)
				   '(face bold mouse-face highlight)))
	    (forward-line 1))
	  (goto-char file-area)
	  (add-text-properties (point) (point-max)
			       `(keymap ,hg-commit-mode-file-map))
	  (goto-char (point-min))
	  (insert hg-commit-message-start)
	  (add-text-properties (point-min) (point) '(face bold-italic))
	  (insert "\n\n")
	  (forward-line -1)
	  (save-excursion
	    (goto-char (point-max))
	    (search-backward hg-commit-message-end)
	    (add-text-properties (match-beginning 0) (point-max)
				 '(read-only t))
	    (goto-char (point-min))
	    (search-forward hg-commit-message-start)
	    (add-text-properties (match-beginning 0) (match-end 0)
				 '(read-only t)))
	  (hg-commit-mode)
          (cd root))))))

(defun hg-diff (path &optional rev1 rev2)
  "Show the differences between REV1 and REV2 of PATH.
When called interactively, the default behaviour is to treat REV1 as
the \"parent\" revision, REV2 as the current edited version of the file, and
PATH as the file edited in the current buffer.
With a prefix argument, prompt for all of these."
  (interactive (list (hg-read-file-name " to diff")
                     (let ((rev1 (hg-read-rev " to start with" 'parent)))
		       (and (not (eq rev1 'parent)) rev1))
		     (let ((rev2 (hg-read-rev " to end with" 'working-dir)))
		       (and (not (eq rev2 'working-dir)) rev2))))
  (hg-sync-buffers path)
  (let ((a-path (hg-abbrev-file-name path))
        ;; none revision is specified explicitly
        (none (and (not rev1) (not rev2)))
        ;; only one revision is specified explicitly
        (one (or (and (or (equal rev1 rev2) (not rev2)) rev1)
                 (and (not rev1) rev2)))
	diff)
    (hg-view-output ((cond
		      (none
		       (format "Mercurial: Diff against parent of %s" a-path))
		      (one
		       (format "Mercurial: Diff of rev %s of %s" one a-path))
		      (t
		       (format "Mercurial: Diff from rev %s to %s of %s"
			       rev1 rev2 a-path))))
      (cond
       (none
        (call-process (hg-binary) nil t nil "diff" path))
       (one
        (call-process (hg-binary) nil t nil "diff" "-r" one path))
       (t
        (call-process (hg-binary) nil t nil "diff" "-r" rev1 "-r" rev2 path)))
      (diff-mode)
      (setq diff (not (= (point-min) (point-max))))
      (font-lock-fontify-buffer)
      (cd (hg-root path)))
    diff))

(defun hg-diff-repo (path &optional rev1 rev2)
  "Show the differences between REV1 and REV2 of repository containing PATH.
When called interactively, the default behaviour is to treat REV1 as
the \"parent\" revision, REV2 as the current edited version of the file, and
PATH as the `hg-root' of the current buffer.
With a prefix argument, prompt for all of these."
  (interactive (list (hg-read-file-name " to diff")
                     (let ((rev1 (hg-read-rev " to start with" 'parent)))
		       (and (not (eq rev1 'parent)) rev1))
		     (let ((rev2 (hg-read-rev " to end with" 'working-dir)))
		       (and (not (eq rev2 'working-dir)) rev2))))
  (hg-diff (hg-root path) rev1 rev2))

(defun hg-forget (path)
  "Lose track of PATH, which has been added, but not yet committed.
This will prevent the file from being incorporated into the Mercurial
repository on the next commit.
With a prefix argument, prompt for the path to forget."
  (interactive (list (hg-read-file-name " to forget")))
  (let ((buf (current-buffer))
	(update (equal buffer-file-name path)))
    (hg-view-output (hg-output-buffer-name)
      (apply 'call-process (hg-binary) nil t nil (list "forget" path))
      ;; "hg forget" shows pathes relative NOT TO ROOT BUT TO REPOSITORY
      (hg-fix-paths)
      (goto-char (point-min))
      (cd (hg-root path)))
    (when update
      (with-current-buffer buf
        (when (local-variable-p 'backup-inhibited)
          (kill-local-variable 'backup-inhibited))
	(hg-mode-line)))))

(defun hg-incoming (&optional repo)
  "Display changesets present in REPO that are not present locally."
  (interactive (list (hg-read-repo-name " where changes would come from")))
  (hg-view-output ((format "Mercurial: Incoming from %s to %s"
			   (hg-abbrev-file-name (hg-root))
			   (hg-abbrev-file-name
			    (or repo hg-incoming-repository))))
    (call-process (hg-binary) nil t nil "incoming"
		  (or repo hg-incoming-repository))
    (hg-log-mode)
    (cd (hg-root))))

(defun hg-init ()
  (interactive)
  (error "not implemented"))

(defun hg-log-mode ()
  "Mode for viewing a Mercurial change log."
  (goto-char (point-min))
  (when (looking-at "^searching for changes.*$")
    (delete-region (match-beginning 0) (match-end 0)))
  (run-hooks 'hg-log-mode-hook))

(defun hg-log (path &optional rev1 rev2 log-limit)
  "Display the revision history of PATH.
History is displayed between REV1 and REV2.
Number of displayed changesets is limited to LOG-LIMIT.
REV1 defaults to the tip, while REV2 defaults to 0.
LOG-LIMIT defaults to `hg-log-limit'.
With a prefix argument, prompt for each parameter."
  (interactive (list (hg-read-file-name " to log")
                     (hg-read-rev " to start with"
                                  "tip")
                     (hg-read-rev " to end with"
				  "0")
                     (hg-read-number "Output limited to: "
                                     hg-log-limit)))
  (let ((a-path (hg-abbrev-file-name path))
        (r1 (or rev1 "tip"))
        (r2 (or rev2 "0"))
        (limit (format "%d" (or log-limit hg-log-limit))))
    (hg-view-output ((if (equal r1 r2)
                         (format "Mercurial: Log of rev %s of %s" rev1 a-path)
                       (format
                        "Mercurial: at most %s log(s) from rev %s to %s of %s"
                        limit r1 r2 a-path)))
      (eval (list* 'call-process (hg-binary) nil t nil
                   "log"
                   "-r" (format "%s:%s" r1 r2)
                   "-l" limit
                   (if (> (length path) (length (hg-root path)))
                       (cons path nil)
                     nil)))
      (hg-log-mode)
      (cd (hg-root path)))))

(defun hg-log-repo (path &optional rev1 rev2 log-limit)
  "Display the revision history of the repository containing PATH.
History is displayed between REV1 and REV2.
Number of displayed changesets is limited to LOG-LIMIT,
REV1 defaults to the tip, while REV2 defaults to 0.
LOG-LIMIT defaults to `hg-log-limit'.
With a prefix argument, prompt for each parameter."
  (interactive (list (hg-read-file-name " to log")
                     (hg-read-rev " to start with"
                                  "tip")
                     (hg-read-rev " to end with"
				  "0")
                     (hg-read-number "Output limited to: "
                                     hg-log-limit)))
  (hg-log (hg-root path) rev1 rev2 log-limit))

(defun hg-outgoing (&optional repo)
  "Display changesets present locally that are not present in REPO."
  (interactive (list (hg-read-repo-name " where changes would go to" nil
					hg-outgoing-repository)))
  (hg-view-output ((format "Mercurial: Outgoing from %s to %s"
			   (hg-abbrev-file-name (hg-root))
			   (hg-abbrev-file-name
			    (or repo hg-outgoing-repository))))
    (call-process (hg-binary) nil t nil "outgoing"
		  (or repo hg-outgoing-repository))
    (hg-log-mode)
    (cd (hg-root))))

(defun hg-pull (&optional repo)
  "Pull changes from repository REPO.
This does not update the working directory."
  (interactive (list (hg-read-repo-name " to pull from")))
  (hg-view-output ((format "Mercurial: Pull to %s from %s"
			   (hg-abbrev-file-name (hg-root))
			   (hg-abbrev-file-name
			    (or repo hg-incoming-repository))))
    (call-process (hg-binary) nil t nil "pull"
		  (or repo hg-incoming-repository))
    (cd (hg-root))))

(defun hg-push (&optional repo)
  "Push changes to repository REPO."
  (interactive (list (hg-read-repo-name " to push to")))
  (hg-view-output ((format "Mercurial: Push from %s to %s"
			   (hg-abbrev-file-name (hg-root))
			   (hg-abbrev-file-name
			    (or repo hg-outgoing-repository))))
    (call-process (hg-binary) nil t nil "push"
		  (or repo hg-outgoing-repository))
    (cd (hg-root))))

(defun hg-revert-buffer-internal ()
  (let ((ctx (hg-buffer-context)))
    (message "Reverting %s..." buffer-file-name)
    (hg-run0 "revert" buffer-file-name)
    (revert-buffer t t t)
    (hg-restore-context ctx)
    (hg-mode-line)
    (message "Reverting %s...done" buffer-file-name)))

(defun hg-revert-buffer ()
  "Revert current buffer's file back to the latest committed version.
If the file has not changed, nothing happens.  Otherwise, this
displays a diff and asks for confirmation before reverting."
  (interactive)
  (let ((vc-suppress-confirm nil)
	(obuf (current-buffer))
	diff)
    (vc-buffer-sync)
    (unwind-protect
	(setq diff (hg-diff buffer-file-name))
      (when diff
	(unless (yes-or-no-p "Discard changes? ")
	  (error "Revert cancelled")))
      (when diff
	(let ((buf (current-buffer)))
	  (delete-window (selected-window))
	  (kill-buffer buf))))
    (set-buffer obuf)
    (when diff
      (hg-revert-buffer-internal))))

(defun hg-root (&optional path)
  "Return the root of the repository that contains the given path.
If the path is outside a repository, return nil.
When called interactively, the root is printed.  A prefix argument
prompts for a path to check."
  (interactive (list (hg-read-file-name)))
  (if (or path (not hg-root))
      (let ((root (do ((prev nil dir)
		       (dir (file-name-directory
                             (or
                              path
                              buffer-file-name
                              (expand-file-name default-directory)))
			    (file-name-directory (directory-file-name dir))))
		      ((equal prev dir))
		    (when (file-directory-p (concat dir ".hg"))
		      (return dir)))))
	(when (interactive-p)
	  (if root
	      (message "The root of this repository is `%s'." root)
	    (message "The path `%s' is not in a Mercurial repository."
		     (hg-abbrev-file-name path))))
	root)
    hg-root))

(defun hg-cwd (&optional path)
  "Return the current directory of PATH within the repository."
  (do ((stack nil (cons (file-name-nondirectory
			 (directory-file-name dir))
			stack))
       (prev nil dir)
       (dir (file-name-directory (or path buffer-file-name
				     (expand-file-name default-directory)))
	    (file-name-directory (directory-file-name dir))))
      ((equal prev dir))
    (when (file-directory-p (concat dir ".hg"))
      (let ((cwd (mapconcat 'identity stack "/")))
	(unless (equal cwd "")
	  (return (file-name-as-directory cwd)))))))

(defun hg-status (path)
  "Print revision control status of a file or directory.
With prefix argument, prompt for the path to give status for.
Names are displayed relative to the repository root."
  (interactive (list (hg-read-file-name " for status" (hg-root))))
  (let ((root (hg-root)))
    (hg-view-output ((format "Mercurial: Status of %s in %s"
			     (let ((name (substring (expand-file-name path)
						    (length root))))
			       (if (> (length name) 0)
				   name
				 "*"))
			     (hg-abbrev-file-name root)))
      (apply 'call-process (hg-binary) nil t nil
	     (list "--cwd" root "status" path))
      (cd (hg-root path)))))

(defun hg-undo ()
  (interactive)
  (error "not implemented"))

(defun hg-update ()
  (interactive)
  (error "not implemented"))

(defun hg-version-other-window (rev)
  "Visit version REV of the current file in another window.
If the current file is named `F', the version is named `F.~REV~'.
If `F.~REV~' already exists, use it instead of checking it out again."
  (interactive "sVersion to visit (default is workfile version): ")
  (let* ((file buffer-file-name)
       	 (version (if (string-equal rev "")
		       "tip"
		        rev))
 	 (automatic-backup (vc-version-backup-file-name file version))
          (manual-backup (vc-version-backup-file-name file version 'manual)))
     (unless (file-exists-p manual-backup)
       (if (file-exists-p automatic-backup)
           (rename-file automatic-backup manual-backup nil)
         (hg-run0 "-q" "cat" "-r" version "-o" manual-backup file)))
     (find-file-other-window manual-backup)))


(provide 'mercurial)


;;; Local Variables:
;;; prompt-to-byte-compile: nil
;;; end:

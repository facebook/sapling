;;; mq.el --- Emacs support for Mercurial Queues

;; Copyright (C) 2006 Bryan O'Sullivan

;; Author: Bryan O'Sullivan <bos@serpentine.com>

;; mq.el is free software; you can redistribute it and/or modify it
;; under the terms of the GNU General Public License version 2 or any
;; later version.

;; mq.el is distributed in the hope that it will be useful, but
;; WITHOUT ANY WARRANTY; without even the implied warranty of
;; MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
;; General Public License for more details.

;; You should have received a copy of the GNU General Public License
;; along with mq.el, GNU Emacs, or XEmacs; see the file COPYING (`C-h
;; C-l').  If not, write to the Free Software Foundation, Inc., 59
;; Temple Place - Suite 330, Boston, MA 02111-1307, USA.

(eval-when-compile (require 'cl))
(require 'mercurial)


(defcustom mq-mode-hook nil
  "Hook run when a buffer enters mq-mode."
  :type 'sexp
  :group 'mercurial)

(defcustom mq-global-prefix "\C-cq"
  "The global prefix for Mercurial Queues keymap bindings."
  :type 'sexp
  :group 'mercurial)

(defcustom mq-edit-mode-hook nil
  "Hook run after a buffer is populated to edit a patch description."
  :type 'sexp
  :group 'mercurial)

(defcustom mq-edit-finish-hook nil
  "Hook run before a patch description is finished up with."
  :type 'sexp
  :group 'mercurial)

(defcustom mq-signoff-address nil
  "Address with which to sign off on a patch."
  :type 'string
  :group 'mercurial)


;;; Internal variables.

(defvar mq-mode nil
  "Is this file managed by MQ?")
(make-variable-buffer-local 'mq-mode)
(put 'mq-mode 'permanent-local t)

(defvar mq-patch-history nil)

(defvar mq-top-patch '(nil))

(defvar mq-prev-buffer nil)
(make-variable-buffer-local 'mq-prev-buffer)
(put 'mq-prev-buffer 'permanent-local t)

(defvar mq-top nil)
(make-variable-buffer-local 'mq-top)
(put 'mq-top 'permanent-local t)

;;; Global keymap.

(defvar mq-global-map
  (let ((map (make-sparse-keymap)))
    (define-key map "." 'mq-push)
    (define-key map ">" 'mq-push-all)
    (define-key map "," 'mq-pop)
    (define-key map "<" 'mq-pop-all)
    (define-key map "=" 'mq-diff)
    (define-key map "r" 'mq-refresh)
    (define-key map "e" 'mq-refresh-edit)
    (define-key map "i" 'mq-new)
    (define-key map "n" 'mq-next)
    (define-key map "o" 'mq-signoff)
    (define-key map "p" 'mq-previous)
    (define-key map "s" 'mq-edit-series)
    (define-key map "t" 'mq-top)
    map))

(global-set-key mq-global-prefix mq-global-map)

(add-minor-mode 'mq-mode 'mq-mode)


;;; Refresh edit mode keymap.

(defvar mq-edit-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map "\C-c\C-c" 'mq-edit-finish)
    (define-key map "\C-c\C-k" 'mq-edit-kill)
    (define-key map "\C-c\C-s" 'mq-signoff)
    map))


;;; Helper functions.

(defun mq-read-patch-name (&optional source prompt force)
  "Read a patch name to use with a command.
May return nil, meaning \"use the default\"."
  (let ((patches (split-string
		  (hg-chomp (hg-run0 (or source "qseries"))) "\n")))
    (when force
      (completing-read (format "Patch%s: " (or prompt ""))
		       (mapcar (lambda (x) (cons x x)) patches)
		       nil
		       nil
		       nil
		       'mq-patch-history))))

(defun mq-refresh-buffers (root)
  (save-excursion
    (dolist (buf (hg-buffers-visiting-repo root))
      (when (not (verify-visited-file-modtime buf))
	(set-buffer buf)
	(let ((ctx (hg-buffer-context)))
	  (message "Refreshing %s..." (buffer-name))
	  (revert-buffer t t t)
	  (hg-restore-context ctx)
	  (message "Refreshing %s...done" (buffer-name))))))
  (hg-update-mode-lines root)
  (mq-update-mode-lines root))

(defun mq-last-line ()
  (goto-char (point-max))
  (beginning-of-line)
  (when (looking-at "^$")
    (forward-line -1))
  (let ((bol (point)))
    (end-of-line)
    (let ((line (buffer-substring bol (point))))
      (when (> (length line) 0)
	line))))

(defun mq-push (&optional patch)
  "Push patches until PATCH is reached.
If PATCH is nil, push at most one patch."
  (interactive (list (mq-read-patch-name "qunapplied" " to push"
					 current-prefix-arg)))
  (let ((root (hg-root))
	(prev-buf (current-buffer))
	last-line ok)
    (unless root
      (error "Cannot push outside a repository!"))
    (hg-sync-buffers root)
    (let ((buf-name (format "MQ: Push %s" (or patch "next patch"))))
      (kill-buffer (get-buffer-create buf-name))
      (split-window-vertically)
      (other-window 1)
      (switch-to-buffer (get-buffer-create buf-name))
      (cd root)
      (message "Pushing...")
      (setq ok (= 0 (apply 'call-process (hg-binary) nil t t "qpush"
			   (if patch (list patch))))
	    last-line (mq-last-line))
      (let ((lines (count-lines (point-min) (point-max))))
	(if (or (<= lines 1)
		(and (equal lines 2) (string-match "Now at:" last-line)))
	    (progn
	      (kill-buffer (current-buffer))
	      (delete-window))
	  (hg-view-mode prev-buf))))
    (mq-refresh-buffers root)
    (sit-for 0)
    (when last-line
      (if ok
	  (message "Pushing... %s" last-line)
	(error "Pushing... %s" last-line)))))

(defun mq-push-all ()
  "Push patches until all are applied."
  (interactive)
  (mq-push "-a"))

(defun mq-pop (&optional patch)
  "Pop patches until PATCH is reached.
If PATCH is nil, pop at most one patch."
  (interactive (list (mq-read-patch-name "qapplied" " to pop to"
					 current-prefix-arg)))
  (let ((root (hg-root))
	last-line ok)
    (unless root
      (error "Cannot pop outside a repository!"))
    (hg-sync-buffers root)
    (set-buffer (generate-new-buffer "qpop"))
    (cd root)
    (message "Popping...")
    (setq ok (= 0 (apply 'call-process (hg-binary) nil t t "qpop"
			 (if patch (list patch))))
	  last-line (mq-last-line))
    (kill-buffer (current-buffer))
    (mq-refresh-buffers root)
    (sit-for 0)
    (when last-line
      (if ok
	  (message "Popping... %s" last-line)
	(error "Popping... %s" last-line)))))

(defun mq-pop-all ()
  "Push patches until none are applied."
  (interactive)
  (mq-pop "-a"))

(defun mq-refresh-internal (root &rest args)
  (hg-sync-buffers root)
  (let ((patch (mq-patch-info "qtop")))
    (message "Refreshing %s..." patch)
    (let ((ret (apply 'hg-run "qrefresh" args)))
      (if (equal (car ret) 0)
	  (message "Refreshing %s... done." patch)
	(error "Refreshing %s... %s" patch (hg-chomp (cdr ret)))))))

(defun mq-refresh (&optional git)
  "Refresh the topmost applied patch.
With a prefix argument, generate a git-compatible patch."
  (interactive "P")
  (let ((root (hg-root)))
    (unless root
      (error "Cannot refresh outside of a repository!"))
    (apply 'mq-refresh-internal root (if git '("--git")))))

(defun mq-patch-info (cmd &optional msg)
  (let* ((ret (hg-run cmd))
	 (info (hg-chomp (cdr ret))))
    (if (equal (car ret) 0)
	(if msg
	    (message "%s patch: %s" msg info)
	  info)
      (error "%s" info))))

(defun mq-top ()
  "Print the name of the topmost applied patch."
  (interactive)
  (mq-patch-info "qtop" "Top"))

(defun mq-next ()
  "Print the name of the next patch to be pushed."
  (interactive)
  (mq-patch-info "qnext" "Next"))

(defun mq-previous ()
  "Print the name of the first patch below the topmost applied patch.
This would become the active patch if popped to."
  (interactive)
  (mq-patch-info "qprev" "Previous"))

(defun mq-edit-finish ()
  "Finish editing the description of this patch, and refresh the patch."
  (interactive)
  (unless (equal (mq-patch-info "qtop") mq-top)
    (error "Topmost patch has changed!"))
  (hg-sync-buffers hg-root)
  (run-hooks 'mq-edit-finish-hook)
  (mq-refresh-internal hg-root "-m" (buffer-substring (point-min) (point-max)))
  (let ((buf mq-prev-buffer))
    (kill-buffer nil)
    (switch-to-buffer buf)))

(defun mq-edit-kill ()
  "Kill the edit currently being prepared."
  (interactive)
  (when (or (not (buffer-modified-p)) (y-or-n-p "Really kill this edit? "))
    (let ((buf mq-prev-buffer))
      (kill-buffer nil)
      (switch-to-buffer buf))))

(defun mq-get-top (root)
  (let ((entry (assoc root mq-top-patch)))
    (if entry
        (cdr entry))))

(defun mq-set-top (root patch)
  (let ((entry (assoc root mq-top-patch)))
    (if entry
        (if patch
            (setcdr entry patch)
          (setq mq-top-patch (delq entry mq-top-patch)))
      (setq mq-top-patch (cons (cons root patch) mq-top-patch)))))

(defun mq-update-mode-lines (root)
  (let ((cwd default-directory))
    (cd root)
    (condition-case nil
        (mq-set-top root (mq-patch-info "qtop"))
      (error (mq-set-top root nil)))
    (cd cwd))
  (let ((patch (mq-get-top root)))
    (save-excursion
      (dolist (buf (hg-buffers-visiting-repo root))
        (set-buffer buf)
        (if mq-mode
            (setq mq-mode (or (and patch (concat " MQ:" patch)) " MQ")))))))
	
(defun mq-mode (&optional arg)
  "Minor mode for Mercurial repositories with an MQ patch queue"
  (interactive "i")
  (cond ((hg-root)
         (setq mq-mode (if (null arg) (not mq-mode)
                         arg))
         (mq-update-mode-lines (hg-root))))
  (run-hooks 'mq-mode-hook))

(defun mq-edit-mode ()
  "Mode for editing the description of a patch.

Key bindings
------------
\\[mq-edit-finish]	use this description
\\[mq-edit-kill]	abandon this description"
  (interactive)
  (use-local-map mq-edit-mode-map)
  (set-syntax-table text-mode-syntax-table)
  (setq local-abbrev-table text-mode-abbrev-table
	major-mode 'mq-edit-mode
	mode-name "MQ-Edit")
  (set-buffer-modified-p nil)
  (setq buffer-undo-list nil)
  (run-hooks 'text-mode-hook 'mq-edit-mode-hook))

(defun mq-refresh-edit ()
  "Refresh the topmost applied patch, editing the patch description."
  (interactive)
  (while mq-prev-buffer
    (set-buffer mq-prev-buffer))
  (let ((root (hg-root))
	(prev-buffer (current-buffer))
	(patch (mq-patch-info "qtop")))
    (hg-sync-buffers root)
    (let ((buf-name (format "*MQ: Edit description of %s*" patch)))
      (switch-to-buffer (get-buffer-create buf-name))
      (when (= (point-min) (point-max))
	(set (make-local-variable 'hg-root) root)
	(set (make-local-variable 'mq-top) patch)
	(setq mq-prev-buffer prev-buffer)
	(insert (hg-run0 "qheader"))
	(goto-char (point-min)))
      (mq-edit-mode)
      (cd root)))
  (message "Type `C-c C-c' to finish editing and refresh the patch."))

(defun mq-new (name)
  "Create a new empty patch named NAME.
The patch is applied on top of the current topmost patch.
With a prefix argument, forcibly create the patch even if the working
directory is modified."
  (interactive (list (mq-read-patch-name "qseries" " to create" t)))
  (message "Creating patch...")
  (let ((ret (if current-prefix-arg
		 (hg-run "qnew" "-f" name)
	       (hg-run "qnew" name))))
    (if (equal (car ret) 0)
	(progn
	  (hg-update-mode-lines (buffer-file-name))
	  (message "Creating patch... done."))
      (error "Creating patch... %s" (hg-chomp (cdr ret))))))

(defun mq-edit-series ()
  "Edit the MQ series file directly."
  (interactive)
  (let ((root (hg-root)))
    (unless root
      (error "Not in an MQ repository!"))
    (find-file (concat root ".hg/patches/series"))))

(defun mq-diff (&optional git)
  "Display a diff of the topmost applied patch.
With a prefix argument, display a git-compatible diff."
  (interactive "P")
  (hg-view-output ((format "MQ: Diff of %s" (mq-patch-info "qtop")))
    (if git
	(call-process (hg-binary) nil t nil "qdiff" "--git")
    (call-process (hg-binary) nil t nil "qdiff"))
    (diff-mode)
    (font-lock-fontify-buffer)))

(defun mq-signoff ()
  "Sign off on the current patch, in the style used by the Linux kernel.
If the variable mq-signoff-address is non-nil, it will be used, otherwise
the value of the ui.username item from your hgrc will be used."
  (interactive)
  (let ((was-editing (eq major-mode 'mq-edit-mode))
	signed)
    (unless was-editing
      (mq-refresh-edit))
    (save-excursion
      (let* ((user (or mq-signoff-address
		       (hg-run0 "debugconfig" "ui.username")))
	     (signoff (concat "Signed-off-by: " user)))
	(if (search-forward signoff nil t)
	    (message "You have already signed off on this patch.")
	  (goto-char (point-max))
	  (let ((case-fold-search t))
	    (if (re-search-backward "^Signed-off-by: " nil t)
		(forward-line 1)
	      (insert "\n")))
	  (insert signoff)
	  (message "%s" signoff)
	  (setq signed t))))
    (unless was-editing
      (if signed
	  (mq-edit-finish)
	(mq-edit-kill)))))


(provide 'mq)


;;; Local Variables:
;;; prompt-to-byte-compile: nil
;;; end:

;;; mq.el --- Emacs support for Mercurial Queues

;; Copyright (C) 2006 Bryan O'Sullivan

;; Author: Bryan O'Sullivan <bos@serpentine.com>

;; mq.el is free software; you can redistribute it and/or modify it
;; under the terms of version 2 of the GNU General Public License as
;; published by the Free Software Foundation.

;; mq.el is distributed in the hope that it will be useful, but
;; WITHOUT ANY WARRANTY; without even the implied warranty of
;; MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
;; General Public License for more details.

;; You should have received a copy of the GNU General Public License
;; along with mq.el, GNU Emacs, or XEmacs; see the file COPYING (`C-h
;; C-l').  If not, write to the Free Software Foundation, Inc., 59
;; Temple Place - Suite 330, Boston, MA 02111-1307, USA.

(require 'mercurial)


(defcustom mq-mode-hook nil
  "Hook run when a buffer enters mq-mode."
  :type 'sexp
  :group 'mercurial)

(defcustom mq-global-prefix "\C-cq"
  "The global prefix for Mercurial Queues keymap bindings."
  :type 'sexp
  :group 'mercurial)


;;; Internal variables.

(defvar mq-patch-history nil)


;;; Global keymap.

(defvar mq-global-map (make-sparse-keymap))
(fset 'mq-global-map mq-global-map)
(global-set-key mq-global-prefix 'mq-global-map)
(define-key mq-global-map "." 'mq-push)
(define-key mq-global-map ">" 'mq-push-all)
(define-key mq-global-map "," 'mq-pop)
(define-key mq-global-map "<" 'mq-pop-all)
(define-key mq-global-map "r" 'mq-refresh)
(define-key mq-global-map "e" 'mq-refresh-edit)
(define-key mq-global-map "n" 'mq-next)
(define-key mq-global-map "p" 'mq-previous)
(define-key mq-global-map "t" 'mq-top)


;;; Helper functions.

(defun mq-read-patch-name (&optional source prompt)
  "Read a patch name to use with a command.
May return nil, meaning \"use the default\"."
  (let ((patches (split-string
		  (hg-chomp (hg-run0 (or source "qseries"))) "\n")))
    (when current-prefix-arg
      (completing-read (format "Patch%s: " (or prompt ""))
		       (map 'list 'cons patches patches)
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
  (hg-update-mode-lines root))

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
  (interactive (list (mq-read-patch-name "qunapplied" " to push")))
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
	(if (and (equal lines 2) (string-match "Now at:" last-line))
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
  (interactive (list (mq-read-patch-name "qapplied" " to pop to")))
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

(defun mq-refresh ()
  "Refresh the topmost applied patch."
  (interactive)
  (let ((root (hg-root)))
    (unless root
      (error "Cannot refresh outside a repository!"))
    (hg-sync-buffers root)
    (message "Refreshing patch...")
    (let ((ret (hg-run "qrefresh")))
      (if (equal (car ret) 0)
	  (message "Refreshing patch... done.")
	(error "Refreshing patch... %s" (hg-chomp (cdr ret)))))))

(defun mq-patch-info (msg cmd)
  (let ((ret (hg-run cmd)))
    (if (equal (car ret) 0)
	(message "%s %s" msg (hg-chomp (cdr ret)))
      (error "%s" (cdr ret)))))

(defun mq-top ()
  "Print the name of the topmost applied patch."
  (interactive)
  (mq-patch-info "Top patch is " "qtop"))

(defun mq-next ()
  "Print the name of the next patch to be pushed."
  (interactive)
  (mq-patch-info "Next patch is " "qnext"))

(defun mq-previous ()
  "Print the name of the first patch below the topmost applied patch.
This would become the active patch if popped to."
  (interactive)
  (mq-patch-info "Previous patch is " "qprev"))

(defun mq-refresh-edit ()
  "Refresh the topmost applied patch, editing the patch description."
  (interactive)
  (error "Not yet implemented"))


(provide 'mq)


;;; Local Variables:
;;; prompt-to-byte-compile: nil
;;; end:

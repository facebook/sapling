;; Copyright (c) Meta Platforms, Inc. and affiliates.
;;
;; This software may be used and distributed according to the terms of the
;; GNU General Public License version 2.

(define-derived-mode dot-t-mode prog-mode "Dot t"
  "Major mode for Sapling .t tests."

  (setq-local delete-trailing-whitespace-on-save nil)
  (setq-local font-lock-defaults '(dot-t-font-lock-keywords))

  ;; Use regexes to override syntax table in certain cases.
  (setq-local font-lock-syntactic-keywords
              `(
                ;; Lines with no whitespace prefix and not starting with "# " are comments.
                ("^\\(?:\\([^#[:space:]]\\)\\|\\(#\\)[[:space:]]\\).*\\(.\\)$"
                 (1 "!" nil t)
                 (2 "!" nil t)
                 (3 "!"))

                ;; Treat ' as string delimiter if the contained string is shortish and doesn't span lines.
                ;; Special case "n't" to not match.
                ("\\(?:n\\)\\('\\)\\(?:t\\)\\|\\('\\)[^\n']\\{1,40\\}\\('\\)"
                 (1 "w" nil t)
                 (2 "\"" nil t)
                 (3 "\"" nil t)))))

(add-to-list 'auto-mode-alist (cons "\\.t\\'" 'dot-t-mode))

(defvar dot-t-mode-map (make-sparse-keymap)
  "Keymap for `dot-t-mode'.")

(defvar dot-t-mode-syntax-table
  (let ((st (make-syntax-table)))
    (modify-syntax-entry ?# "w" st)
    (modify-syntax-entry ?- "w" st)
    (modify-syntax-entry ?_ "w" st)
    st))

(defun dot-t-font-lock-keywords ()
  `(
    ;; # directives like "#if" are keywords.
    ("^#\\w+" . font-lock-keyword-face)

    ;; Leading "$" and later "<", ">", "|", "&" and "$(" are keywords.
    ("^\\s-+\\$\\s-"
     (0 font-lock-keyword-face)
     ("\\([<>|&]\\)\\|\\(\\$\\)(" nil nil
      (1 font-lock-keyword-face nil t)
      (2 font-lock-keyword-face nil t)))

    ;; in "$ hg foo", "hg" is builtin, "foo" is function.
    ("^\\s-+\\$\\s-.*?+\\_<\\(hg\\|sl\\)\\_>.*? \\(\\w+\\)[[:space:]$]"
     (1 font-lock-builtin-face)
     (2 font-lock-function-name-face))

    ;; CLI flags like "--verbose" are variable name.
    ("^  \\$" (" -[^[:space:]]+" nil nil (0 font-lock-variable-name-face)))

    ;; In "$ ENV=var foo", "$" is keyword, "foo" is function.
    ("^\\s-+\\$\\(?:[[:space:]]\\|[A-Z]+=[^[:space:]]+\\)+.*?\\([a-z]\\w+\\)"
     (1 font-lock-function-name-face))

    ;; In "foo.bar=", "foo.bar" is variable name.
    ("\\([^.[:space:]]+\\.[^[:space:]=]+\\)=" 1 font-lock-variable-name-face)

    (dot-t-match-heredoc (1 font-lock-string-face t t))

    ("  >>> " . font-lock-keyword-face)

    ;; "$FOO" is variable use.
    ("\\$[A-Z_-]+" . 'font-lock-variable-use-face)

    ;; In "FOO=bar", "FOO" is variable name, "bar" is string.
    ("[[:space:]]\\([A-Z_-]+\\)=\\([^[:space:]]+\\)"
     (1 font-lock-variable-name-face)
     (2 font-lock-string-face))

    ;; Exit codes are warning face.
    ("^  \\[[0-9]+\\]" . font-lock-warning-face)

    ("/dev/null" . font-lock-constant-face)

    ;; Output suffix things like "(glob)" are builtin.
    ("^.*?[^[:space:]].*?\\(\\(\\s-+([^', )]+\\(?: !\\)?)\\)+\\)$" (1 font-lock-builtin-face))

    ;; Hashes are warning face.
    ("[0-9a-f]\\{12,40\\}" . font-lock-warning-face)))

(defun dot-t-match-heredoc (end)
  "Find opening heredoc, find closing, and then mutate match data
 to contain entire doc."
  (let (found-match)
    (while (and
            (not found-match)
            (re-search-forward "<<\\s-*['\"]?\\(\\w+\\)" end t))
      (let* ((token (match-string 1))
             (start (- (point) 2 (length token)))
             (md (match-data)))
        (when (search-forward token end t)
          (setq found-match t)
          (setf (nth 2 md) start (nth 3 md) (point))
          (set-match-data md))))
    found-match))

(defcustom dot-t-sl-command (or (getenv "SL") "sl")
  "Path to sl binary."
  :type 'string
  :group 'sapling)

(defun dot-t-debug-test ()
  "Open interactive shell to debug test at current line."
  ;; TODO: support tramp
  (interactive)
  (let* ((file (buffer-file-name))
         (output-buf-name "*debugrestoretest*")
         (error-file-name "/tmp/debugrestoretest-stderr")
         (ret (call-process
               dot-t-sl-command nil `(,output-buf-name ,error-file-name) nil
               "debugrestoretest"
               "--line" (number-to-string (line-number-at-pos))
               "--record-if-needed"
               file)))
    (if (= ret 0)
        (let ((vterm-shell (with-current-buffer output-buf-name (buffer-string))))
          (vterm-other-window  (concat "*debug " file "*")))
      (message "debugrestoretest failed - see %s" error-file-name))
    (kill-buffer output-buf-name)))

(define-compilation-mode dot-t-compilation-mode "Dot t compilation"
  "Dot-t compilation mode."

  (setq-local compilation-error-regexp-alist-alist nil)
  (setq-local compilation-error-regexp-alist nil)

  (add-to-list 'compilation-error-regexp-alist 'dot-t)
  ;; Make `"/some/path.py", line 123` into a link.
  ;; Make `/some/path.py:123` into a link.
  (add-to-list 'compilation-error-regexp-alist-alist
               '(dot-t . ("\\(/.*?\\.py\\)\\(?:\", line \\|:\\)\\([0-9]+\\)" 1 2)))

  (add-hook 'compilation-filter-hook #'ansi-color-compilation-filter nil t))

(defun dot-t-run-test ()
  "Run current test file.

Add prefix arg to run with --fix.
"
  (interactive)

  (let (args (file (buffer-file-name)))
    (when (file-remote-p file)
      (setq file (tramp-file-name-localname (tramp-dissect-file-name file))))

    (if (save-excursion (goto-char (point-min)) (search-forward "#debugruntest-incompatible" nil t))
        (let ((run-tests-py (concat (locate-dominating-file file "run-tests.py") "run-tests.py")))
          (setq args (list run-tests-py
                           "--noprogress"
                           "--maxdifflines" "10000"
                           "--with-hg" (executable-find dot-t-sl-command)
                           ;; "--chg" ; makes tests hang for some reason
                           ))
          (when (save-excursion (goto-char (point-min)) (search-forward "#require fsmonitor" nil t))
            (setq args (append args '("--watchman")))))
      (setq args (list dot-t-sl-command
                       ".t"
                       "--config" "testing.max-mismatch-per-file=10000")))

    (when current-prefix-arg
      (setq args (append args '("--fix"))))

    (setq args (append args (list file)))

    (let ((process-environment (cons "TERM=xterm-256color" process-environment)))
      (compilation-start (string-join args " ") 'dot-t-compilation-mode))))

(provide 'dot-t-mode)

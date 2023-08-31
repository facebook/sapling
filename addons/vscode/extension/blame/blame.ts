/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {VSCodeReposList} from '../VSCodeRepo';
import type {Repository} from 'isl-server/src/Repository';
import type {ServerSideTracker} from 'isl-server/src/analytics/serverSideTracker';
import type {Logger} from 'isl-server/src/logger';
import type {CommitInfo, Result} from 'isl/src/types';
import type {
  DecorationOptions,
  Disposable,
  TextDocument,
  TextDocumentChangeEvent,
  TextEditor,
  TextEditorSelectionChangeEvent,
} from 'vscode';

import {Internal} from '../Internal';
import {getDiffBlameHoverMarkup} from './blameHover';
import {getRealignedBlameInfo, shortenAuthorName} from './blameUtils';
import {getUsername} from 'isl-server/src/analytics/environment';
import {relativeDate} from 'isl/src/relativeDate';
import {LRU} from 'shared/LRU';
import {debounce} from 'shared/debounce';
import {unwrap} from 'shared/utils';
import {DecorationRangeBehavior, MarkdownString, Position, Range, window, workspace} from 'vscode';

function areYouTheAuthor(author: string) {
  const you = getUsername();
  return author.includes(you);
}

type BlameText = {
  inline: string;
  hover: string;
};
type RepoCaches = {
  headHash: string;
  blameCache: LRU<string, CachedBlame>; // Caches file path -> file blame.
};
type CachedBlame = {
  baseBlameLines: Array<[line: string, info: CommitInfo | undefined]>;
  currentBlameLines:
    | undefined // undefined if not yet populated with local changes.
    | Array<[line: string, info: CommitInfo | undefined]>; // undefined entry represents locally changed lines.
};

const MAX_NUM_FILES_CACHED = 20;

/**
 * Provides inline blame annotations.
 *
 * Blame is fetched via `sl blame` once per file,
 * based on the current commit hash in your stack.
 * Blame is invalidated if the head commit changes (commit, amend, goto, ...).
 *
 * The current file's content at the current commit is available as part of the fetched blame,
 * and used to diff with the current file text editor contents.
 * This difference is used to realign the blame to include your local uncommitted changes.
 * This diff is run on every text change to the file.
 *
 * One line of blame is rendered next to your cursor, and re-rended every time the cursor moves.
 *
 * TODO: instead of diffing with the current file contents, we could instead record all your edits
 * into linelog, and derive blame for that. That would give us timestamps for each change and a way
 * to quickly go backwards in time.
 */
export class InlineBlameProvider implements Disposable {
  loadingEditors = new Set<string>();

  currentEditor: TextEditor | undefined;
  currentRepo: Repository | undefined;
  currentPosition: Position | undefined;

  filesBeingProcessed = new Set<string>();
  disposables: Array<Disposable> = [];
  observedRepos = new Map<string, RepoCaches>();
  decorationType = window.createTextEditorDecorationType({});

  constructor(
    private reposList: VSCodeReposList,
    private logger: Logger,
    private tracker: ServerSideTracker,
  ) {
    this.initBasedOnConfig();
  }

  initBasedOnConfig() {
    const config = 'sapling.showInlineBlame';
    const enableBlameByDefault =
      Internal?.shouldEnableBlameByDefault == null
        ? /* OSS */ true
        : Internal?.shouldEnableBlameByDefault();
    if (workspace.getConfiguration().get<boolean>(config, enableBlameByDefault)) {
      this.init();
    }
    this.disposables.push(
      workspace.onDidChangeConfiguration(configChange => {
        if (configChange.affectsConfiguration(config)) {
          workspace.getConfiguration().get<boolean>(config) ? this.init() : this.deinit();
        }
      }),
    );
  }

  init(): void {
    // Since VS Code sometimes opens on a file, we need to ensure that file's blame is loaded
    const activeEditor = window.activeTextEditor;
    if (activeEditor && this.isFile(activeEditor)) {
      this.loadingEditors.add(activeEditor.document.uri.fsPath);
      this.switchFocusedEditor(activeEditor);
    }

    const debouncedChangeActiveEditor = debounce((textEditor: TextEditor | undefined) => {
      this.switchFocusedEditor(textEditor);
    }, 1500); // TODO: we could use a longer debounce for new files which aren't cached & shorter debounce if cached.
    this.disposables.push(
      window.onDidChangeActiveTextEditor(textEditor => {
        if (textEditor && this.isFile(textEditor)) {
          // There is a debounce period before any processing is done on the editor.
          // loadingEditors is used to "lock" keystroke actions during the debounce
          // period, as well as before the editor finished processing.
          this.loadingEditors.add(textEditor.document.uri.fsPath);
        }
        this.currentEditor = undefined;
        this.decorationType.dispose();

        debouncedChangeActiveEditor(textEditor);
      }),
    );

    // change the current line's blame when moving the cursor
    const debouncedOnChangeSelection = debounce((event: TextEditorSelectionChangeEvent) => {
      const selection = event.selections[0];
      const activePosition = selection.active;
      if (
        !this.currentPosition ||
        (selection.isEmpty && activePosition.line !== this.currentPosition.line)
      ) {
        this.currentPosition = activePosition;
        const uri = event.textEditor.document.uri.fsPath;
        if (event.kind && !this.loadingEditors.has(uri) && this.isUriCached(uri)) {
          this.showBlameAtPos(activePosition);
        }
      }
    }, 50);
    this.disposables.push(
      window.onDidChangeTextEditorSelection(e => {
        if (e.kind != null) {
          debouncedOnChangeSelection(e);
        }
      }),
    );

    // update blame offsets if the document is changed (to account for local changes)
    const debouncedOnTextDocumentChange = debounce((event: TextDocumentChangeEvent) => {
      // precondition: event is a file:// scheme change
      const uri = event.document.uri.fsPath;
      if (this.loadingEditors.has(uri) || !this.isUriCached(uri)) {
        return;
      }

      // Document has been modified, so update that file's blame before showing annotation.
      this.updateBlame(event.document);
      if (this.currentPosition) {
        this.showBlameAtPos(this.currentPosition);
      }
    }, 500);
    this.disposables.push(
      workspace.onDidChangeTextDocument(e => {
        if (this.isFile(e)) {
          this.decorationType.dispose(); // dispose decorations on any change without waiting for debouncing
          debouncedOnTextDocumentChange(e);
        }
      }),
    );

    this.logger.info('Initialized inline blame');
  }

  deinit(): void {
    this.decorationType.dispose();
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
    this.disposables = [];

    this.observedRepos.clear();
    this.loadingEditors.clear();
    this.filesBeingProcessed.clear();

    this.currentRepo = undefined;
    this.currentEditor = undefined;
    this.currentPosition = undefined;
  }

  private async switchFocusedEditor(textEditor: TextEditor | undefined): Promise<void> {
    this.currentEditor = textEditor;
    this.currentPosition = textEditor?.selection.active;
    if (textEditor && this.isFile(textEditor) && this.currentPosition) {
      const foundBlame = await this.fetchBlameIfMissing(textEditor);
      if (foundBlame) {
        // Update blame before showing incase keystrokes were pressed on load.
        this.updateBlame(textEditor.document);
        this.showBlameAtPos(this.currentPosition);
      }
      // Editor is finished processing, so remove it from loadingEditors.
      this.loadingEditors.delete(textEditor.document.uri.fsPath);
    }
  }

  /**
   * blame is fetched by calling `sl blame` only when the head commit or active file changes,
   * but not if the cursor moves or local edits are made.
   */
  private async fetchBlameIfMissing(textEditor: TextEditor): Promise<boolean> {
    const uri = textEditor.document.uri;
    const fileUri = uri.fsPath;
    if (this.filesBeingProcessed.has(fileUri)) {
      return false;
    }
    this.filesBeingProcessed.add(fileUri);

    const repo = this.reposList.repoForPath(fileUri)?.repo;
    if (!repo) {
      this.logger.warn(`Could not fetch Blame: No repository found for path ${uri.fsPath}.`);
      this.filesBeingProcessed.delete(fileUri);
      return false;
    }

    this.currentRepo = repo;
    const repoUri = repo.info.repoRoot;

    if (this.isUriCached(fileUri)) {
      this.filesBeingProcessed.delete(fileUri);
      return true;
    } else if (!this.observedRepos.has(repoUri)) {
      // If we have found a new repo, subscribe to that repo before continuing.
      this.subscribeToRepo(repo);
    }

    if (fileUri.endsWith('.code-workspace')) {
      // workspace files are unrecognized.
      this.filesBeingProcessed.delete(fileUri);
      return false;
    }

    const path = uri.fsPath;
    const startTime = Date.now();

    const repoCaches = this.observedRepos.get(repoUri);
    if (repoCaches == null) {
      this.logger.warn(`Could not fetch Blame: repo not in cache.`);
      return false;
    }

    const blame = await this.getBlame(textEditor, repoCaches?.headHash);

    if (blame.error) {
      this.tracker.error('BlameLoaded', 'BlameError', blame.error.message, {
        duration: Date.now() - startTime,
      });
    } else {
      this.tracker.track('BlameLoaded', {
        duration: Date.now() - startTime,
      });
    }

    this.filesBeingProcessed.delete(fileUri);
    if (blame.error) {
      if (blame.error.name === 'No Blame') {
        this.logger.info(`No blame found for path ${path}`, blame.error.message);
      } else {
        const message = `Failed to fetch Blame for path ${path}`;
        this.logger.error(`${message}: `, blame.error.message);
        return false;
      }
    } else if (blame.value.length === 0) {
      this.logger.info(`No blame found for path ${path}`);
    }

    const blameLines = unwrap(blame.value);

    repoCaches.blameCache.set(fileUri, {
      baseBlameLines: blameLines,
      currentBlameLines: undefined,
    });
    return true;
  }

  private async getBlame(
    textEditor: TextEditor,
    baseHash: string,
  ): Promise<Result<Array<[line: string, commit: CommitInfo | undefined]>>> {
    const uri = textEditor.document.uri.fsPath;
    const repo = this.reposList.repoForPath(uri)?.repo;
    try {
      return {value: await unwrap(repo).blame(uri, baseHash)};
    } catch (err: unknown) {
      return {error: err as Error};
    }
  }

  private showBlameAtPos(position: Position): void {
    this.decorationType.dispose();
    if (!this.currentEditor) {
      return;
    }
    const blameText = this.getBlameText(position.line);
    if (!blameText) {
      return;
    }

    const endChar = this.currentEditor.document.lineAt(position).range.end.character;
    const endPosition = new Position(position.line, endChar);

    const range = new Range(endPosition, endPosition);
    const hoverMessage = new MarkdownString(blameText.hover);
    const decorationOptions: DecorationOptions = {range, hoverMessage};

    this.assignDecorationType(blameText.inline);
    this.currentEditor.setDecorations(this.decorationType, [decorationOptions]);
  }

  private assignDecorationType(contentText: string): void {
    if (!this.currentEditor) {
      return;
    }
    const margin = '0 0 0 3vw';
    this.decorationType = window.createTextEditorDecorationType({
      dark: {
        after: {
          color: '#ffffff33',
          contentText,
          margin,
        },
      },
      light: {
        after: {
          color: '#00000033',
          contentText,
          margin,
        },
      },
      rangeBehavior: DecorationRangeBehavior.ClosedOpen,
    });
  }

  private getBlameText(line: number): BlameText | undefined {
    if (this.currentRepo == null) {
      return undefined;
    }
    const repoCaches = this.observedRepos.get(this.currentRepo.info.repoRoot);
    if (!this.currentEditor || !repoCaches) {
      return undefined;
    }
    const uri = this.currentEditor.document.uri;
    const revisionSet = repoCaches.blameCache.get(uri.fsPath);
    if (!revisionSet) {
      return undefined;
    }

    if (revisionSet.currentBlameLines == null) {
      this.updateBlame(this.currentEditor.document);
    }

    const blameLines = unwrap(revisionSet.currentBlameLines);
    if (line >= blameLines.length) {
      return undefined;
    }
    const commit = blameLines[line][1];
    if (!commit) {
      return {inline: `(you) \u2022 Local Changes`, hover: ''};
    }

    const DOT = '\u2022';
    try {
      const inline = `${this.authorHint(commit.author)}${relativeDate(
        commit.date,
        {},
      )} ${DOT} ${commit.title.trim()}`;
      const hover = getDiffBlameHoverMarkup(this.currentRepo, commit);
      const blameText = {inline, hover};
      return blameText;
    } catch (err) {
      this.logger.error('Error getting blame text:', err);
      return undefined;
    }
  }

  private authorHint(author: string): string {
    if (areYouTheAuthor(author)) {
      return '(you) ';
    }
    if (Internal?.showAuthorNameInInlineBlame?.() === false) {
      // Internally, don't show author inline unless it's you. Hover to see the author.
      return '';
    }
    return shortenAuthorName(author) + ', ';
  }

  private initRepoCaches(repoUri: string): void {
    const caches: RepoCaches = {
      headHash: '',
      blameCache: new LRU(MAX_NUM_FILES_CACHED),
    };
    this.observedRepos.set(repoUri, caches);
  }

  private subscribeToRepo(repo: Repository): void {
    const repoUri = repo.info.repoRoot;
    this.initRepoCaches(repoUri);

    this.disposables.push(
      repo.subscribeToHeadCommit(head => {
        const repoCaches = this.observedRepos.get(repoUri);
        if (!repoCaches) {
          return;
        }

        if (head.hash === repoCaches.headHash) {
          // Same head means the blame can't have changed.
          return;
        }

        if (repoCaches.headHash !== '') {
          this.logger.info('Head commit changed, invaldating blame.');
        }

        repoCaches.headHash = head.hash;
        repoCaches.blameCache.clear();
        this.switchFocusedEditor(window.activeTextEditor);
      }),
    );
  }

  private isUriCached(uri: string): boolean {
    for (const repoCaches of this.observedRepos.values()) {
      if (repoCaches.blameCache.get(uri) != null) {
        return true;
      }
    }
    return false;
  }

  private isFile(editorInstance: {document: TextDocument}): boolean {
    return editorInstance.document.uri.scheme === 'file';
  }

  private updateBlame(document: TextDocument): void {
    const uri = document.uri.fsPath;
    if (this.filesBeingProcessed.has(uri)) {
      return;
    }
    this.filesBeingProcessed.add(uri);
    const cachedBlame = this.getCachedBlame(document);
    if (!cachedBlame) {
      this.filesBeingProcessed.delete(uri);
      return;
    }

    const newRevisionInfo = getRealignedBlameInfo(cachedBlame.baseBlameLines, document.getText());

    cachedBlame.currentBlameLines = newRevisionInfo;
    this.filesBeingProcessed.delete(uri);
  }

  private getCachedBlame(document: TextDocument): CachedBlame | undefined {
    const uri = document.uri.fsPath;
    for (const repoCaches of this.observedRepos.values()) {
      if (repoCaches.blameCache.get(uri) != null) {
        return repoCaches.blameCache.get(uri);
      }
    }
    return undefined;
  }

  dispose(): void {
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
    this.disposables = [];
    this.decorationType.dispose();
  }
}

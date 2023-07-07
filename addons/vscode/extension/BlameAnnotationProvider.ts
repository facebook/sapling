import type {Logger} from 'isl-server/src/logger';
import type {TextEditorSelectionChangeEvent} from 'vscode';

import {repositoryCache} from 'isl-server/src/RepositoryCache';
import {Range, Disposable, MarkdownString, window} from 'vscode';

const annotationDecoration = window.createTextEditorDecorationType({
  after: {
    margin: '0px 0px 0px 30px',
    color: 'rgba(156,156,156,.4)',
  },
});

class BlameAnnotationProvider implements Disposable {
  private disposables: Disposable[] = [];
  private annotation: Disposable | undefined;
  constructor(private logger: Logger) {
    this.disposables.push(
      Disposable.from(
        window.onDidChangeTextEditorSelection(this.onTextEditorSelectionChanged, this),
      ),
    );
  }
  private async onTextEditorSelectionChanged(e: TextEditorSelectionChangeEvent) {
    try {
      const editor = e.textEditor;
      const document = editor.document;
      const repo = repositoryCache.cachedRepositoryForPath(document.uri.fsPath);
      if (!repo) {
        return;
      }

      this.annotation?.dispose();
      let isDisposed = false;
      this.annotation = {dispose: () => (isDisposed = true)};

      if (document.isDirty) {
        return;
      }
      const line = e.selections[0].active.line;
      const fileBlame = await repo.fetchBlame(document.uri.fsPath);

      const lineBlame = fileBlame?.[line];
      if (!lineBlame || isDisposed) {
        return;
      }
      const date = formatTimeSince(new Date(lineBlame.date));
      const commit = await repo.fetchCommit(lineBlame.node);
      if (isDisposed) {
        return;
      }
      const commitUserName = commit?.author.replace(/ <.*>/, '') ?? 'Unknown author';

      // TODO: Is there a better way to get the diff/PR number from public commits?
      const diffId = commit?.diffId ?? commit?.title?.match(/#(\d+)/)?.[1];
      const reviewLink =
        diffId && repo.codeReviewProvider ? repo.codeReviewProvider.getDiffUrl(diffId) : '';

      const markdown = new MarkdownString(
        `### ${commitUserName}, ${date}${
          diffId != null ? ` via PR [#${diffId}](${reviewLink})` : ''
        }\n#### ${commit?.title}\n\n${commit?.description}\n\n`,
      );
      const inlineText = `${commitUserName}, ${date}${
        diffId != null ? ' via PR #' + diffId : ''
      } • ${truncate(commit?.title ?? 'Unknown', 50)}`;

      editor.setDecorations(annotationDecoration, [
        {
          range: new Range(line, 10000000, line, 10000000), // Necessary so hover message doesn't trigger on the entire line
          hoverMessage: markdown,
          renderOptions: {
            after: {
              contentText: inlineText,
            },
          },
        },
      ]);
      this.annotation = {dispose: () => editor.setDecorations(annotationDecoration, [])};
    } catch (err) {
      this.logger.error('Error while handling blame annotation', err);
    }
  }
  dispose() {
    this.disposables.forEach(d => d.dispose());
    this.annotation?.dispose();
  }
}

export function registerBlameAnnotationProvider(logger: Logger) {
  return new BlameAnnotationProvider(logger);
}

const DATE_UNITS = [
  {unit: 'second', ms: 1000},
  {unit: 'minute', ms: 60 * 1000},
  {unit: 'hour', ms: 60 * 60 * 1000},
  {unit: 'day', ms: 24 * 60 * 60 * 1000},
  {unit: 'week', ms: 7 * 24 * 60 * 60 * 1000},
  {unit: 'month', ms: 30 * 24 * 60 * 60 * 1000},
  {unit: 'year', ms: 365 * 24 * 60 * 60 * 1000},
];

function formatTimeSince(date: Date) {
  const timeSince = Date.now() - date.getTime();
  const unit = DATE_UNITS.findLast(unit => unit.ms <= timeSince) ?? DATE_UNITS[0];
  const value = Math.round(timeSince / unit.ms);
  return `${value} ${unit.unit}${value !== 1 ? 's' : ''} ago`;
}

function truncate(str: string, n: number) {
  return str.length > n ? str.substring(0, n - 1) + '...' : str;
}

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useAtomValue} from 'jotai';
import clientToServerAPI from '../ClientToServerAPI';
import {T} from '../i18n';
import {writeAtom} from '../jotaiUtils';

import {ErrorNotice} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {minimalDisambiguousPaths} from 'shared/minimalDisambiguousPaths';
import {tracker} from '../analytics';
import {Collapsable} from '../Collapsable';
import {relativePath} from '../CwdSelector';
import {Link} from '../Link';
import platform from '../platform';
import {repoRelativeCwd} from '../repositoryData';
import {registerDisposable} from '../utils';
import './AICodeReviewStatus.css';
import {
  codeReviewStatusAtom,
  commentsByFilePathAtom,
  firstPassCommentData,
  firstPassCommentDataCount,
  firstPassCommentError,
} from './firstPassCodeReviewAtoms';

registerDisposable(
  firstPassCommentData,
  clientToServerAPI.onMessageOfType('platform/gotAIReviewComments', data => {
    const result = data.comments;
    if (result.error) {
      writeAtom(codeReviewStatusAtom, 'error');
      writeAtom(firstPassCommentError, result.error);
      tracker.error('AICodeReviewCompleted', 'AICodeReviewError', result.error);
    } else {
      writeAtom(codeReviewStatusAtom, 'success');
      tracker.track('AICodeReviewCompleted', {extras: {commentCount: result.value.length}});
    }
  }),
  import.meta.hot,
);

export function AICodeReviewStatus(): JSX.Element | null {
  const repoRoot = useAtomValue(repoRelativeCwd);
  const status = useAtomValue(codeReviewStatusAtom);
  const commentCount = useAtomValue(firstPassCommentDataCount);
  const commentsByFilePath = useAtomValue(commentsByFilePathAtom);
  const error = useAtomValue(firstPassCommentError);
  const disambiguatedPaths = minimalDisambiguousPaths(Object.keys(commentsByFilePath));

  // TODO: move this component to vscode/webview
  if (platform.platformName !== 'vscode' || status == null) {
    return null;
  }

  return (
    <Collapsable
      className="comment-collapsable"
      title={
        <div className="comment-collapsible-title">
          <b>
            <T>Devmate Code Review</T>
          </b>
          <div className="comment-collapsible-title-status">
            {status === 'success' &&
              (commentCount > 0 ? (
                <div className="comment-count">
                  {commentCount}
                  <Icon icon="comment" />
                </div>
              ) : (
                <Icon icon="check" />
              ))}
            {status === 'error' && <Icon icon="error" color="red" />}
            {status === 'running' && <Icon icon="loading" />}
          </div>
        </div>
      }>
      <div className="comment-content-container">
        {status === 'running' && (
          <div className="comment-loading">Devmate is reviewing your code...</div>
        )}
        {status === 'success' &&
          (commentCount > 0 ? (
            <div className="comment-list">
              {Object.entries(commentsByFilePath).map(([filepath, comments], i) =>
                comments.map((comment, j) => (
                  <div className="comment-container" key={comment.issueID || `${filepath}-${j}`}>
                    <div className="comment-header">
                      <Link
                        onClick={() =>
                          platform.openFile(relativePath(repoRoot, filepath), {
                            line: comment.startLine,
                          })
                        }>
                        <b>
                          {disambiguatedPaths[i]}:{comment.startLine}
                        </b>
                      </Link>
                    </div>
                    <div className="comment-body">
                      <T>{comment.description}</T>
                    </div>
                  </div>
                )),
              )}
            </div>
          ) : (
            <div>
              <T>Everything looks good! Devmate didn't find any issues.</T>
            </div>
          ))}
        {status === 'error' && <ErrorNotice title="Failed to load comments" error={error} />}
      </div>
    </Collapsable>
  );
}

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import type {ParsedDiff} from 'diff';
import {Icon} from 'isl-components/Icon';
import type {ReactNode} from 'react';
import {forwardRef, lazy, Suspense} from 'react';
import {ComparisonType} from 'shared/Comparison';
import type {DiffViewMode} from '../ComparisonView/SplitDiffView/types';
import {Column, Row} from '../ComponentUtils';
import type {ThemeColor} from '../theme';
import type {CodeChange, DiffComment, SuggestedChange} from '../types';
import {CodePatchSuggestionStatus, SuggestedChangeType} from '../types';
import InlineCommentContent from './InlineCommentContent';
import InlineCommentSuggestionActionBottomBar from './InlineCommentSuggestionActionBottomBar';

const InlineCommentComparisonView = lazy(() => import('./InlineCommentComparisonView'));

const styles = stylex.create({
  alignTop: {alignItems: 'flex-start'},
  subheadingsAlignBaseline: {alignItems: 'baseline', gap: '4px'},
  tooltipBar: {width: '100%', justifyContent: 'space-between'},
  boldText: {fontWeight: '700', fontSize: '100%'},
  subtle: {fontSize: '90%', opacity: 0.8},
  headerRow: {
    justifyContent: 'space-between',
    alignItems: 'start',
    wordBreak: 'break-word',
  },
  headerContent: {
    maxWidth: '320px',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  headerControl: {
    gap: '4px',
  },
  shortCutTipLink: {
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    fontSize: '12px',
    color: 'var(--vscode-descriptionForeground)',
    cursor: 'pointer',
  },
  underlineOnHoverText: {
    textDecoration: {
      ':hover': 'underline',
    },
  },
});

export const InlineComment = forwardRef(
  (
    {
      comment,
      versionInfo,
      collapsed,
      diffViewMode,
      onAccept,
      onReject,
      onResolve,
      onUnresolve,
      onClickHeader,
      headerControls,
      bottomControls,
      useThemeHook,
    }: {
      comment: DiffComment;
      versionInfo?: {
        isLatestVersion?: boolean;
        versionAbbr?: string;
      };
      collapsed: boolean;
      diffViewMode: DiffViewMode;

      onAccept: (codeSuggestion?: SuggestedChange) => unknown;
      onReject: (codeSuggestion?: SuggestedChange) => unknown;
      onResolve: () => unknown;
      onUnresolve: () => unknown;
      onClickHeader: () => unknown;

      headerControls: ReactNode;
      bottomControls: ReactNode;

      useThemeHook: () => ThemeColor;
    },
    ref: React.ForwardedRef<HTMLDivElement>,
  ) => {
    const path = comment?.filename ?? '';
    const codeSuggestion = comment?.suggestedChange ?? null;
    const authorName = comment.authorName;
    const codeChange = codeSuggestion?.codeChange;

    const renderDiffView = (codeChange: CodeChange[]) => {
      const changes = codeChange?.filter(
        (change): change is CodeChange & {patch: ParsedDiff} => change.patch != null,
      );

      if (changes == null || changes.length === 0) {
        return null;
      }

      return changes.map((change, i) => {
        return (
          <div key={i}>
            {changes.length === 1 ? null : (
              <div {...stylex.props(styles.boldText)}>Change {i + 1}</div>
            )}
            <InlineCommentComparisonView
              path={path}
              suggestion={change.patch}
              ctx={{
                collapsed: false,
                displayLineNumbers: changes.length > 1, // TODO: currently this line number is not aligned value
                id: {
                  comparison: {type: ComparisonType.HeadChanges},
                  path,
                },
                setCollapsed: () => null,
                display: diffViewMode,
                useThemeHook,
              }}
            />
          </div>
        );
      });
    };

    return (
      <div
        ref={ref}
        className="container"
        style={{width: collapsed || diffViewMode === 'unified' ? 600 : 1000}}>
        {collapsed ? (
          <div className="headerRow" onClick={onClickHeader}>
            <div className="headerLeftContent">
              <Icon icon="comment" />
              <div className="headerTitle">
                <span {...stylex.props(styles.boldText)}>{authorName}</span>
                {comment.content != null && comment.content !== '' && (
                  <div {...stylex.props(styles.subtle, styles.headerContent)}>
                    {comment.content}
                  </div>
                )}
              </div>
            </div>
            <Row xstyle={styles.headerControl}>{headerControls}</Row>
          </div>
        ) : (
          <>
            <Row xstyle={styles.headerRow}>
              <Column alignStart style={{marginBlock: '8px'}}>
                <InlineCommentContent
                  comment={comment}
                  isHeadComment={true}
                  isLatestVersion={versionInfo?.isLatestVersion}
                  versionAbbr={versionInfo?.versionAbbr}
                />
                {comment.replies.map((reply, i) => (
                  <InlineCommentContent comment={reply} key={i} />
                ))}
              </Column>
              <Row xstyle={styles.headerControl}>{headerControls}</Row>
            </Row>
            <Column alignStart style={{marginBlock: '8px'}}>
              {path && codeSuggestion != null && codeChange != null && (
                <>
                  {codeSuggestion.type !== SuggestedChangeType.HUMAN_SUGGESTION && (
                    <Row xstyle={styles.subheadingsAlignBaseline}>
                      <div {...stylex.props(styles.boldText)}>
                        {codeSuggestion.type === SuggestedChangeType.METAMATE_SUGGESTION
                          ? 'Metamate'
                          : 'Signal'}
                      </div>
                      <div {...stylex.props(styles.subtle)}>suggested changes</div>
                    </Row>
                  )}
                  <Suspense>{renderDiffView(codeChange)}</Suspense>
                </>
              )}
              <Row xstyle={styles.tooltipBar}>
                {codeSuggestion?.status != null ? (
                  <InlineCommentSuggestionActionBottomBar
                    resolved={codeSuggestion.status === CodePatchSuggestionStatus.Accepted}
                    onAccept={() => onAccept(codeSuggestion)}
                    onReject={() => onReject(codeSuggestion)}
                  />
                ) : (
                  <InlineCommentSuggestionActionBottomBar
                    resolved={comment.isResolved ?? false}
                    onAccept={onResolve}
                    onReject={onUnresolve}
                    acceptLabel="Resolve"
                    rejectLabel="Unresolve"
                    isToggle={true}
                  />
                )}
                <Row>{bottomControls}</Row>
              </Row>
            </Column>
          </>
        )}
      </div>
    );
  },
);

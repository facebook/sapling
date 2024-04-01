/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {LabelFragment} from './generated/graphql';

import FieldLabel from './FieldLabel';
import YokedRepoLabelsInput from './YokedRepoLabelsInput';
import {
  gitHubClient,
  gitHubPullRequest,
  gitHubPullRequestLabels,
  gitHubPullRequestViewerDidAuthor,
} from './recoil';
import {GearIcon, TagIcon} from '@primer/octicons-react';
import {ActionMenu, Box, Button, IssueLabelToken, StyledOcticon} from '@primer/react';
import {useEffect, useMemo} from 'react';
import {useRecoilCallback, useRecoilState, useRecoilValue} from 'recoil';
import {notEmpty} from 'shared/utils';

export default function PullRequestLabels(): React.ReactElement {
  const pullRequest = useRecoilValue(gitHubPullRequest);
  const [pullRequestLabels, setPullRequestLabels] = useRecoilState(gitHubPullRequestLabels);
  const viewerDidAuthor = useRecoilValue(gitHubPullRequestViewerDidAuthor);
  const existingLabelIDs = useMemo(
    () => new Set(pullRequestLabels.map(({id}) => id)),
    [pullRequestLabels],
  );

  // Initialize pullRequestLabels state using pullRequest once it is available.
  useEffect(() => {
    if (pullRequest != null) {
      const labels = (pullRequest.labels?.nodes ?? []).filter(notEmpty);
      setPullRequestLabels(labels);
    }
  }, [pullRequest, setPullRequestLabels]);

  const updateLabels = useRecoilCallback<[LabelFragment, boolean], Promise<void>>(
    ({snapshot}) =>
      async ({id, name, color}: LabelFragment, isExisting: boolean) => {
        const client = snapshot.getLoadable(gitHubClient).valueMaybe();
        if (client == null) {
          return Promise.reject('client not found');
        }

        const pullRequestId = snapshot.getLoadable(gitHubPullRequest).valueMaybe()?.id;
        if (pullRequestId == null) {
          return Promise.reject('pull request not found');
        }

        try {
          // When adding or removing a label, optimistically update
          // pullRequestLabels and the UI instead of waiting for the respective
          // API call to return.
          if (!isExisting) {
            const labels = [...pullRequestLabels, {id, name, color}].sort((a, b) =>
              a.name.localeCompare(b.name),
            );
            setPullRequestLabels(labels);
            await client.addLabels({
              labelableId: pullRequestId,
              labelIds: [id],
            });
          } else {
            const labels = pullRequestLabels.filter(label => label.id !== id);
            setPullRequestLabels(labels);
            await client.removeLabels({
              labelableId: pullRequestId,
              labelIds: [id],
            });
          }
        } catch {
          // If there is an error, roll back the update by resetting
          // pullRequestLabels to its previous value.
          setPullRequestLabels(pullRequestLabels);
        }
      },
    [pullRequestLabels, setPullRequestLabels],
  );

  const label = viewerDidAuthor && (
    <ActionMenu>
      <ActionMenu.Anchor>
        <button className="pr-label-button">
          <StyledOcticon icon={TagIcon} />
        </button>
      </ActionMenu.Anchor>
      <ActionMenu.Overlay width="medium">
        <YokedRepoLabelsInput existingLabelIDs={existingLabelIDs} onSelect={updateLabels} />
      </ActionMenu.Overlay>
    </ActionMenu>
  );

  return (
    <Box display="flex" alignItems="center" gridGap={2} paddingLeft={3}>
      {label}
      <Box display="flex" gridGap={1}>
        {pullRequestLabels.map(({id, name, color}) => (
          <IssueLabelToken
            style={{
              color: '#57606a',
              background: 'none',
              borderColor: 'rgba(27,31,36,0.15)',
            }}
            key={id}
            text={name}
            fillColor={`rgba(234,238,242,0.5)`}
            size="large"
            onRemove={!viewerDidAuthor ? undefined : () => updateLabels({id, name, color}, true)}
            hideRemoveButton={!viewerDidAuthor}
          />
        ))}
      </Box>
    </Box>
  );

  return (
    <>
      {pullRequestLabels.map(({id, name, color}) => (
        <IssueLabelToken
          style={{
            color: '#57606a',
            backgroundColor: 'rgba(234,238,242,0.5)',
            borderColor: 'rgba(27,31,36,0.15)',
          }}
          key={id}
          text={name}
          fillColor={`rgba(234,238,242,0.5)`}
          size="large"
          onRemove={!viewerDidAuthor ? undefined : () => updateLabels({id, name, color}, true)}
          hideRemoveButton={!viewerDidAuthor}
        />
      ))}
    </>
  );
}

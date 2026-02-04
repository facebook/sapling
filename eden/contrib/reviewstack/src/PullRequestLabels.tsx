/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {LabelFragment} from './generated/graphql';

import FieldLabel from './FieldLabel';
import RepoLabelsInput from './RepoLabelsInput';
import {
  gitHubClientAtom,
  gitHubPullRequestAtom,
  gitHubPullRequestLabelsAtom,
  gitHubPullRequestViewerDidAuthorAtom,
} from './jotai';
import {GearIcon} from '@primer/octicons-react';
import {ActionMenu, Box, Button, IssueLabelToken} from '@primer/react';
import {useAtom, useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useCallback, useEffect, useMemo} from 'react';
import {notEmpty} from 'shared/utils';

export default function PullRequestLabels(): React.ReactElement {
  const pullRequest = useAtomValue(gitHubPullRequestAtom);
  const [pullRequestLabels, setPullRequestLabels] = useAtom(gitHubPullRequestLabelsAtom);
  const viewerDidAuthor = useAtomValue(gitHubPullRequestViewerDidAuthorAtom);
  const existingLabelIDs = useMemo(
    () => new Set(pullRequestLabels.map(({id}) => id)),
    [pullRequestLabels],
  );

  // Load the GitHub client asynchronously
  const loadableClient = useMemo(() => loadable(gitHubClientAtom), []);
  const clientLoadable = useAtomValue(loadableClient);
  const client = clientLoadable.state === 'hasData' ? clientLoadable.data : null;

  // Initialize pullRequestLabels state using pullRequest once it is available.
  useEffect(() => {
    if (pullRequest != null) {
      const labels = (pullRequest.labels?.nodes ?? []).filter(notEmpty);
      setPullRequestLabels(labels);
    }
  }, [pullRequest, setPullRequestLabels]);

  const updateLabels = useCallback(
    async ({id, name, color}: LabelFragment, isExisting: boolean) => {
      if (client == null) {
        return Promise.reject('client not found');
      }

      const pullRequestId = pullRequest?.id;
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
    [client, pullRequest, pullRequestLabels, setPullRequestLabels],
  );

  const label = !viewerDidAuthor ? (
    <FieldLabel label="Labels" />
  ) : (
    <ActionMenu>
      <ActionMenu.Anchor>
        <Button trailingIcon={GearIcon}>Labels</Button>
      </ActionMenu.Anchor>
      <ActionMenu.Overlay width="medium">
        <RepoLabelsInput existingLabelIDs={existingLabelIDs} onSelect={updateLabels} />
      </ActionMenu.Overlay>
    </ActionMenu>
  );

  return (
    <Box display="flex" alignItems="center" gridGap={2}>
      {label}
      <Box display="flex" flexWrap="wrap" gridGap={1}>
        {pullRequestLabels.map(({id, name, color}) => (
          <IssueLabelToken
            key={id}
            text={name}
            fillColor={`#${color}`}
            onRemove={!viewerDidAuthor ? undefined : () => updateLabels({id, name, color}, true)}
          />
        ))}
      </Box>
    </Box>
  );
}

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import TrustedRenderedMarkdown from './TrustedRenderedMarkdown';
import {gitHubCurrentCommitAtom} from './jotai';
import {Box, Link, Text} from '@primer/react';
import {useAtomValue} from 'jotai';

export default function CommitHeader(): React.ReactElement | null {
  const commit = useAtomValue(gitHubCurrentCommitAtom);

  if (commit == null) {
    return null;
  }

  const {messageHeadlineHTML, url} = commit;

  return (
    <Box
      borderBottomWidth={1}
      borderBottomStyle="solid"
      borderBottomColor="border.default"
      fontWeight="bold"
      padding={3}>
      <TrustedRenderedMarkdown trustedHTML={messageHeadlineHTML} inline={true} />{' '}
      <Link href={url} target="_blank">
        <Text fontWeight="normal">(view on GitHub)</Text>
      </Link>
    </Box>
  );
}

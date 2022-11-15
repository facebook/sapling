/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {AlertIcon} from '@primer/octicons-react';
import {Text, Flash, StyledOcticon, Box} from '@primer/react';
import {Component} from 'react';

function ErrorNotice({title, error}: {title: React.ReactNode; error: Error}) {
  return (
    <Flash variant="warning" sx={{margin: 20}}>
      <StyledOcticon icon={AlertIcon} />
      <Text fontWeight="bold">{title}</Text>
      <Box as="p">
        <Text fontFamily={'mono'}>{error.stack ?? error.toString()}</Text>
      </Box>
    </Flash>
  );
}

type Props = {
  children: React.ReactNode;
};
type State = {error: Error | null};
export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = {error: null};
  }

  static getDerivedStateFromError(error: Error) {
    return {error};
  }

  render() {
    if (this.state.error != null) {
      return <ErrorNotice title="Something went wrong" error={this.state.error} />;
    }

    return this.props.children;
  }
}

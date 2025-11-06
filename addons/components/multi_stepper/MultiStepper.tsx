/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MultiStepperContext} from './MultiStepperContext';

import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  container: {
    overflowY: 'hidden',
    height: '100%',
  },
  contentLayout: {
    display: 'flex',
    flexDirection: 'row',
    gap: '24px',
    height: '100%',
  },
  leftContentContainer: {
    flexShrink: 0,
    minWidth: '200px',
  },
  stepContent: {
    flex: 1,
    overflowY: 'auto',
  },
});

type Props<TKey> = {
  /**
   * Stepper state to control the current step and navigation.
   */
  stepper: MultiStepperContext<TKey>;

  /**
   * Optional left content to display on the left side of the stepper.
   */
  leftContent?: React.ReactNode;

  /**
   * The content to display in each step.
   * Each child corresponds to a step in the stepper.
   */
  children: Array<React.ReactNode>;
};

/**
 * Component to render a set of steps, providing a way to navigate between
 * the steps and display the content for each step.
 *
 * Within the content of each step, you can use the `useMultiStepperContext` hook
 * to access the current step and navigate to other steps.
 */
export function MultiStepper<TKey>({leftContent, stepper, children}: Props<TKey>) {
  return (
    <div {...stylex.props(styles.container)}>
      <div {...stylex.props(styles.contentLayout)}>
        {leftContent && <div {...stylex.props(styles.leftContentContainer)}>{leftContent}</div>}
        <div {...stylex.props(styles.stepContent)}>{children[stepper.getStepIndex()]}</div>
      </div>
    </div>
  );
}

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MultiStepperContext} from 'isl-components/multi_stepper/MultiStepperContext';

import * as stylex from '@stylexjs/stylex';
import {Icon} from 'isl-components/Icon';

const styles = stylex.create({
  stepItem: {
    display: 'flex',
    alignItems: 'center',
    gap: '12px',
    padding: '12px',
    borderRadius: '4px',
    marginBottom: '8px',
  },
  stepItemActive: {
    backgroundColor: 'var(--highlight-background)',
  },
  stepItemCompleted: {
    cursor: 'pointer',
  },
  stepNumber: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '20px',
    height: '20px',
    borderRadius: '50%',
    border: '2px solid var(--foreground)',
    fontSize: '12px',
    fontWeight: 'bold',
  },
  stepNumberActive: {
    backgroundColor: 'var(--button-primary-background)',
    borderColor: 'var(--button-primary-background)',
    color: 'var(--button-primary-foreground)',
  },
  stepNumberCompleted: {
    backgroundColor: 'var(--success-background)',
    borderColor: 'var(--success-background)',
  },
  stepLabel: {
    fontSize: '14px',
  },
  stepLabelActive: {
    fontWeight: 'bold',
  },
});

type Props<TKey> = {
  stepper: MultiStepperContext<TKey>;
};

/**
 * A vertical progress bar for a multi-step stepper.
 */
export function VerticalStepperProgress<TKey>({stepper}: Props<TKey>) {
  const icon = <Icon icon="check" />;

  const steps = stepper.getAllSteps();
  const currentIndex = stepper.getStepIndex();

  return (
    <div>
      {steps.map((step, index) => {
        const isActive = index === currentIndex;
        const isCompleted = index < currentIndex;

        return (
          <div
            key={String(step.key)}
            onClick={() => (isCompleted ? stepper.goToStepByKey(step.key) : undefined)}
            {...stylex.props(
              styles.stepItem,
              isActive && styles.stepItemActive,
              isCompleted && styles.stepItemCompleted,
            )}>
            <div
              {...stylex.props(
                styles.stepNumber,
                isActive && styles.stepNumberActive,
                isCompleted && styles.stepNumberCompleted,
              )}>
              {isCompleted ? icon : index + 1}
            </div>
            <div {...stylex.props(styles.stepLabel, isActive && styles.stepLabelActive)}>
              {step.label}
            </div>
          </div>
        );
      })}
    </div>
  );
}

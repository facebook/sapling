/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {useMemo, useState} from 'react';

export type StepConfig<TKey> = Readonly<{
  /**
   * Key to identify this step.
   */
  key: TKey;

  /**
   * Label to display (in the stepper, header, etc.)
   */
  label: ReactNode;
}>;

export type MultiStepperContext<TKey> = Readonly<{
  /* Getters */

  /**
   * Gets the current step config.
   */
  getCurrentStep: () => StepConfig<TKey>;

  /**
   * Gets the step config for the given key.
   */
  getStep: (step: TKey) => StepConfig<TKey> | undefined;

  /**
   * Gets the index of the current step.
   */
  getStepIndex: () => number;

  /**
   * Gets the total number of steps.
   */
  getStepCount: () => number;

  /**
   * Get all step configs in order.
   */
  getAllSteps: () => Array<StepConfig<TKey>>;

  /* Navigation Methods */

  /**
   * Go to the step at the given index.
   */
  goToStep: (index: number) => void;

  /**
   * Go to the step with the given key.
   */
  goToStepByKey: (key: TKey) => void;

  /**
   * Go to the next step.
   */
  goToNextStep: () => void;

  /**
   * Go to the previous step.
   */
  goToPreviousStep: () => void;

  /**
   * Go to the first step.
   */
  goToFirstStep: () => void;

  /**
   * Go to the last step.
   */
  goToLastStep: () => void;
}>;

/**
 * Hook to access the current step and navigate to other steps.
 */
export function useMultiStepperContext<TKey>(
  pages: Array<StepConfig<TKey>>,
): MultiStepperContext<TKey> {
  const [currentStep, setCurrentStep] = useState<number>(0);

  const stepByKey = useMemo(
    () => new Map<TKey, number>(pages.map((page, index) => [page.key, index])),
    [pages],
  );

  const value = useMemo(
    () => ({
      _steps: pages,
      _currentStep: currentStep,
      _setCurrentStep: setCurrentStep,

      getCurrentStep: () => pages[currentStep],

      getStep: (step: TKey) => pages.at(stepByKey.get(step) ?? -1),

      getStepIndex: () => currentStep,

      getStepCount: () => pages.length,

      getAllSteps: () => pages,

      goToStep: (index: number) => setCurrentStep(index),

      goToStepByKey: (key: TKey) => {
        const index = stepByKey.get(key);
        if (index != null) {
          setCurrentStep(index);
        }
      },

      goToNextStep: () => {
        if (currentStep < pages.length - 1) {
          setCurrentStep(currentStep + 1);
        }
      },

      goToPreviousStep: () => {
        if (currentStep > 0) {
          setCurrentStep(currentStep - 1);
        }
      },

      goToFirstStep: () => setCurrentStep(0),

      goToLastStep: () => setCurrentStep(pages.length - 1),
    }),
    [currentStep, pages, stepByKey],
  );

  return value;
}

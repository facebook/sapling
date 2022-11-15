/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {I18nSupport, t, T} from '../i18n';
import {render, screen} from '@testing-library/react';
import '../i18n/en/common.json';

jest.mock(
  '../i18n/en/common.json',
  () => ({
    translate_me: 'this was translated',
    plural_one: 'There is one apple',
    plural_other: 'There are {count} apples',
    replace_and_plural_one: 'There is one $type apple',
    replace_and_plural_other: 'There are {count} $type apples',
  }),
  {virtual: true},
);

describe('i18n', () => {
  describe('<T>', () => {
    it('can render translations', () => {
      render(
        <I18nSupport>
          <T>translate_me</T>
        </I18nSupport>,
      );

      expect(screen.getByText('this was translated')).toBeInTheDocument();
      expect(screen.queryByText('translate_me')).not.toBeInTheDocument();
    });

    it('can render plurals: singular', () => {
      render(
        <I18nSupport>
          <T count={1}>plural</T>
        </I18nSupport>,
      );

      expect(screen.getByText('There is one apple')).toBeInTheDocument();
      expect(screen.queryByText('plural')).not.toBeInTheDocument();
    });

    it('can render plurals: plural', () => {
      render(
        <I18nSupport>
          <T count={2}>plural</T>
        </I18nSupport>,
      );

      expect(screen.getByText('There are 2 apples')).toBeInTheDocument();
      expect(screen.queryByText('plural')).not.toBeInTheDocument();
    });

    it('can replace with strings', () => {
      render(
        <I18nSupport>
          <T replace={{apples: 'oranges'}}>compare apples and bananas</T>
        </I18nSupport>,
      );

      expect(screen.getByText('oranges and bananas', {exact: false})).toBeInTheDocument();
      expect(screen.queryByText('apples', {exact: false})).not.toBeInTheDocument();
    });

    it('can replace with components', () => {
      render(
        <I18nSupport>
          <T replace={{apples: <b data-testid="orange">oranges</b>}}>compare apples and bananas</T>
        </I18nSupport>,
      );

      expect(screen.getByTestId('orange')).toBeInTheDocument();
      expect(screen.queryByText('apples', {exact: false})).not.toBeInTheDocument();
    });

    it('can replace multiple times', () => {
      render(
        <I18nSupport>
          <T replace={{apples: <b data-testid="orange">oranges</b>, bananas: 'grapefruit'}}>
            compare apples and bananas
          </T>
        </I18nSupport>,
      );

      expect(screen.getByTestId('orange')).toBeInTheDocument();
      expect(screen.getByText('and grapefruit', {exact: false})).toBeInTheDocument();
      expect(screen.queryByText('apples', {exact: false})).not.toBeInTheDocument();
      expect(screen.queryByText('bananas', {exact: false})).not.toBeInTheDocument();
    });

    it('can replace with plurals: singular', () => {
      render(
        <I18nSupport>
          <T replace={{$type: 'cool'}} count={1}>
            replace_and_plural
          </T>
        </I18nSupport>,
      );

      expect(screen.queryByText('There is one cool apple')).toBeInTheDocument();
    });

    it('can replace with plurals: plural', () => {
      render(
        <I18nSupport>
          <T replace={{$type: 'cool'}} count={2}>
            replace_and_plural
          </T>
        </I18nSupport>,
      );

      expect(screen.queryByText('There are 2 cool apples')).toBeInTheDocument();
    });
  });

  describe('t()', () => {
    it('can render translations', () => {
      expect(t('translate_me')).toEqual('this was translated');
    });

    it('can render plurals: singular', () => {
      expect(t('plural', {count: 1})).toEqual('There is one apple');
    });

    it('can render plurals: plural', () => {
      expect(t('plural', {count: 2})).toEqual('There are 2 apples');
    });

    it('can replace with strings', () => {
      expect(t('compare apples and bananas', {replace: {apples: 'oranges'}})).toEqual(
        'compare oranges and bananas',
      );
    });

    it('can replace multiple times', () => {
      expect(
        t('compare apples and bananas', {replace: {apples: 'oranges', bananas: 'grapefruit'}}),
      ).toEqual('compare oranges and grapefruit');
    });

    it('can replace with plurals: singular', () => {
      expect(t('replace_and_plural', {replace: {$type: 'cool'}, count: 1})).toEqual(
        'There is one cool apple',
      );
    });

    it('can replace with plurals: plural', () => {
      expect(t('replace_and_plural', {replace: {$type: 'cool'}, count: 2})).toEqual(
        'There are 2 cool apples',
      );
    });
  });
});

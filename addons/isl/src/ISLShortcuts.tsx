/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {
  collapseModifiersToNumber,
  KeyCode,
  makeCommandDispatcher,
  Modifier,
} from 'isl-components/KeyboardShortcuts';
import {isMac} from 'isl-components/OperatingSystem';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtom, useAtomValue} from 'jotai';
import {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import {TypedEventEmitter} from 'shared/TypedEventEmitter';
import {getTracker} from './analytics/globalTracker';
import {featureFlagLoadable} from './featureFlags';
import {t, T} from './i18n';
import {Internal} from './Internal';
import {configBackedAtom} from './jotaiUtils';
import {useModal} from './useModal';

import './ISLShortcuts.css';

const CMD = isMac ? Modifier.CMD : Modifier.CTRL;

/* eslint-disable no-bitwise */
export const [ISLCommandContext, useCommand, dispatchCommand, allCommands, setCommands] =
  makeCommandDispatcher(
    {
      OpenShortcutHelp: [Modifier.SHIFT, KeyCode.QuestionMark],
      ToggleSidebar: [CMD, KeyCode.Period],
      OpenUncommittedChangesComparisonView: [CMD, KeyCode.SingleQuote],
      OpenHeadChangesComparisonView: [[CMD, Modifier.SHIFT], KeyCode.SingleQuote],
      Escape: [Modifier.NONE, KeyCode.Escape],
      SelectUpwards: [Modifier.NONE, KeyCode.UpArrow],
      SelectDownwards: [Modifier.NONE, KeyCode.DownArrow],
      OpenDetails: [Modifier.NONE, KeyCode.RightArrow],
      ContinueSelectionUpwards: [Modifier.SHIFT, KeyCode.UpArrow],
      ContinueSelectionDownwards: [Modifier.SHIFT, KeyCode.DownArrow],
      SelectAllCommits: [Modifier.ALT, KeyCode.A],
      HideSelectedCommits: [Modifier.NONE, KeyCode.Backspace],
      ZoomIn: [Modifier.ALT, KeyCode.Plus],
      ZoomOut: [Modifier.ALT, KeyCode.Minus],
      ToggleTheme: [Modifier.ALT, KeyCode.T],
      ToggleShelvedChangesDropdown: [Modifier.ALT, KeyCode.S],
      ToggleDownloadCommitsDropdown: [Modifier.ALT, KeyCode.D],
      ToggleCwdDropdown: [Modifier.ALT, KeyCode.C],
      ToggleBulkActionsDropdown: [Modifier.ALT, KeyCode.B],
      ToggleFocusMode: [Modifier.ALT, KeyCode.F],
      ToggleBookmarksManagerDropdown: [Modifier.ALT, KeyCode.M],
      RebaseOntoCurrentStackBase: [Modifier.ALT, KeyCode.R],
      ToggleFilterDropdown: [Modifier.CMD, KeyCode.F],
      Pull: [Modifier.ALT, KeyCode.P],
      ArcPull: [[Modifier.ALT, Modifier.SHIFT], KeyCode.P],
    },
    ({command, key, modifiers}) => {
      getTracker()?.track('KeyboardShortcutUsed', {extras: {command, key, modifiers}});
    },
  );

export type ISLCommandName = Parameters<typeof useCommand>[0];

/** Like useCommand, but returns an eventEmitter you can subscribe to */
export function useCommandEvent(commandName: ISLCommandName): TypedEventEmitter<'change', null> {
  const emitter = useMemo(() => new TypedEventEmitter<'change', null>(), []);
  useCommand(commandName, () => {
    emitter.emit('change', null);
  });
  return emitter;
}

/** A persisted [modifiers, keyCode] binding override (mirrors a CommandDefinition). */
export type KeyBinding = [Array<Modifier>, KeyCode];

/** Commands whose key binding the user is allowed to override from Settings. */
export const OVERRIDABLE_COMMANDS: ReadonlyArray<ISLCommandName> = ['Pull', 'ArcPull'];

/** Default (built-in) bindings, before user overrides are applied. */
const defaultCommands = allCommands;

/**
 * User overrides for keyboard shortcuts, persisted in the user's sl config as JSON
 * (`isl.keyboard-shortcut-overrides`). Only {@link OVERRIDABLE_COMMANDS} are honored.
 */
export const keyboardShortcutOverrides = configBackedAtom<
  Partial<Record<ISLCommandName, KeyBinding>>
>('isl.keyboard-shortcut-overrides', {});

/** Drop any override for a command we don't allow to be overridden (defensive against hand-edited config). */
function sanitizeOverrides(
  overrides: Partial<Record<ISLCommandName, KeyBinding>>,
): Partial<Record<ISLCommandName, KeyBinding>> {
  const result: Partial<Record<ISLCommandName, KeyBinding>> = {};
  for (const command of OVERRIDABLE_COMMANDS) {
    const binding = overrides[command];
    if (binding != null) {
      result[command] = binding;
    }
  }
  return result;
}

/**
 * Whether the customizable Pull/Arc Pull keyboard-shortcut feature is enabled.
 * Gated by a Gatekeeper killswitch: the feature is ON by default — including in OSS (no
 * Gatekeeper available) and while the flag is still loading — and flipping
 * `isl_keyboard_shortcuts_killswitch` ON disables it. Mirrors the LandModalKillswitch pattern.
 */
export const keyboardShortcutsEnabledAtom = atom(get => {
  const killswitch = Internal.featureFlags?.KeyboardShortcutsKillswitch;
  if (killswitch == null) {
    return true;
  }
  const flag = get(featureFlagLoadable(killswitch));
  const killed = flag.state === 'hasData' ? flag.data : false;
  return !killed;
});

/** Hook form of {@link keyboardShortcutsEnabledAtom}. */
export function useKeyboardShortcutsEnabled(): boolean {
  return useAtomValue(keyboardShortcutsEnabledAtom);
}

/**
 * Effective bindings = built-in defaults with user overrides applied on top.
 * When the feature is killswitched off, overrides are ignored (defaults only).
 */
export const effectiveCommandsAtom = atom<typeof allCommands>(get =>
  get(keyboardShortcutsEnabledAtom)
    ? {...defaultCommands, ...sanitizeOverrides(get(keyboardShortcutOverrides))}
    : defaultCommands,
);

/**
 * Apply persisted keyboard-shortcut overrides to the live dispatcher.
 * Mount once near the top of the app (inside {@link ISLCommandContext}). Overrides load
 * asynchronously from the sl config, so the dispatcher starts on defaults and switches once
 * the config arrives.
 */
export function useApplyKeyboardShortcutOverrides(): void {
  const effective = useAtomValue(effectiveCommandsAtom);
  useEffect(() => {
    setCommands(effective);
  }, [effective]);
}

export const ISLShortcutLabels: Partial<Record<ISLCommandName, string>> = {
  Escape: t('Dismiss Tooltips and Popups'),
  OpenShortcutHelp: t('Open Shortcut Help'),
  ToggleSidebar: t('Toggle Commit Info Sidebar'),
  OpenUncommittedChangesComparisonView: t('Open Uncommitted Changes Comparison View'),
  OpenHeadChangesComparisonView: t('Open Head Changes Comparison View'),
  SelectAllCommits: t('Select All Commits'),
  ToggleTheme: t('Toggle Light/Dark Theme'),
  ZoomIn: t('Zoom In'),
  ZoomOut: t('Zoom Out'),
  ToggleShelvedChangesDropdown: t('Toggle Shelved Changes Dropdown'),
  ToggleDownloadCommitsDropdown: t('Toggle Download Commits Dropdown'),
  ToggleCwdDropdown: t('Toggle CWD Dropdown'),
  ToggleBulkActionsDropdown: t('Toggle Bulk Actions Dropdown'),
  ToggleFocusMode: t('Toggle Focus Mode'),
  ToggleBookmarksManagerDropdown: t('Toggle Bookmarks Manager Dropdown'),
  RebaseOntoCurrentStackBase: t('Rebase Selected Commits onto Current Stack Base'),
  ToggleFilterDropdown: t('Filter Commits'),
  Pull: t('Pull'),
  ArcPull: t('Arc Pull'),
};

/** keyCodes for the modifier keys themselves, which can't be a shortcut on their own. */
const PURE_MODIFIER_KEYCODES = new Set<number>([16, 17, 18, 91, 93, 224]);

/**
 * keyCodes reserved for navigation / editing / window actions that must never be bound as a
 * shortcut — e.g. binding Tab would swallow Tab and break keyboard navigation in the dialog.
 * Backspace (8), Tab (9), Enter (13), PageUp/Down (33/34), End/Home (35/36), arrows (37-40),
 * and F1-F12 (112-123).
 */
const RESERVED_KEYCODES = new Set<number>([
  8, 9, 13, 33, 34, 35, 36, 37, 38, 39, 40, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122,
  123,
]);

/**
 * Whether `commandName`'s effective binding collides with another command's, and if so which.
 * Returns the conflicting command name, or undefined when there's no conflict.
 */
function findConflict(
  effective: typeof allCommands,
  commandName: ISLCommandName,
): ISLCommandName | undefined {
  const [mods, key] = effective[commandName];
  const collapsed = collapseModifiersToNumber(mods);
  for (const [other, [otherMods, otherKey]] of Object.entries(effective) as Array<
    [ISLCommandName, [Modifier | Array<Modifier>, KeyCode]]
  >) {
    if (other === commandName) {
      continue;
    }
    // ArcPull isn't shown/active in OSS builds, so don't flag it as a conflict there.
    if (other === 'ArcPull' && Internal.additionalPullOptions == null) {
      continue;
    }
    if (otherKey === key && collapseModifiersToNumber(otherMods) === collapsed) {
      return other;
    }
  }
  return undefined;
}

/** A focusable control that records the next key combination the user presses. */
function ShortcutCapture({
  onCapture,
  onCancel,
}: {
  onCapture: (binding: KeyBinding) => void;
  onCancel: () => void;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    ref.current?.focus();
  }, []);
  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    // Prevent the captured keys from triggering the global shortcut dispatcher or the modal.
    event.preventDefault();
    event.stopPropagation();
    const keyCode = event.keyCode;
    if (keyCode === KeyCode.Escape) {
      onCancel();
      return;
    }
    if (PURE_MODIFIER_KEYCODES.has(keyCode) || RESERVED_KEYCODES.has(keyCode)) {
      // Wait for a real, non-reserved key. Reserved keys (Tab, Enter, arrows, F-keys, …) are
      // ignored rather than bound, so they can't hijack navigation/editing. Press Escape to cancel.
      return;
    }
    const modifiers: Array<Modifier> = [];
    if (event.shiftKey) {
      modifiers.push(Modifier.SHIFT);
    }
    if (event.ctrlKey) {
      modifiers.push(Modifier.CTRL);
    }
    if (event.altKey) {
      modifiers.push(Modifier.ALT);
    }
    if (event.metaKey) {
      modifiers.push(Modifier.CMD);
    }
    onCapture([modifiers.length > 0 ? modifiers : [Modifier.NONE], keyCode]);
  };
  return (
    <Button
      ref={ref}
      icon
      className="shortcut-capture"
      onKeyDown={handleKeyDown}
      onBlur={onCancel}
      data-testid="shortcut-capture">
      <T>Press keys…</T>
    </Button>
  );
}

function KeyboardShortcutsList() {
  const [overrides, setOverrides] = useAtom(keyboardShortcutOverrides);
  const effective = useAtomValue(effectiveCommandsAtom);
  const enabled = useKeyboardShortcutsEnabled();
  const [capturing, setCapturing] = useState<ISLCommandName | null>(null);

  const rows = (Object.entries(ISLShortcutLabels) as Array<[ISLCommandName, string]>).filter(
    ([name]) =>
      // Arc Pull is internal-only; only surface it in the help dialog for internal builds.
      (name !== 'ArcPull' || Internal.additionalPullOptions != null) &&
      // Hide the Pull/Arc Pull rows entirely when the feature is killswitched off.
      (!OVERRIDABLE_COMMANDS.includes(name) || enabled),
  );
  const hasOverrides = OVERRIDABLE_COMMANDS.some(command => overrides[command] != null);

  return (
    <div className="keyboard-shortcuts-menu">
      <table>
        <tbody>
          {rows.map(([name, label]) => {
            const [modifiers, keyCode] = effective[name];
            const overridable = OVERRIDABLE_COMMANDS.includes(name);
            const isOverridden = overrides[name] != null;
            const conflict = overridable ? findConflict(effective, name) : undefined;
            return (
              <tr key={name}>
                <td>{label}</td>
                <td className="keyboard-shortcut-binding">
                  {capturing === name ? (
                    <ShortcutCapture
                      onCapture={binding => {
                        setOverrides({...overrides, [name]: binding});
                        setCapturing(null);
                      }}
                      onCancel={() => setCapturing(null)}
                    />
                  ) : (
                    <Kbd
                      modifiers={Array.isArray(modifiers) ? modifiers : [modifiers]}
                      keycode={keyCode}
                    />
                  )}
                  {conflict != null && capturing !== name && (
                    <span className="keyboard-shortcut-conflict">
                      <Icon icon="warning" color="yellow" />
                      <T replace={{$command: ISLShortcutLabels[conflict] ?? conflict}}>
                        Conflicts with $command
                      </T>
                    </span>
                  )}
                </td>
                <td className="keyboard-shortcut-actions">
                  {overridable && (
                    <>
                      <Tooltip title={t('Rebind this shortcut')}>
                        <Button
                          icon
                          onClick={() => setCapturing(name)}
                          data-testid={`edit-shortcut-${name}`}>
                          <Icon icon="edit" />
                        </Button>
                      </Tooltip>
                      {isOverridden && (
                        <Tooltip title={t('Reset to default')}>
                          <Button
                            icon
                            onClick={() => {
                              const next = {...overrides};
                              delete next[name];
                              setOverrides(next);
                            }}
                            data-testid={`reset-shortcut-${name}`}>
                            <Icon icon="discard" />
                          </Button>
                        </Tooltip>
                      )}
                    </>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {hasOverrides && (
        <Button onClick={() => setOverrides({})} data-testid="reset-all-shortcuts">
          <Icon icon="discard" slot="start" />
          <T>Reset all to defaults</T>
        </Button>
      )}
    </div>
  );
}

export function useShowKeyboardShortcutsHelp(): () => unknown {
  const showModal = useModal();
  const showShortcutsModal = () => {
    showModal({
      type: 'custom',
      component: () => <KeyboardShortcutsList />,
      icon: 'keyboard',
      title: t('Keyboard Shortcuts'),
    });
  };
  useCommand('OpenShortcutHelp', showShortcutsModal);
  return showShortcutsModal;
}

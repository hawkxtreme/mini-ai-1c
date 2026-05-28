import test from 'node:test';
import assert from 'node:assert/strict';
import {
  buildPromptFromSlashCommandTemplate,
  getQuickActionBindings,
  getQuickActionCommandId,
  getQuickActionMenuLabel,
  isQuickActionSlashCommand,
  resolveSlashCommandsForRuntime,
} from '../slashCommands';
import { DEFAULT_SLASH_COMMANDS, type SlashCommand } from '../../types/settings';

test('resolveSlashCommandsForRuntime preserves edited system command templates', () => {
  const saved: SlashCommand[] = DEFAULT_SLASH_COMMANDS.map((cmd) =>
    cmd.id === 'review'
      ? { ...cmd, template: 'CUSTOM REVIEW {code}' }
      : cmd,
  );

  const resolved = resolveSlashCommandsForRuntime(saved, DEFAULT_SLASH_COMMANDS);
  const review = resolved.find((cmd) => cmd.id === 'review');

  assert.equal(review?.template, 'CUSTOM REVIEW {code}');
});

test('resolveSlashCommandsForRuntime appends missing default system commands', () => {
  const saved = DEFAULT_SLASH_COMMANDS.filter((cmd) => cmd.id !== 'elaborate');

  const resolved = resolveSlashCommandsForRuntime(saved, DEFAULT_SLASH_COMMANDS);

  assert.ok(resolved.some((cmd) => cmd.id === 'elaborate'));
});

test('buildPromptFromSlashCommandTemplate replaces quick action placeholders everywhere', () => {
  const prompt = buildPromptFromSlashCommandTemplate(
    'Q={query}\nD={diagnostics}\nC1={code}\nC2={code}',
    {
      code: 'Procedure Test()',
      query: 'add guard',
      diagnostics: 'line 1: missing semicolon',
    },
  );

  assert.equal(
    prompt,
    'Q=add guard\nD=line 1: missing semicolon\nC1=Procedure Test()\nC2=Procedure Test()',
  );
});

test('quick action bindings cover all configurator context menu commands', () => {
  assert.deepEqual(getQuickActionBindings(), [
    { action: 'describe', commandId: 'desc', menuLabel: 'Описание' },
    { action: 'elaborate', commandId: 'elaborate', menuLabel: 'Доработать...' },
    { action: 'fix', commandId: 'fix', menuLabel: 'Исправить' },
    { action: 'explain', commandId: 'explain', menuLabel: 'Объяснить' },
    { action: 'review', commandId: 'review', menuLabel: 'Ревью кода' },
  ]);
});

test('quick action bindings point to editable system slash commands', () => {
  const defaultsById = new Map(DEFAULT_SLASH_COMMANDS.map((command) => [command.id, command]));

  for (const binding of getQuickActionBindings()) {
    const command = defaultsById.get(binding.commandId);
    assert.ok(command, `missing default command for ${binding.action}`);
    assert.equal(command?.is_system, true);
    assert.equal(isQuickActionSlashCommand(command!.id), true);
    assert.equal(getQuickActionCommandId(binding.action), command!.id);
    assert.equal(getQuickActionMenuLabel(command!.id), binding.menuLabel);
  }
});

test('elaborate quick action is separate from refactor command', () => {
  assert.equal(getQuickActionCommandId('elaborate'), 'elaborate');
  assert.equal(getQuickActionCommandId('elaborate') === 'refactor', false);
  assert.equal(isQuickActionSlashCommand('refactor'), false);
});

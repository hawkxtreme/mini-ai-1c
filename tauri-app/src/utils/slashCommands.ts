import type { SlashCommand } from '../types/settings';

export type ConfiguratorQuickAction = 'describe' | 'elaborate' | 'fix' | 'explain' | 'review';

export interface QuickActionSlashCommandBinding {
  action: ConfiguratorQuickAction;
  commandId: string;
  menuLabel: string;
}

const QUICK_ACTION_BINDINGS: readonly QuickActionSlashCommandBinding[] = [
  { action: 'describe', commandId: 'desc', menuLabel: 'Описание' },
  { action: 'elaborate', commandId: 'elaborate', menuLabel: 'Доработать...' },
  { action: 'fix', commandId: 'fix', menuLabel: 'Исправить' },
  { action: 'explain', commandId: 'explain', menuLabel: 'Объяснить' },
  { action: 'review', commandId: 'review', menuLabel: 'Ревью кода' },
] as const;

export interface SlashCommandTemplateValues {
  code?: string | null;
  query?: string | null;
  diagnostics?: string | null;
}

export function resolveSlashCommandsForRuntime(
  savedCommands: SlashCommand[] | null | undefined,
  defaultCommands: SlashCommand[],
): SlashCommand[] {
  const saved = savedCommands?.length ? savedCommands : defaultCommands;
  const defaultsById = new Map(defaultCommands.map((command) => [command.id, command]));
  const seen = new Set<string>();

  const resolved = saved.map((command) => {
    seen.add(command.id);
    const defaults = defaultsById.get(command.id);
    return defaults && command.is_system
      ? { ...defaults, ...command, is_system: true }
      : command;
  });

  for (const command of defaultCommands) {
    if (command.is_system && !seen.has(command.id)) {
      resolved.push(command);
    }
  }

  return resolved;
}

export function buildPromptFromSlashCommandTemplate(
  template: string,
  values: SlashCommandTemplateValues,
): string {
  return template
    .split('{code}').join(values.code ?? '')
    .split('{query}').join(values.query ?? '')
    .split('{diagnostics}').join(values.diagnostics ?? '');
}

export function findSlashCommandById(
  commands: SlashCommand[],
  id: string,
): SlashCommand | undefined {
  return commands.find((command) => command.id === id);
}

export function getQuickActionBindings(): QuickActionSlashCommandBinding[] {
  return QUICK_ACTION_BINDINGS.map((binding) => ({ ...binding }));
}

export function getQuickActionCommandId(
  action: ConfiguratorQuickAction | string,
): string | undefined {
  return QUICK_ACTION_BINDINGS.find((binding) => binding.action === action)?.commandId;
}

export function getQuickActionMenuLabel(commandId: string): string | undefined {
  return QUICK_ACTION_BINDINGS.find((binding) => binding.commandId === commandId)?.menuLabel;
}

export function isQuickActionSlashCommand(commandId: string): boolean {
  return QUICK_ACTION_BINDINGS.some((binding) => binding.commandId === commandId);
}

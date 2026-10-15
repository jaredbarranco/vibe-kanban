import { type ReactNode, useRef } from 'react';
import {
  CheckIcon,
  GitBranchIcon,
  PaperclipIcon,
  RobotIcon,
  XIcon,
} from '@phosphor-icons/react';
import { useTranslation } from 'react-i18next';
import { Checkbox } from './Checkbox';
import { ChatBoxBase, VisualVariant, type DropzoneProps } from './ChatBoxBase';
import { DropdownMenuItem, DropdownMenuLabel } from './Dropdown';
import { PrimaryButton } from './PrimaryButton';
import type { LocalAttachmentMetadata } from './WorkspaceContext';
import { ToolbarDropdown, ToolbarIconButton } from './Toolbar';

export interface EditorProps {
  value: string;
  onChange: (value: string) => void;
}

export interface ModelSelectorProps<TExecutorConfig = unknown> {
  onAdvancedSettings: () => void;
  presets: string[];
  selectedPreset: string | null;
  onPresetSelect: (presetId: string | null) => void;
  onOverrideChange: (partial: Partial<TExecutorConfig>) => void;
  executorConfig: TExecutorConfig | null;
  presetOptions: TExecutorConfig | null | undefined;
}

export interface ExecutorProps<TExecutor extends string = string> {
  selected: TExecutor | null;
  options: TExecutor[];
  onChange: (executor: TExecutor) => void;
}

export interface GitCommitAgentProps {
  selected: string | null;
  options: string[];
  modelId: string | null;
  systemPrompt: string | null;
  onChange: (
    executor: string | null,
    modelId: string | null,
    systemPrompt: string | null
  ) => void;
}

export interface SaveAsDefaultProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  visible: boolean;
}

export interface LinkedIssueBadgeProps {
  simpleId: string;
  title: string;
  onRemove: () => void;
}

export interface CreateChatBoxEditorRenderProps<
  TExecutor extends string = string,
> {
  value: string;
  onChange: (value: string) => void;
  onCmdEnter: () => void;
  disabled: boolean;
  repoIds?: string[];
  repoId?: string;
  executor: TExecutor | null;
  onPasteFiles?: (files: File[]) => void;
  localAttachments?: LocalAttachmentMetadata[];
}

interface CreateChatBoxProps<TExecutor extends string = string> {
  editor: EditorProps;
  renderEditor: (props: CreateChatBoxEditorRenderProps<TExecutor>) => ReactNode;
  agentIcon?: ReactNode;
  onSend: () => void;
  isSending: boolean;
  disabled?: boolean;
  executor: ExecutorProps<TExecutor>;
  formatExecutorLabel?: (executor: TExecutor) => string;
  emptyExecutorLabel?: string;
  saveAsDefault?: SaveAsDefaultProps;
  error?: string | null;
  repoIds?: string[];
  repoId?: string;
  modelSelector?: ReactNode;
  onPasteFiles?: (files: File[]) => void;
  localAttachments?: LocalAttachmentMetadata[];
  dropzone?: DropzoneProps;
  onEditRepos: () => void;
  repoSummaryLabel: string;
  repoSummaryTitle: string;
  linkedIssue?: LinkedIssueBadgeProps | null;
  gitCommitAgent?: GitCommitAgentProps;
}

/**
 * Lightweight chat box for create mode.
 * Supports sending and attachments - no queue, stop, or feedback functionality.
 */
function defaultExecutorLabel(executor: string) {
  return executor
    .replace(/[_-]+/g, ' ')
    .toLowerCase()
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

export function CreateChatBox<TExecutor extends string = string>({
  editor,
  renderEditor,
  onSend,
  isSending,
  disabled = false,
  executor,
  formatExecutorLabel = defaultExecutorLabel,
  emptyExecutorLabel = 'Select Executor',
  saveAsDefault,
  error,
  repoIds,
  repoId,
  modelSelector,
  onPasteFiles,
  localAttachments,
  dropzone,
  onEditRepos,
  repoSummaryLabel,
  repoSummaryTitle,
  linkedIssue,
  gitCommitAgent,
}: CreateChatBoxProps<TExecutor>) {
  const { t } = useTranslation(['common', 'tasks']);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const isDisabled = disabled || isSending;
  const canSend = editor.value.trim().length > 0 && !isDisabled;

  const handleCmdEnter = () => {
    if (canSend) {
      onSend();
    }
  };

  const handleAttachClick = () => {
    fileInputRef.current?.click();
  };

  const handleFileInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files || []);
    if (files.length > 0 && onPasteFiles) {
      onPasteFiles(files);
    }
    e.target.value = '';
  };

  const executorLabel = executor.selected
    ? formatExecutorLabel(executor.selected)
    : emptyExecutorLabel;

  const gitCommitAgentLabel = gitCommitAgent?.selected
    ? formatExecutorLabel(gitCommitAgent.selected as TExecutor)
    : t('tasks:conversation.gitCommitAgent.none');

  return (
    <ChatBoxBase
      editor={renderEditor({
        value: editor.value,
        onChange: editor.onChange,
        onCmdEnter: handleCmdEnter,
        disabled: isDisabled,
        repoIds,
        repoId,
        executor: executor.selected ?? null,
        onPasteFiles,
        localAttachments,
      })}
      error={error}
      visualVariant={VisualVariant.NORMAL}
      dropzone={dropzone}
      modelSelector={modelSelector}
      aboveEditor={
        gitCommitAgent?.selected ? (
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <label className="text-xs text-low font-medium">
                {t('tasks:conversation.gitCommitAgent.modelIdLabel')}
              </label>
              <input
                type="text"
                className="w-full rounded border border-border bg-primary px-2 py-1.5 text-sm text-normal placeholder:text-subtle focus:outline-none focus:ring-1 focus:ring-border-focus disabled:cursor-not-allowed disabled:opacity-50"
                placeholder={t(
                  'tasks:conversation.gitCommitAgent.modelIdPlaceholder'
                )}
                value={gitCommitAgent.modelId ?? ''}
                onChange={(e) =>
                  gitCommitAgent.onChange(
                    gitCommitAgent.selected,
                    e.target.value || null,
                    gitCommitAgent.systemPrompt
                  )
                }
                disabled={isDisabled}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs text-low font-medium">
                {t('tasks:conversation.gitCommitAgent.systemPromptLabel')}
              </label>
              <textarea
                className="w-full rounded border border-border bg-primary px-2 py-1.5 text-sm text-normal placeholder:text-subtle resize-none focus:outline-none focus:ring-1 focus:ring-border-focus"
                rows={3}
                placeholder={t(
                  'tasks:conversation.gitCommitAgent.systemPromptPlaceholder'
                )}
                value={gitCommitAgent.systemPrompt ?? ''}
                onChange={(e) =>
                  gitCommitAgent.onChange(
                    gitCommitAgent.selected,
                    gitCommitAgent.modelId,
                    e.target.value || null
                  )
                }
                disabled={isDisabled}
              />
            </div>
          </div>
        ) : undefined
      }
      headerLeft={
        <>
          <RobotIcon className="size-icon-xl text-low shrink-0" />
          <ToolbarDropdown label={executorLabel} disabled={isDisabled}>
            <DropdownMenuLabel>
              {t('tasks:conversation.executors')}
            </DropdownMenuLabel>
            {executor.options.map((exec) => (
              <DropdownMenuItem
                key={exec}
                icon={executor.selected === exec ? CheckIcon : undefined}
                onClick={() => executor.onChange(exec)}
              >
                {formatExecutorLabel(exec)}
              </DropdownMenuItem>
            ))}
          </ToolbarDropdown>
          {gitCommitAgent && (
            <>
              <GitBranchIcon className="size-icon-xl text-low shrink-0 ml-1" />
              <ToolbarDropdown
                label={gitCommitAgentLabel}
                disabled={isDisabled}
              >
                <DropdownMenuLabel>
                  {t('tasks:conversation.gitCommitAgent.label')}
                </DropdownMenuLabel>
                <DropdownMenuItem
                  icon={
                    gitCommitAgent.selected === null ? CheckIcon : undefined
                  }
                  onClick={() => gitCommitAgent.onChange(null, null, null)}
                >
                  {t('tasks:conversation.gitCommitAgent.none')}
                </DropdownMenuItem>
                {gitCommitAgent.options.map((exec) => (
                  <DropdownMenuItem
                    key={exec}
                    icon={
                      gitCommitAgent.selected === exec ? CheckIcon : undefined
                    }
                    onClick={() =>
                      gitCommitAgent.onChange(
                        exec,
                        gitCommitAgent.modelId,
                        gitCommitAgent.systemPrompt
                      )
                    }
                  >
                    {formatExecutorLabel(exec as TExecutor)}
                  </DropdownMenuItem>
                ))}
              </ToolbarDropdown>
            </>
          )}
          {saveAsDefault?.visible && (
            <label className="flex items-center gap-1.5 text-sm text-low cursor-pointer ml-2">
              <Checkbox
                checked={saveAsDefault.checked}
                onCheckedChange={saveAsDefault.onChange}
                className="h-3.5 w-3.5"
                disabled={isDisabled}
              />
              <span>{t('tasks:conversation.saveAsDefault')}</span>
            </label>
          )}
        </>
      }
      footerLeft={
        <>
          <ToolbarIconButton
            icon={PaperclipIcon}
            aria-label={t('tasks:taskFormDialog.attachFile')}
            title={t('tasks:taskFormDialog.attachFile')}
            onClick={handleAttachClick}
            disabled={isDisabled}
          />
          <input
            ref={fileInputRef}
            type="file"
            multiple
            className="hidden"
            onChange={handleFileInputChange}
          />
          <button
            type="button"
            onClick={onEditRepos}
            title={repoSummaryTitle}
            disabled={isDisabled}
            className="max-w-[320px] truncate text-sm text-normal hover:text-high disabled:cursor-not-allowed disabled:opacity-50"
          >
            {repoSummaryLabel}
          </button>
          {linkedIssue && (
            <>
              <div
                className="inline-flex items-center gap-half whitespace-nowrap text-sm text-low"
                title={linkedIssue.title}
              >
                <span className="font-mono text-xs text-normal">
                  {linkedIssue.simpleId}
                </span>
                <button
                  type="button"
                  onClick={linkedIssue.onRemove}
                  disabled={isDisabled}
                  className="inline-flex items-center text-low hover:text-error transition-colors disabled:cursor-not-allowed disabled:opacity-50"
                  aria-label={`Remove link to ${linkedIssue.simpleId}`}
                >
                  <XIcon className="size-icon-xs" weight="bold" />
                </button>
              </div>
            </>
          )}
        </>
      }
      footerRight={
        <PrimaryButton
          onClick={onSend}
          disabled={!canSend}
          actionIcon={isSending ? 'spinner' : undefined}
          value={
            isSending
              ? t('tasks:conversation.workspace.creating')
              : t('tasks:conversation.workspace.create')
          }
        />
      }
    />
  );
}

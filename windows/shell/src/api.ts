import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  Attachment,
  AttachmentItem,
  HistoryVersion,
  LibraryInfo,
  PageDto,
  PageEntry,
  PageHistory,
  PageRef,
  SaveResult,
  SearchHit,
} from './types'

/** Команды приложения на Rust. Ошибки приходят строкой с готовым текстом для человека. */
export const api = {
  library: () => invoke<LibraryInfo>('get_library'),
  pagesIndex: () => invoke<PageEntry[]>('get_pages_index'),
  page: (page: PageRef, version: HistoryVersion | null) => invoke<PageDto>('get_page', { page, version }),
  search: (query: string) => invoke<SearchHit[]>('search', { query }),
  backlinks: (page: PageRef) => invoke<SearchHit[]>('backlinks', { page }),
  attachments: (page: PageRef) => invoke<Attachment[]>('page_attachments', { page }),
  history: (page: PageRef) => invoke<PageHistory>('page_history', { page }),
  templates: (spaceId: string) => invoke<string[]>('templates', { spaceId }),
  createPage: (parent: PageRef, title: string, template: string | null) => invoke<PageRef>('create_page', { parent, title, template }),
  renamePage: (page: PageRef, title: string) => invoke<PageRef>('rename_page', { page, title }),
  trashPage: (page: PageRef) => invoke<void>('trash_page', { page }),
  commitSpace: (spaceId: string, message: string) => invoke<boolean>('commit_space', { spaceId, message }),
  editorSave: (pageKey: string, markdown: string, baseline: string | null, force: boolean) =>
    invoke<SaveResult>('editor_save', { pageKey, markdown, baseline, force }),
  setProperties: (page: PageRef, status: string | null, labels: string[]) =>
    invoke<{ oldHash: string; newHash: string }>('set_properties', { page, status, labels }),
  saveAttachment: (page: PageRef, name: string, data: string) => invoke<AttachmentItem>('save_attachment', { page, name, data }),
  copyAttachments: (page: PageRef, paths: string[]) => invoke<AttachmentItem[]>('copy_attachments', { page, paths }),
  openLink: (href: string, page: PageRef) => invoke<void>('open_link', { href, page }),
  openFile: (path: string) => invoke<void>('open_file', { path }),
  revealPage: (page: PageRef) => invoke<void>('reveal_page', { page }),
  revealPath: (path: string) => invoke<void>('reveal_path', { path }),
  copyPagePath: (page: PageRef) => invoke<void>('copy_page_path', { page }),
  createDefaultSpaces: () => invoke<void>('create_default_spaces'),
  createSpace: (name: string, kind: string, parentFolder: string) => invoke<void>('create_space', { name, kind, parentFolder }),
  addSpace: (folder: string) => invoke<void>('add_space', { folder }),
  removeSpace: (spaceId: string) => invoke<void>('remove_space', { spaceId }),
  forgetMissing: (path: string) => invoke<void>('forget_missing', { path }),
  defaultSpacesFolder: () => invoke<string>('default_spaces_folder'),
  newWindow: () => invoke<void>('new_window'),
  takePendingLinks: () => invoke<string[]>('take_pending_links'),
  cliStatus: () => invoke<{ folder: string | null; installed: boolean }>('cli_status'),
  cliInstall: () => invoke<void>('cli_install'),
  updateInfo: () => invoke<{ currentVersion: string; canUpdate: boolean }>('update_info'),
  updateCheck: () =>
    invoke<{ version: string; currentVersion: string; notes: string; downloaded: boolean } | null>('update_check'),
  updateDownload: (installOnExit: boolean) => invoke<void>('update_download', { installOnExit }),
  updateInstall: () => invoke<void>('update_install'),
}

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (message) => handler(message.payload))
}

/** Текст ошибки из команды Rust или из исключения. */
export function errorText(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return String(error)
}

import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { For, Match, Show, Switch, createResource, createSignal, onCleanup, onMount, type JSX } from 'solid-js'
import { Portal } from 'solid-js/web'
import * as actions from '../actions'
import { api, errorText } from '../api'
import { FolderOpenIcon, LibraryIcon, RotateCwIcon, TerminalIcon, XIcon } from '../icons'
import * as editor from '../store/editor'
import { errorMessage, library, nodeInfo, report, setErrorMessage } from '../store/library'
import * as navigation from '../store/navigation'
import type { PageRef } from '../types'
import { SpaceIcon } from './SpaceIcon'
import { UpdateDialog, UpdateSettings } from './Updates'

function Modal(props: { title?: string; onClose: () => void; width?: number; children: JSX.Element }) {
  let card!: HTMLDivElement
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      props.onClose()
    }
  }
  onMount(() => {
    document.addEventListener('keydown', onKeyDown, true)
    const focusable = card.querySelector<HTMLElement>('[autofocus], input, select, button.accent, button')
    focusable?.focus()
  })
  onCleanup(() => document.removeEventListener('keydown', onKeyDown, true))
  return (
    <Portal>
      <div class="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && props.onClose()}>
        <div ref={card} class="modal" role="dialog" aria-modal="true" aria-label={props.title} style={{ width: `${props.width ?? 460}px` }}>
          <Show when={props.title}>
            <div class="modal-title">
              <h2>{props.title}</h2>
              <button class="icon-button" title="Закрыть" onClick={props.onClose}>
                <XIcon size={16} />
              </button>
            </div>
          </Show>
          {props.children}
        </div>
      </div>
    </Portal>
  )
}

export function DialogHost() {
  const close = () => navigation.setSheet(null)
  return (
    <>
      <Switch>
        <Match when={navigation.sheet()?.kind === 'newPage' && navigation.sheet()}>
          {(sheet) => <NewPageForm parent={(sheet() as { parent: PageRef }).parent} close={close} />}
        </Match>
        <Match when={navigation.sheet()?.kind === 'rename' && navigation.sheet()}>
          {(sheet) => <RenameForm page={(sheet() as { page: PageRef }).page} close={close} />}
        </Match>
        <Match when={navigation.sheet()?.kind === 'settings'}>
          <Modal title="Настройки" onClose={close} width={620}>
            <Settings />
          </Modal>
        </Match>
        <Match when={navigation.sheet()?.kind === 'update'}>
          <Modal onClose={close} width={560}>
            <UpdateDialog close={close} />
          </Modal>
        </Match>
      </Switch>
      <Show when={navigation.pendingTrash()}>
        {(page) => (
          <Modal title={`Переместить «${nodeInfo(page())?.node.title ?? page().path}» в Корзину?`} onClose={() => navigation.setPendingTrash(null)}>
            <div class="dialog-body">
              <p>Вместе с подстраницами. Вернуть можно из Корзины или из истории пространства.</p>
              <div class="dialog-buttons">
                <button class="button" onClick={() => navigation.setPendingTrash(null)}>
                  Отмена
                </button>
                <button class="button danger" autofocus onClick={() => void actions.confirmTrash()}>
                  Переместить в Корзину
                </button>
              </div>
            </div>
          </Modal>
        )}
      </Show>
      <Show when={errorMessage()}>
        {(message) => (
          <Modal title="Не получилось" onClose={() => setErrorMessage(null)} width={420}>
            <div class="dialog-body">
              <p class="selectable">{message()}</p>
              <div class="dialog-buttons">
                <button class="button accent" autofocus onClick={() => setErrorMessage(null)}>
                  OK
                </button>
              </div>
            </div>
          </Modal>
        )}
      </Show>
    </>
  )
}

function NewPageForm(props: { parent: PageRef; close: () => void }) {
  const [title, setTitle] = createSignal('')
  const [template, setTemplate] = createSignal('')
  const [error, setError] = createSignal<string | null>(null)
  const [templates] = createResource(() => props.parent.spaceId, (spaceId) => api.templates(spaceId).catch(() => []))

  const create = async () => {
    if (!title().trim()) return
    try {
      const ref = await api.createPage(props.parent, title(), template() || null)
      props.close()
      navigation.open(ref)
      // Новую страницу сразу открываем на правку, как в Confluence.
      await editor.begin(ref)
    } catch (failure) {
      setError(errorText(failure))
    }
  }

  return (
    <Modal title="Новая страница" onClose={props.close}>
      <form
        class="dialog-body form"
        onSubmit={(event) => {
          event.preventDefault()
          void create()
        }}
      >
        <label class="field">
          <span>Заголовок</span>
          <input autofocus value={title()} onInput={(event) => setTitle(event.currentTarget.value)} />
        </label>
        <label class="field">
          <span>Шаблон</span>
          <select value={template()} onChange={(event) => setTemplate(event.currentTarget.value)}>
            <option value="">Без шаблона</option>
            <For each={templates() ?? []}>{(name) => <option value={name}>{name}</option>}</For>
          </select>
        </label>
        <div class="field">
          <span>Внутри</span>
          <span class="secondary">{nodeInfo(props.parent)?.node.title ?? ''}</span>
        </div>
        <Show when={error()}>
          <p class="error-text">{error()}</p>
        </Show>
        <div class="dialog-buttons">
          <button type="button" class="button" onClick={props.close}>
            Отмена
          </button>
          <button type="submit" class="button accent" disabled={!title().trim()}>
            Создать
          </button>
        </div>
      </form>
    </Modal>
  )
}

function RenameForm(props: { page: PageRef; close: () => void }) {
  const [title, setTitle] = createSignal(nodeInfo(props.page)?.node.title ?? '')
  const [error, setError] = createSignal<string | null>(null)

  const rename = async () => {
    try {
      const renamed = await editor.rename(props.page, title())
      if (navigation.selection()?.spaceId === props.page.spaceId && navigation.selection()?.path === props.page.path) {
        navigation.relocateTo(renamed)
      }
      props.close()
    } catch (failure) {
      setError(errorText(failure))
    }
  }

  return (
    <Modal title="Переименовать страницу" onClose={props.close}>
      <form
        class="dialog-body form"
        onSubmit={(event) => {
          event.preventDefault()
          void rename()
        }}
      >
        <label class="field">
          <span>Заголовок</span>
          <input autofocus value={title()} onInput={(event) => setTitle(event.currentTarget.value)} onFocus={(event) => event.currentTarget.select()} />
        </label>
        <p class="secondary small">Ссылки на страницу обновятся во всех пространствах.</p>
        <Show when={error()}>
          <p class="error-text">{error()}</p>
        </Show>
        <div class="dialog-buttons">
          <button type="button" class="button" onClick={props.close}>
            Отмена
          </button>
          <button type="submit" class="button accent" disabled={!title().trim()}>
            Переименовать
          </button>
        </div>
      </form>
    </Modal>
  )
}

function Settings() {
  const [tab, setTab] = createSignal<'spaces' | 'cli' | 'updates'>('spaces')
  return (
    <div class="settings">
      <nav class="tabs">
        <button classList={{ selected: tab() === 'spaces' }} onClick={() => setTab('spaces')}>
          <LibraryIcon size={16} />
          Пространства
        </button>
        <button classList={{ selected: tab() === 'cli' }} onClick={() => setTab('cli')}>
          <TerminalIcon size={16} />
          Командная строка
        </button>
        <button classList={{ selected: tab() === 'updates' }} onClick={() => setTab('updates')}>
          <RotateCwIcon size={16} />
          Обновления
        </button>
      </nav>
      <div class="settings-panel">
        <Switch>
          <Match when={tab() === 'spaces'}>
            <SpacesSettings />
          </Match>
          <Match when={tab() === 'cli'}>
            <CommandLineSettings />
          </Match>
          <Match when={tab() === 'updates'}>
            <UpdateSettings />
          </Match>
        </Switch>
      </div>
    </div>
  )
}

export async function chooseAndAddSpace(): Promise<void> {
  const folder = await openDialog({ directory: true, title: 'Выберите папку пространства Folio' })
  if (typeof folder !== 'string') return
  api.addSpace(folder).catch(report)
}

function SpacesSettings() {
  const [creating, setCreating] = createSignal(false)
  return (
    <div class="settings-section">
      <h3>Подключённые пространства</h3>
      <For each={library().spaces} fallback={<p class="secondary">Пространств пока нет.</p>}>
        {(space) => (
          <div class="settings-row">
            <span class="space-line">
              <SpaceIcon icon={space.icon} />
              <span>
                {space.name}
                <small class="middle-ellipsis" title={space.rootPath}>
                  {space.rootPath}
                </small>
              </span>
            </span>
            <span class="row-buttons">
              <button class="button" onClick={() => api.revealPath(space.rootPath).catch(report)}>
                Показать
              </button>
              <button class="button" onClick={() => api.removeSpace(space.id).catch(report)}>
                Отключить
              </button>
            </span>
          </div>
        )}
      </For>
      <p class="secondary small">«Отключить» убирает пространство из Folio, файлы остаются на месте.</p>
      <Show
        when={creating()}
        fallback={
          <div class="row-buttons">
            <button class="button" onClick={() => setCreating(true)}>
              Создать пространство…
            </button>
            <button class="button" onClick={() => void chooseAndAddSpace()}>
              Подключить готовую папку…
            </button>
          </div>
        }
      >
        <CreateSpaceForm close={() => setCreating(false)} />
      </Show>
    </div>
  )
}

function CreateSpaceForm(props: { close: () => void }) {
  const [name, setName] = createSignal('')
  const [kind, setKind] = createSignal('blank')
  const [folder, setFolder] = createSignal('')
  const [error, setError] = createSignal<string | null>(null)
  void api.defaultSpacesFolder().then(setFolder)

  const choose = async () => {
    const chosen = await openDialog({ directory: true, title: 'Где создать пространство' })
    if (typeof chosen === 'string') setFolder(chosen)
  }

  const create = async () => {
    try {
      await api.createSpace(name(), kind(), folder())
      props.close()
    } catch (failure) {
      setError(errorText(failure))
    }
  }

  return (
    <form
      class="create-space form"
      onSubmit={(event) => {
        event.preventDefault()
        void create()
      }}
    >
      <h3>Новое пространство</h3>
      <label class="field">
        <span>Название</span>
        <input autofocus value={name()} onInput={(event) => setName(event.currentTarget.value)} />
      </label>
      <label class="field">
        <span>Тип</span>
        <select value={kind()} onChange={(event) => setKind(event.currentTarget.value)}>
          <option value="work">Рабочее — шаблоны протокола, требований и ТЗ</option>
          <option value="personal">Личное — шаблон заметки</option>
          <option value="blank">Пустое</option>
        </select>
      </label>
      <div class="field">
        <span>Папка</span>
        <span class="folder-line">
          <span class="secondary middle-ellipsis" title={folder()}>
            {folder()}\{name().trim() || '…'}
          </span>
          <button type="button" class="button" onClick={() => void choose()}>
            <FolderOpenIcon size={14} />
            Выбрать…
          </button>
        </span>
      </div>
      <Show when={error()}>
        <p class="error-text">{error()}</p>
      </Show>
      <div class="dialog-buttons">
        <button type="button" class="button" onClick={props.close}>
          Отмена
        </button>
        <button type="submit" class="button accent" disabled={!name().trim()}>
          Создать
        </button>
      </div>
    </form>
  )
}

function CommandLineSettings() {
  const [status, { refetch }] = createResource(() => api.cliStatus())
  const [error, setError] = createSignal<string | null>(null)
  const install = async () => {
    try {
      await api.cliInstall()
      setError(null)
    } catch (failure) {
      setError(errorText(failure))
    }
    void refetch()
  }
  return (
    <div class="settings-section">
      <div class="settings-row">
        <span>Команда</span>
        <code>folio</code>
      </div>
      <div class="settings-row">
        <span>Состояние</span>
        <span class="secondary">{status()?.installed ? 'добавлена в PATH' : status()?.folder ? 'не добавлена в PATH' : 'folio.exe не найден'}</span>
      </div>
      <p class="secondary small">
        Утилита работает с теми же пространствами, что и приложение: дерево, поиск, новые страницы, переименование с починкой ссылок, проверка и
        история. Установка добавляет папку с folio.exe в переменную PATH вашей учётной записи — откройте терминал заново, чтобы она подхватилась.
      </p>
      <Show when={status()?.folder}>
        <p class="secondary small selectable">{status()?.folder}</p>
      </Show>
      <div class="row-buttons">
        <button class="button accent" disabled={!status()?.folder || status()?.installed} onClick={() => void install()}>
          {status()?.installed ? 'Установлена' : 'Добавить в PATH'}
        </button>
      </div>
      <Show when={error()}>
        <p class="error-text">{error()}</p>
      </Show>
    </div>
  )
}

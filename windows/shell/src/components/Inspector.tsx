import { For, Show, createEffect, createSignal, on, type JSX } from 'solid-js'
import { api } from '../api'
import { fileSize, fromSeconds, russianShortDate } from '../format'
import { ChevronRightIcon, ClockIcon, CornerUpLeftIcon, FileIcon, ImageIcon } from '../icons'
import * as editor from '../store/editor'
import { changedFolders, changedPages, historyChange, indexRevision, library, nodeInfo, report } from '../store/library'
import * as navigation from '../store/navigation'
import { preference, savePreference } from '../store/preferences'
import { refKey, type Attachment, type PageHistory, type PageRef, type SearchHit } from '../types'
import { showContextMenu } from './Menu'
import { saveSnapshot } from '../actions'

const PRESET_STATUSES = ['Черновик', 'В работе', 'На согласовании', 'Утверждено', 'Отклонено', 'Архив']
const IMAGE_EXTENSIONS = /\.(png|jpe?g|gif|webp|bmp|svg|ico|tiff?|heic|avif)$/i

export function Inspector() {
  return (
    <aside class="inspector">
      <Show when={nodeInfo(navigation.selection()) && navigation.selection()} fallback={<div class="empty-state small">Страница не выбрана</div>}>
        {(ref) => <PageInspector page={ref()} />}
      </Show>
    </aside>
  )
}

function Section(props: { id: string; title: string; children: JSX.Element }) {
  const [open, setOpen] = createSignal(preference(`inspector.${props.id}`, true))
  return (
    <section class="inspector-section">
      <button
        class="inspector-heading"
        aria-expanded={open()}
        onClick={() => {
          setOpen(!open())
          savePreference(`inspector.${props.id}`, open())
        }}
      >
        <ChevronRightIcon size={12} class="chevron" classList={{ open: open() }} />
        <span>{props.title}</span>
      </button>
      <Show when={open()}>
        <div class="inspector-content">{props.children}</div>
      </Show>
    </section>
  )
}

function PageInspector(props: { page: PageRef }) {
  const [backlinks, setBacklinks] = createSignal<SearchHit[]>([])
  const [attachments, setAttachments] = createSignal<Attachment[]>([])
  const [history, setHistory] = createSignal<PageHistory>({ revisions: [], hasUncommittedChanges: false })
  const key = () => refKey(props.page)

  createEffect(
    on([key, indexRevision], async () => {
      const page = props.page
      const found = await api.backlinks(page).catch(() => [])
      if (refKey(page) === key()) setBacklinks(found)
    }),
  )

  const [attachmentsRevision, setAttachmentsRevision] = createSignal(0)
  createEffect(on(changedFolders, (keys) => keys.has(key()) && setAttachmentsRevision((value) => value + 1), { defer: true }))
  createEffect(
    on([key, attachmentsRevision], async () => {
      const page = props.page
      const files = await api.attachments(page).catch(() => [])
      if (refKey(page) === key()) setAttachments(files)
    }),
  )

  // История перечитывается после снимков, правок снаружи и перестройки дерева; свои сохранения только отмечают «есть изменения».
  const [historyRevision, setHistoryRevision] = createSignal(0)
  createEffect(on(historyChange, (change) => change?.spaceId === props.page.spaceId && setHistoryRevision((value) => value + 1), { defer: true }))
  createEffect(on(changedPages, (keys) => keys.has(key()) && setHistoryRevision((value) => value + 1), { defer: true }))
  createEffect(on(() => library().revision, () => setHistoryRevision((value) => value + 1), { defer: true }))
  createEffect(
    on([key, historyRevision], async () => {
      const page = props.page
      const loaded = await api.history(page).catch(() => ({ revisions: [], hasUncommittedChanges: false }))
      if (refKey(page) === key()) setHistory(loaded)
    }),
  )
  createEffect(
    on(editor.saveState, (state) => {
      if (state.kind === 'saved' && !history().hasUncommittedChanges) setHistory({ ...history(), hasUncommittedChanges: true })
    }),
  )

  return (
    <div class="inspector-scroll">
      <Section id="properties" title="Свойства">
        <Properties page={props.page} />
      </Section>

      <Section id="outline" title="Содержание">
        <Show when={navigation.outline().length} fallback={<div class="placeholder">Заголовков нет</div>}>
          <For each={navigation.outline()}>
            {(item) => (
              <button class="inspector-link" style={{ 'padding-left': `${8 + (item.level - 1) * 12}px` }} onClick={() => navigation.requestHeading(item.index)}>
                {item.text}
              </button>
            )}
          </For>
        </Show>
      </Section>

      <Section id="backlinks" title="Ссылаются сюда">
        <Show when={backlinks().length} fallback={<div class="placeholder">Ссылок нет</div>}>
          <For each={backlinks()}>
            {(hit) => (
              <button class="inspector-link with-icon" onClick={() => navigation.open(hit.ref)}>
                <CornerUpLeftIcon size={14} />
                <span>{hit.title}</span>
              </button>
            )}
          </For>
        </Show>
      </Section>

      <Section id="attachments" title="Вложения">
        <Show when={attachments().length} fallback={<div class="placeholder">Файлов рядом со страницей нет</div>}>
          <For each={attachments()}>
            {(file) => (
              <button
                class="inspector-link with-icon"
                title={`${file.name} · ${fileSize(file.size)}`}
                onClick={() => api.openFile(file.path).catch(report)}
                onContextMenu={(event) =>
                  showContextMenu(event, [
                    { label: 'Открыть', action: () => void api.openFile(file.path).catch(report) },
                    { label: 'Показать в Проводнике', action: () => void api.revealPath(file.path).catch(report) },
                  ])
                }
              >
                {IMAGE_EXTENSIONS.test(file.name) ? <ImageIcon size={14} /> : <FileIcon size={14} />}
                <span class="middle-ellipsis">{file.name}</span>
              </button>
            )}
          </For>
        </Show>
      </Section>

      <Section id="history" title="История">
        <Show when={history().hasUncommittedChanges}>
          <button class="button small" title="В пространстве есть изменения, которых нет в истории" onClick={() => saveSnapshot(props.page.spaceId)}>
            <ClockIcon size={14} />
            <span>Сохранить снимок</span>
          </button>
        </Show>
        <RevisionRow title="Текущая версия" selected={navigation.version() === null} onSelect={() => navigation.setVersion(null)} />
        <For each={history().revisions}>
          {(revision) => (
            <RevisionRow
              title={revision.message}
              subtitle={russianShortDate(fromSeconds(revision.date))}
              selected={navigation.version()?.hash === revision.hash}
              onSelect={() => navigation.setVersion({ hash: revision.hash, path: revision.path, date: revision.date, message: revision.message })}
            />
          )}
        </For>
      </Section>
    </div>
  )
}

function RevisionRow(props: { title: string; subtitle?: string; selected: boolean; onSelect: () => void }) {
  return (
    <button class="revision-row" classList={{ selected: props.selected }} aria-pressed={props.selected} onClick={props.onSelect}>
      <span class="radio" classList={{ checked: props.selected }} />
      <span class="revision-text">
        <span class="revision-title">{props.title}</span>
        <Show when={props.subtitle}>
          <span class="revision-date">{props.subtitle}</span>
        </Show>
      </span>
    </button>
  )
}

function Properties(props: { page: PageRef }) {
  const node = () => nodeInfo(props.page)?.node
  const [labelsText, setLabelsText] = createSignal('')
  let labelsFocused = false

  // Метки есть только в шапке файла; дерево их не хранит, поэтому они берутся из последнего показа страницы.
  createEffect(() => {
    const labels = currentLabels()
    if (!labelsFocused) setLabelsText(labels.join(', '))
  })

  const statusOptions = () => {
    const current = node()?.status?.label
    return current && !PRESET_STATUSES.includes(current) ? [...PRESET_STATUSES, current] : PRESET_STATUSES
  }

  const saveLabels = () => {
    const labels = labelsText()
      .split(',')
      .map((label) => label.trim())
      .filter(Boolean)
    if (labels.join('\n') === currentLabels().join('\n')) return
    void editor.setProperties(props.page, node()?.status?.label ?? null, labels)
  }

  return (
    <div class="properties">
      <label class="property">
        <span>Статус</span>
        <select
          value={node()?.status?.label ?? ''}
          onChange={(event) => void editor.setProperties(props.page, event.currentTarget.value || null, currentLabels())}
        >
          <option value="">Без статуса</option>
          <For each={statusOptions()}>{(status) => <option value={status}>{status}</option>}</For>
        </select>
      </label>
      <label class="property">
        <span>Метки</span>
        <input
          type="text"
          placeholder="через запятую"
          value={labelsText()}
          onInput={(event) => setLabelsText(event.currentTarget.value)}
          onFocus={() => (labelsFocused = true)}
          onBlur={() => {
            labelsFocused = false
            saveLabels()
          }}
          onKeyDown={(event) => event.key === 'Enter' && event.currentTarget.blur()}
        />
      </label>
    </div>
  )
}

/** Метки открытой страницы из последнего показа: их присылает команда `get_page`. */
const [shownLabels, setShownLabels] = createSignal<{ key: string; labels: string[] }>({ key: '', labels: [] })

export function noteShownLabels(page: PageRef, labels: string[]): void {
  setShownLabels({ key: refKey(page), labels })
}

function currentLabels(): string[] {
  const ref = navigation.selection()
  return ref && shownLabels().key === refKey(ref) ? shownLabels().labels : []
}

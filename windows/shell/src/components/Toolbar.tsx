import { Show } from 'solid-js'
import * as actions from '../actions'
import {
  CheckIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  EllipsisIcon,
  ListIcon,
  PanelRightIcon,
  PencilIcon,
  PlusIcon,
  RedoIcon,
  SquarePenIcon,
  TableIcon,
  TypeIcon,
  UndoIcon,
} from '../icons'
import { russianShortDate, fromSeconds } from '../format'
import * as editor from '../store/editor'
import { ancestorTitles, nodeInfo } from '../store/library'
import * as navigation from '../store/navigation'
import { sameRef } from '../types'
import { MenuButton, type MenuItem } from './Menu'

const PANELS: [string, string][] = [
  ['info', 'Информация'],
  ['note', 'Заметка'],
  ['success', 'Успех'],
  ['warning', 'Предупреждение'],
  ['error', 'Ошибка'],
]

function blockToggle(label: string, command: string): MenuItem {
  return { label, action: () => editor.send(command), checked: editor.formatting().block === command }
}

function textMenu(): MenuItem[] {
  const state = editor.formatting()
  return [
    blockToggle('Обычный текст', 'paragraph'),
    blockToggle('Заголовок 1', 'heading1'),
    blockToggle('Заголовок 2', 'heading2'),
    blockToggle('Заголовок 3', 'heading3'),
    blockToggle('Цитата', 'blockquote'),
    blockToggle('Блок кода', 'codeBlock'),
    { separator: true },
    { label: 'Жирный', action: () => editor.send('bold'), checked: state.bold, shortcut: 'Ctrl+B' },
    { label: 'Курсив', action: () => editor.send('italic'), checked: state.italic, shortcut: 'Ctrl+I' },
    { label: 'Зачёркнутый', action: () => editor.send('strike'), checked: state.strike },
    { label: 'Код', action: () => editor.send('code'), checked: state.code },
    { label: state.link ? 'Изменить ссылку…' : 'Ссылка…', action: () => editor.send('link'), shortcut: 'Ctrl+K' },
    { separator: true },
    { label: 'Очистить форматирование', action: () => editor.send('clearFormatting') },
  ]
}

function listMenu(): MenuItem[] {
  return [
    blockToggle('Маркированный список', 'bulletList'),
    blockToggle('Нумерованный список', 'orderedList'),
    blockToggle('Задачи', 'taskList'),
    { separator: true },
    { label: 'Переместить блок выше', action: () => editor.send('moveBlockUp'), shortcut: 'Alt+Shift+↑' },
    { label: 'Переместить блок ниже', action: () => editor.send('moveBlockDown'), shortcut: 'Alt+Shift+↓' },
  ]
}

function insertMenu(): MenuItem[] {
  return [
    { label: 'Таблица', action: () => editor.send('insertTable') },
    { label: 'Панель', submenu: PANELS.map(([type, label]) => ({ label, action: () => editor.send('insertPanel', { type }) })) },
    { label: 'Раскрывающийся блок', action: () => editor.send('insertExpand') },
    { label: 'Статус', action: () => editor.send('insertStatus') },
    { separator: true },
    { label: 'Ссылка на страницу…', action: () => editor.send('insertPageLink') },
    { label: 'Картинка или файл…', action: () => editor.send('requestAttachment') },
    { separator: true },
    { label: 'Оглавление', action: () => editor.send('insertToc') },
    { label: 'Дочерние страницы', action: () => editor.send('insertChildren') },
    { label: 'Разделитель', action: () => editor.send('horizontalRule') },
  ]
}

function tableMenu(): MenuItem[] {
  return [
    { label: 'Строка выше', action: () => editor.send('addRowBefore') },
    { label: 'Строка ниже', action: () => editor.send('addRowAfter') },
    { label: 'Столбец слева', action: () => editor.send('addColumnBefore') },
    { label: 'Столбец справа', action: () => editor.send('addColumnAfter') },
    { separator: true },
    { label: 'Строка заголовков', action: () => editor.send('toggleHeaderRow') },
    { separator: true },
    { label: 'Удалить строку', action: () => editor.send('deleteRow') },
    { label: 'Удалить столбец', action: () => editor.send('deleteColumn') },
    { label: 'Удалить таблицу', action: () => editor.send('deleteTable'), danger: true },
  ]
}

function moreMenu(): MenuItem[] {
  const ref = navigation.selection()
  const isPage = !!ref && !!nodeInfo(ref)
  return [
    { label: 'Переименовать…', action: () => actions.renamePage(), disabled: !isPage || !ref?.path, shortcut: 'F2' },
    { label: 'Показать в Проводнике', action: () => actions.revealPage(), disabled: !isPage, shortcut: 'Ctrl+Alt+R' },
    { label: 'Скопировать путь к файлу', action: () => actions.copyPagePath(), disabled: !isPage, shortcut: 'Ctrl+Alt+Shift+C' },
    { separator: true },
    { label: 'Сохранить снимок в истории', action: () => actions.saveSnapshot(), disabled: !isPage, shortcut: 'Ctrl+Alt+S' },
    { separator: true },
    { label: 'Новое окно', action: actions.newWindow, shortcut: 'Ctrl+Shift+N' },
    { label: 'Настройки…', action: actions.openSettings, shortcut: 'Ctrl+,' },
    { separator: true },
    { label: 'Переместить в Корзину…', action: () => actions.trashPage(), disabled: !isPage || !ref?.path, danger: true },
  ]
}

export function Toolbar() {
  const title = () => nodeInfo(navigation.selection())?.node.title ?? ''
  const subtitle = () => {
    const ref = navigation.selection()
    if (!ref) return ''
    const path = ancestorTitles(ref).join(' › ')
    let state = ''
    if (editor.isEditing() && sameRef(editor.editingRef(), ref)) {
      const save = editor.saveState()
      state = save.kind === 'idle' ? 'правка' : save.kind === 'saved' ? 'правка · сохранено' : `не сохранено: ${save.message}`
    } else {
      const version = navigation.version()
      if (version) state = `версия от ${russianShortDate(fromSeconds(version.date))}`
    }
    return [path, state].filter(Boolean).join(' · ')
  }

  return (
    <header class="toolbar">
      <div class="toolbar-group">
        <button class="toolbar-button" title="Назад (Alt+←)" disabled={!navigation.canGoBack()} onClick={navigation.goBack}>
          <ChevronLeftIcon size={18} />
        </button>
        <button class="toolbar-button" title="Вперёд (Alt+→)" disabled={!navigation.canGoForward()} onClick={navigation.goForward}>
          <ChevronRightIcon size={18} />
        </button>
      </div>
      <div class="toolbar-title">
        <div class="title" title={title()}>
          {title()}
        </div>
        <Show when={subtitle()}>
          <div class="subtitle" title={subtitle()}>
            {subtitle()}
          </div>
        </Show>
      </div>
      <div class="toolbar-group">
        <Show
          when={editor.isEditing()}
          fallback={
            <>
              <button class="toolbar-button labeled" title="Редактировать страницу (Ctrl+E)" disabled={!actions.canEdit()} onClick={actions.toggleEditing}>
                <PencilIcon size={16} />
                <span>Редактировать</span>
              </button>
              <button class="toolbar-button" title="Новая подстраница открытой страницы (Ctrl+N)" disabled={!navigation.selection()} onClick={() => actions.newPage()}>
                <SquarePenIcon size={17} />
              </button>
            </>
          }
        >
          <button class="toolbar-button" title="Отменить (Ctrl+Z)" disabled={!editor.formatting().canUndo} onClick={() => editor.send('undo')}>
            <UndoIcon size={17} />
          </button>
          <button class="toolbar-button" title="Повторить (Ctrl+Y)" disabled={!editor.formatting().canRedo} onClick={() => editor.send('redo')}>
            <RedoIcon size={17} />
          </button>
          <span class="toolbar-divider" />
          <MenuButton title="Стиль и оформление текста" items={textMenu}>
            <TypeIcon size={17} />
            <ChevronDownIcon size={12} />
          </MenuButton>
          <MenuButton title="Списки и порядок блоков" items={listMenu}>
            <ListIcon size={17} />
            <ChevronDownIcon size={12} />
          </MenuButton>
          <MenuButton title="Вставить элемент — или введите «/» в тексте" items={insertMenu}>
            <PlusIcon size={17} />
            <ChevronDownIcon size={12} />
          </MenuButton>
          <MenuButton title="Строки и столбцы таблицы под курсором" items={tableMenu} disabled={!editor.formatting().inTable}>
            <TableIcon size={17} />
            <ChevronDownIcon size={12} />
          </MenuButton>
          <span class="toolbar-divider" />
          <button class="button accent" title="Закончить правку и сохранить снимок в истории (Ctrl+E)" onClick={() => void editor.finish()}>
            <CheckIcon size={16} />
            <span>Готово</span>
          </button>
        </Show>
        <MenuButton title="Ещё" items={moreMenu}>
          <EllipsisIcon size={18} />
        </MenuButton>
        <button
          class="toolbar-button"
          classList={{ pressed: navigation.inspectorVisible() }}
          title="Показать или скрыть инспектор (Ctrl+Alt+I)"
          onClick={actions.toggleInspector}
        >
          <PanelRightIcon size={18} />
        </button>
      </div>
    </header>
  )
}

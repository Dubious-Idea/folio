import { For, Show, createSignal, onCleanup, onMount, type JSX } from 'solid-js'
import { Portal } from 'solid-js/web'

export type MenuItem =
  | {
      label: string
      action: () => void
      checked?: boolean
      disabled?: boolean
      danger?: boolean
      shortcut?: string
    }
  | { separator: true }
  | { label: string; submenu: MenuItem[] }

interface OpenMenu {
  items: MenuItem[]
  x: number
  y: number
  /** Нижняя граница кнопки: меню открывается под ней или над ней, если снизу не хватает места. */
  anchorTop?: number
}

const [openMenu, setOpenMenu] = createSignal<OpenMenu | null>(null)

export function showContextMenu(event: MouseEvent, items: MenuItem[]): void {
  event.preventDefault()
  event.stopPropagation()
  setOpenMenu({ items, x: event.clientX, y: event.clientY })
}

export function showMenuBelow(element: HTMLElement, items: MenuItem[]): void {
  const rect = element.getBoundingClientRect()
  setOpenMenu({ items, x: rect.left, y: rect.bottom + 2, anchorTop: rect.top })
}

export function closeMenu(): void {
  setOpenMenu(null)
}

export function isMenuOpen(): boolean {
  return openMenu() !== null
}

/** Кнопка панели, открывающая выпадающее меню. */
export function MenuButton(props: { title: string; items: () => MenuItem[]; disabled?: boolean; children: JSX.Element }) {
  let button!: HTMLButtonElement
  return (
    <button
      ref={button}
      class="toolbar-button has-menu"
      title={props.title}
      aria-haspopup="menu"
      disabled={props.disabled}
      onClick={() => showMenuBelow(button, props.items())}
    >
      {props.children}
    </button>
  )
}

export function MenuHost() {
  return (
    <Show when={openMenu()}>
      {(menu) => (
        <Portal>
          <div class="menu-backdrop" onMouseDown={closeMenu} onContextMenu={(event) => (event.preventDefault(), closeMenu())} />
          <MenuList items={menu().items} x={menu().x} y={menu().y} anchorTop={menu().anchorTop} />
        </Portal>
      )}
    </Show>
  )
}

function MenuList(props: { items: MenuItem[]; x: number; y: number; anchorTop?: number }) {
  let list!: HTMLDivElement
  const [position, setPosition] = createSignal({ left: props.x, top: props.y })
  const [submenu, setSubmenu] = createSignal<{ items: MenuItem[]; x: number; y: number } | null>(null)
  const [active, setActive] = createSignal(-1)

  onMount(() => {
    const rect = list.getBoundingClientRect()
    const left = Math.max(4, Math.min(props.x, window.innerWidth - rect.width - 4))
    let top = props.y
    if (top + rect.height > window.innerHeight - 4) {
      top = props.anchorTop !== undefined ? props.anchorTop - rect.height - 2 : window.innerHeight - rect.height - 4
    }
    setPosition({ left, top: Math.max(4, top) })
    list.focus()
  })

  const enabledIndexes = () => props.items.map((item, index) => ({ item, index })).filter(({ item }) => !('separator' in item) && !('disabled' in item && item.disabled))

  const activate = (item: MenuItem, element: HTMLElement) => {
    if ('separator' in item) return
    if ('submenu' in item) {
      const rect = element.getBoundingClientRect()
      setSubmenu({ items: item.submenu, x: rect.right - 2, y: rect.top - 4 })
      return
    }
    if (item.disabled) return
    closeMenu()
    item.action()
  }

  const onKeyDown = (event: KeyboardEvent) => {
    const enabled = enabledIndexes()
    const position = enabled.findIndex(({ index }) => index === active())
    if (event.key === 'Escape') {
      event.preventDefault()
      closeMenu()
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : -1
      const next = enabled[(position + step + enabled.length) % enabled.length]
      if (next) setActive(next.index)
    } else if (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowRight') {
      const element = list.querySelector<HTMLElement>(`[data-index="${active()}"]`)
      const item = props.items[active()]
      if (item && element) {
        event.preventDefault()
        activate(item, element)
      }
    }
  }

  const closeOnBlur = () => {
    if (!document.hasFocus()) closeMenu()
  }
  window.addEventListener('blur', closeOnBlur)
  onCleanup(() => window.removeEventListener('blur', closeOnBlur))

  return (
    <>
      <div
        ref={list}
        class="menu"
        role="menu"
        tabIndex={-1}
        style={{ left: `${position().left}px`, top: `${position().top}px` }}
        onKeyDown={onKeyDown}
        onContextMenu={(event) => event.preventDefault()}
      >
        <For each={props.items}>
          {(item, index) =>
            'separator' in item ? (
              <div class="menu-separator" role="separator" />
            ) : (
              <button
                class="menu-item"
                classList={{ danger: 'danger' in item && !!item.danger, active: active() === index() }}
                role="menuitem"
                data-index={index()}
                disabled={'disabled' in item && item.disabled}
                onMouseEnter={(event) => {
                  setActive(index())
                  if ('submenu' in item) activate(item, event.currentTarget)
                  else setSubmenu(null)
                }}
                onClick={(event) => activate(item, event.currentTarget)}
              >
                <span class="menu-check">{'checked' in item && item.checked ? '✓' : ''}</span>
                <span class="menu-label">{item.label}</span>
                <span class="menu-shortcut">{'shortcut' in item ? item.shortcut : 'submenu' in item ? '›' : ''}</span>
              </button>
            )
          }
        </For>
      </div>
      <Show when={submenu()}>{(open) => <MenuList items={open().items} x={open().x} y={open().y} />}</Show>
    </>
  )
}

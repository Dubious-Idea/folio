import { For, Match, Show, Switch } from 'solid-js'
import { russianShortDate } from '../format'
import { CircleArrowDownIcon, CircleCheckIcon, RotateCwIcon, TriangleAlertIcon } from '../icons'
import * as navigation from '../store/navigation'
import * as updater from '../store/updater'

/** Строка внизу боковой панели, когда есть новая версия. */
export function UpdateBanner() {
  return (
    <Switch>
      <Match when={updater.phase().kind === 'readyToInstall' && updater.availableRelease()}>
        {(release) => (
          <button
            class="update-banner"
            title="Обновление уже скачано и само установится при выходе из Folio"
            onClick={() => void updater.install(release())}
          >
            <RotateCwIcon size={16} />
            <span>Перезапустить для версии {release().version}</span>
          </button>
        )}
      </Match>
      <Match when={updater.availableRelease()}>
        {(release) => (
          <button
            class="update-banner"
            onClick={() => {
              updater.present(release())
              navigation.setSheet({ kind: 'update' })
            }}
          >
            <CircleArrowDownIcon size={16} />
            <span>Доступна версия {release().version}</span>
          </button>
        )}
      </Match>
    </Switch>
  )
}

/** Окно «Доступна новая версия». */
export function UpdateDialog(props: { close: () => void }) {
  const release = () => updater.presented() ?? updater.availableRelease()
  return (
    <Show
      when={release()}
      fallback={
        <div class="dialog-body center">
          <CircleCheckIcon size={36} class="accent" />
          <h2>Установлена последняя версия</h2>
          <p class="secondary">Folio {updater.currentVersion() ?? ''}</p>
          <div class="dialog-buttons">
            <button class="button accent" onClick={props.close}>
              OK
            </button>
          </div>
        </div>
      }
    >
      {(current) => (
        <div class="dialog-body update-dialog">
          <div class="update-header">
            <img src="/icon.png" alt="" width="56" height="56" />
            <div>
              <h2>Доступна версия {current().version}</h2>
              <p class="secondary">Сейчас установлена {updater.currentVersion() ?? 'сборка для разработки'}</p>
              <a class="link" href="https://github.com/Dubious-Idea/folio/releases" target="_blank" rel="noreferrer">
                Страница выпуска
              </a>
            </div>
          </div>
          <div class="update-notes">
            <Show when={updater.displayNotes(current().notes).length} fallback={<p>Описание изменений — на странице выпуска.</p>}>
              <For each={updater.displayNotes(current().notes)}>{(line) => (line.heading ? <strong>{line.text}</strong> : <p>{line.text}</p>)}</For>
            </Show>
          </div>
          <UpdateStatus />
          <div class="dialog-buttons spread">
            <button
              class="button"
              disabled={updater.isBusy()}
              onClick={() => {
                updater.skip(current())
                props.close()
              }}
            >
              Пропустить версию
            </button>
            <span class="spacer" />
            <button class="button" onClick={props.close}>
              Позже
            </button>
            <button class="button accent" disabled={updater.isBusy()} onClick={() => void updater.install(current())}>
              {updater.phase().kind === 'readyToInstall' ? 'Перезапустить' : 'Установить'}
            </button>
          </div>
        </div>
      )}
    </Show>
  )
}

function UpdateStatus() {
  return (
    <Switch>
      <Match when={updater.phase().kind === 'downloading' && updater.phase()}>
        {(phase) => (
          <div class="progress-row">
            <span>Загрузка обновления…</span>
            <progress max="1" value={'fraction' in phase() ? (phase() as { fraction: number }).fraction : 0} />
          </div>
        )}
      </Match>
      <Match when={updater.phase().kind === 'readyToInstall'}>
        <p class="secondary">Обновление скачано и установится при выходе из Folio.</p>
      </Match>
      <Match when={updater.phase().kind === 'installing'}>
        <div class="progress-row">
          <span>Перезапуск…</span>
          <progress />
        </div>
      </Match>
      <Match when={updater.phase().kind === 'failed' && updater.phase()}>
        {(phase) => (
          <p class="error-text">
            <TriangleAlertIcon size={14} /> {'message' in phase() ? (phase() as { message: string }).message : ''}
          </p>
        )}
      </Match>
    </Switch>
  )
}

export function UpdateSettings() {
  const statusText = () => {
    const phase = updater.phase()
    switch (phase.kind) {
      case 'idle': {
        const checked = updater.lastCheck()
        return checked ? `Проверено ${russianShortDate(checked)}` : 'Ещё не проверялись'
      }
      case 'checking':
        return 'Проверяю…'
      case 'upToDate':
        return 'Установлена последняя версия'
      case 'available':
        return `Доступна версия ${phase.release.version}`
      case 'downloading':
        return `Загрузка: ${Math.round(phase.fraction * 100)}%`
      case 'readyToInstall':
        return `Версия ${phase.release.version} скачана и установится при выходе`
      case 'installing':
        return 'Перезапуск…'
      case 'failed':
        return phase.message
    }
  }

  return (
    <div class="settings-section">
      <div class="settings-row">
        <span>Установленная версия</span>
        <span class="secondary">{updater.currentVersion() ?? 'сборка для разработки'}</span>
      </div>
      <label class="settings-row toggle-row">
        <span>Проверять обновления автоматически</span>
        <input
          type="checkbox"
          class="toggle"
          checked={updater.checksAutomatically()}
          disabled={!updater.canUpdate()}
          onChange={(event) => updater.setChecksAutomatically(event.currentTarget.checked)}
        />
      </label>
      <label class="settings-row toggle-row">
        <span>
          Устанавливать обновления автоматически
          <small>Новая версия скачивается в фоне и ставится, когда вы закрываете Folio</small>
        </span>
        <input
          type="checkbox"
          class="toggle"
          checked={updater.installsAutomatically()}
          disabled={!updater.canUpdate() || !updater.checksAutomatically()}
          onChange={(event) => updater.setInstallsAutomatically(event.currentTarget.checked)}
        />
      </label>
      <div class="settings-row">
        <span>
          Состояние
          <small>{statusText()}</small>
        </span>
        <Switch
          fallback={
            <button class="button" disabled={!updater.canUpdate() || updater.isBusy()} onClick={() => void checkNow()}>
              Проверить сейчас
            </button>
          }
        >
          <Match when={updater.phase().kind === 'readyToInstall' && updater.availableRelease()}>
            {(release) => (
              <button class="button accent" onClick={() => void updater.install(release())}>
                Перезапустить сейчас
              </button>
            )}
          </Match>
          <Match when={updater.availableRelease()}>
            {(release) => (
              <button
                class="button"
                onClick={() => {
                  updater.present(release())
                  navigation.setSheet({ kind: 'update' })
                }}
              >
                Установить {release().version}…
              </button>
            )}
          </Match>
        </Switch>
      </div>
      <a class="link" href="https://github.com/Dubious-Idea/folio/releases" target="_blank" rel="noreferrer">
        Все выпуски на GitHub
      </a>
    </div>
  )
}

export async function checkNow(): Promise<void> {
  await updater.check(true)
  if (updater.presented() || updater.phase().kind === 'upToDate') navigation.setSheet({ kind: 'update' })
}

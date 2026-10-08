import { getCurrentWindow } from '@tauri-apps/api/window'
import { createSignal } from 'solid-js'
import { api, errorText, on } from '../api'
import { preference, savePreference } from './preferences'

export interface Release {
  version: string
  notes: string
}

export type Phase =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'upToDate' }
  | { kind: 'available'; release: Release }
  | { kind: 'downloading'; release: Release; fraction: number }
  /** Скачано и установится при выходе из приложения. */
  | { kind: 'readyToInstall'; release: Release }
  | { kind: 'installing'; release: Release }
  | { kind: 'failed'; message: string }

const [phase, setPhase] = createSignal<Phase>({ kind: 'idle' })
const [lastCheck, setLastCheck] = createSignal<Date | null>(null)
const [checksAutomatically, setChecks] = createSignal(preference('checkForUpdatesAutomatically', true))
const [installsAutomatically, setInstalls] = createSignal(preference('installUpdatesAutomatically', true))
const [currentVersion, setCurrentVersion] = createSignal<string | null>(null)
const [canUpdate, setCanUpdate] = createSignal(false)
/** Выпуск, о котором нужно спросить в окне обновления. */
const [presented, setPresented] = createSignal<Release | null>(null)

export { canUpdate, checksAutomatically, currentVersion, installsAutomatically, lastCheck, phase, presented }

const LAUNCH_DELAY_MS = 8_000
const CHECK_INTERVAL_MS = 6 * 3600 * 1000
let schedule: number | undefined

export function availableRelease(): Release | null {
  const current = phase()
  return 'release' in current ? current.release : null
}

export function isBusy(): boolean {
  const kind = phase().kind
  return kind === 'checking' || kind === 'downloading' || kind === 'installing'
}

export function present(release: Release | null): void {
  setPresented(release)
}

export function setChecksAutomatically(enabled: boolean): void {
  setChecks(enabled)
  savePreference('checkForUpdatesAutomatically', enabled)
  reschedule()
}

export function setInstallsAutomatically(enabled: boolean): void {
  setInstalls(enabled)
  savePreference('installUpdatesAutomatically', enabled)
}

export function skip(release: Release): void {
  savePreference('skippedUpdateVersion', release.version)
}

export async function startUpdater(): Promise<void> {
  const info = await api.updateInfo()
  setCurrentVersion(info.currentVersion)
  setCanUpdate(info.canUpdate)
  await on<number>('update-progress', (fraction) => {
    const current = phase()
    if (current.kind === 'downloading') setPhase({ ...current, fraction })
  })
  // Проверяет только главное окно, остальные узнают о выпуске сами при открытии настроек.
  if (getCurrentWindow().label === 'main') reschedule()
}

function reschedule(): void {
  window.clearTimeout(schedule)
  if (!checksAutomatically() || !canUpdate()) return
  const run = async () => {
    await check(false)
    schedule = window.setTimeout(run, CHECK_INTERVAL_MS)
  }
  schedule = window.setTimeout(run, LAUNCH_DELAY_MS)
}

export async function check(userInitiated: boolean): Promise<void> {
  const current = phase()
  if (current.kind === 'readyToInstall') {
    if (userInitiated) present(current.release)
    return
  }
  if (isBusy()) return
  setPhase({ kind: 'checking' })
  try {
    const found = await api.updateCheck()
    setLastCheck(new Date())
    if (!found) {
      setPhase({ kind: 'upToDate' })
      return
    }
    const release = { version: found.version, notes: found.notes }
    if (found.downloaded) {
      setPhase({ kind: 'readyToInstall', release })
      return
    }
    const skipped = preference<string | null>('skippedUpdateVersion', null) === release.version
    if (!userInitiated && skipped) {
      setPhase({ kind: 'available', release })
      return
    }
    if (!userInitiated && installsAutomatically()) {
      void download(release, false)
      return
    }
    setPhase({ kind: 'available', release })
    present(release)
  } catch (error) {
    setPhase({ kind: 'failed', message: errorText(error) })
  }
}

async function download(release: Release, relaunchWhenReady: boolean): Promise<void> {
  setPhase({ kind: 'downloading', release, fraction: 0 })
  try {
    await api.updateDownload(!relaunchWhenReady && installsAutomatically())
    if (relaunchWhenReady) {
      await installNow(release)
    } else {
      setPhase({ kind: 'readyToInstall', release })
    }
  } catch (error) {
    setPhase({ kind: 'failed', message: errorText(error) })
  }
}

/** Скачать, если ещё не скачано, и сразу перезапуститься в новую версию. */
export async function install(release: Release): Promise<void> {
  if (phase().kind === 'readyToInstall') {
    await installNow(release)
    return
  }
  await download(release, true)
}

async function installNow(release: Release): Promise<void> {
  setPhase({ kind: 'installing', release })
  try {
    await api.updateInstall()
  } catch (error) {
    setPhase({ kind: 'failed', message: errorText(error) })
  }
}

/** Текст для окна обновления: без раздела «Установка». */
export function displayNotes(markdown: string): { heading: boolean; text: string }[] {
  const lines: { heading: boolean; text: string }[] = []
  let skipping = false
  for (const line of markdown.split(/\r?\n/)) {
    if (line.startsWith('#')) {
      const title = line.replace(/^#+/, '').trim()
      skipping = title.toLowerCase().includes('установка')
      if (!skipping) lines.push({ heading: true, text: title })
      continue
    }
    if (!skipping) lines.push({ heading: false, text: line })
  }
  while (lines.length && !lines.at(-1)?.text.trim()) lines.pop()
  while (lines.length && !lines[0].text.trim()) lines.shift()
  return lines
}

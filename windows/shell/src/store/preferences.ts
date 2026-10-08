/** Настройки окна в localStorage: раскрытые ветки, ширина панелей, свёрнутые разделы инспектора. */
const PREFIX = 'folio.'

export function preference<T>(key: string, fallback: T): T {
  try {
    const stored = localStorage.getItem(PREFIX + key)
    return stored === null ? fallback : (JSON.parse(stored) as T)
  } catch {
    return fallback
  }
}

export function savePreference(key: string, value: unknown): void {
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify(value))
  } catch {
    // Хранилище недоступно — настройка просто не запомнится.
  }
}

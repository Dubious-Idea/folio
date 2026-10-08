/** Сокращения месяцев, как в русской локали: «9 окт. 2026 г.». */
const SHORT_MONTHS = ['янв.', 'февр.', 'мар.', 'апр.', 'мая', 'июн.', 'июл.', 'авг.', 'сент.', 'окт.', 'нояб.', 'дек.']

const pad = (value: number) => String(value).padStart(2, '0')

/** «15.09.2026, 20:41». */
export function russianDateTime(date: Date): string {
  return `${pad(date.getDate())}.${pad(date.getMonth() + 1)}.${date.getFullYear()}, ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

/** «15 сент. 2026 г., 20:41» — коротко, как в списке версий. */
export function russianShortDate(date: Date): string {
  return `${date.getDate()} ${SHORT_MONTHS[date.getMonth()]} ${date.getFullYear()} г., ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

export function fromSeconds(seconds: number): Date {
  return new Date(seconds * 1000)
}

/** Ключ заголовка: как `title_key` в ядре — без учёта регистра и лишних пробелов. */
export function titleKey(text: string): string {
  return text.split(/\s+/).filter(Boolean).join(' ').toLowerCase()
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} Б`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} КБ`
  return `${(bytes / 1024 / 1024).toFixed(1)} МБ`
}

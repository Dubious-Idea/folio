import { render } from 'solid-js/web'
import { App } from './App'
import './styles.css'

// Контекстное меню WebView2 («Назад», «Обновить», «Проверить») в приложении не к месту: свои меню рисует интерфейс.
document.addEventListener('contextmenu', (event) => {
  const target = event.target as HTMLElement | null
  const isText = target?.closest('input, textarea, [contenteditable="true"]')
  if (!isText) event.preventDefault()
})

render(() => <App />, document.getElementById('root')!)

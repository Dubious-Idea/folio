import { cpSync, existsSync, readFileSync, statSync } from 'node:fs'
import { dirname, extname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig, type Plugin } from 'vite'
import solid from 'vite-plugin-solid'

// Редактор страниц — общая с macOS веб-часть из web/. В окне он живёт во фрейме по адресу /page/.
const here = dirname(fileURLToPath(import.meta.url))
const editorDist = resolve(here, '../../web/dist')

function pageEditor(): Plugin {
  const types: Record<string, string> = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' }
  return {
    name: 'folio-page-editor',
    configureServer(server) {
      server.middlewares.use('/page', (request, response, next) => {
        const relative = decodeURIComponent((request.url ?? '/').split('?')[0]).replace(/^\/+/, '') || 'index.html'
        const file = join(editorDist, relative)
        if (!file.startsWith(editorDist) || !existsSync(file) || !statSync(file).isFile()) return next()
        response.setHeader('Content-Type', types[extname(file)] ?? 'application/octet-stream')
        response.end(readFileSync(file))
      })
    },
    writeBundle(options) {
      if (!existsSync(join(editorDist, 'index.html'))) {
        throw new Error('Нет сборки редактора: выполните `npm ci && npm run build` в папке web')
      }
      cpSync(editorDist, join(options.dir ?? 'dist', 'page'), { recursive: true })
    },
  }
}

export default defineConfig({
  root: here,
  plugins: [solid(), pageEditor()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'chrome120',
    chunkSizeWarningLimit: 1000,
  },
})

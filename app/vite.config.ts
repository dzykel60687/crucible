// Vite serves the UI on port 1420 for `tauri dev`.
//
// Dev-only bridge: when the UI is opened in a plain browser (no Tauri), it POSTs command text
// to /__crucible. This middleware feeds it to `crucible --json shell` built from this workspace,
// so the browser view drives the REAL engine, not a copy. Never part of a release build.
import { defineConfig, type Plugin } from 'vite'
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { existsSync } from 'node:fs'
import { resolve } from 'node:path'

function engineBridge(): Plugin {
  let child: ChildProcessWithoutNullStreams | null = null
  let buffer = ''
  let waiting: ((line: string) => void)[] = []
  let queue = Promise.resolve()

  const start = () => {
    const exe = resolve(__dirname, '../target/debug/crucible.exe')
    if (!existsSync(exe)) throw new Error(`build the CLI first: cargo build -p crucible-cli (${exe} missing)`)
    child = spawn(exe, ['--json', 'shell'], { stdio: 'pipe' })
    child.stdout.setEncoding('utf8')
    child.stdout.on('data', (d: string) => {
      buffer += d
      let i
      while ((i = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, i).trim()
        buffer = buffer.slice(i + 1)
        if (line.startsWith('{')) waiting.shift()?.(line)
      }
    })
    child.on('exit', () => { child = null; waiting = [] })
  }

  // One request at a time: send the line, then project.state, and collect JSON lines until the
  // state arrives. The shell stops a line at its first error, as the window does.
  const run = (line: string) => new Promise<object>((done) => {
    if (!child) start()
    const replies: any[] = []
    const take = (raw: string) => {
      const r = JSON.parse(raw)
      if (r.command === 'project.state') done({ replies, state: r.data })
      else { replies.push(r); waiting.push(take) }
    }
    waiting.push(take)
    child!.stdin.write(line.replace(/\r?\n/g, ' ') + '\nproject.state\n')
  })

  return {
    name: 'crucible-engine-bridge',
    configureServer(server) {
      server.middlewares.use('/__crucible', (req, res) => {
        let body = ''
        req.on('data', (c) => (body += c))
        req.on('end', () => {
          queue = queue.then(async () => {
            try {
              const out = await run(body)
              res.setHeader('content-type', 'application/json')
              res.end(JSON.stringify(out))
            } catch (e) {
              res.statusCode = 500
              res.end(String(e))
            }
          })
        })
      })
    },
  }
}

export default defineConfig({
  plugins: [engineBridge()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: '127.0.0.1' },
  build: { target: 'es2022', outDir: 'dist', emptyOutDir: true },
})

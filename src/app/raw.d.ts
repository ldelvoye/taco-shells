// Which app this build is, replaced at build time by electron.vite.config.ts.
declare const __CHANNEL__: string

// Vite inlines a `?raw` import as the file's text. The main process has no
// vite/client types, so the shape is declared here.
declare module '*?raw' {
  const content: string
  export default content
}

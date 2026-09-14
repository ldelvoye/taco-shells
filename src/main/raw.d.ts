// Vite inlines a `?raw` import as the file's text. The main process has no
// vite/client types, so the shape is declared here.
declare module '*?raw' {
  const content: string
  export default content
}

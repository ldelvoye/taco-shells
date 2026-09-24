// Anything a program prints can carry a link, so only schemes that open a
// browser or a mail client are let through to the OS.
const OPENABLE_PROTOCOLS = new Set(['http:', 'https:', 'mailto:'])

/** Whether a link clicked in a terminal may be handed to the OS to open. */
export function isOpenableLink(link: string): boolean {
  let url: URL
  try {
    url = new URL(link)
  } catch {
    return false
  }
  return OPENABLE_PROTOCOLS.has(url.protocol)
}

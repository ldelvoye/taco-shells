import { describe, expect, it } from 'vitest'

import { createApp, INSTALL_SCRIPT_URL } from '../src/app.ts'

describe('the site', () => {
  it("redirects /install.sh to the latest release's script", async () => {
    const response = await createApp().request('/install.sh')

    expect(response.status).toBe(302)
    expect(response.headers.get('location')).toBe(INSTALL_SCRIPT_URL)
  })

  it('serves the page at /', async () => {
    const response = await createApp().request('/')

    expect(response.status).toBe(200)
    expect(response.headers.get('content-type')).toMatch(/^text\/html/)
  })

  it('answers 404 for a path that is not a file of the page', async () => {
    const response = await createApp().request('/nope')

    expect(response.status).toBe(404)
  })
})

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

  it('serves nothing from outside public/', async () => {
    const response = await createApp().request('/package.json')

    expect(response.status).toBe(404)
  })
})

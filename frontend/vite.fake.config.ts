// The Vite config the browser tier runs against: the real application, with
// the fake API mounted as middleware in the same dev server.
//
// `mergeConfig` on the production config rather than a config of its own, so
// the alias, the plugins and the build settings stay defined once. Nothing
// test-only is loaded when the production bundle is built, because nothing
// imports this file except Playwright.
//
// One origin for the app and the API is the point. `apiClient` uses
// `baseURL: '/api'`, and serving the fake from the same dev server means that
// relative base is exercised exactly as written -- a separate fake server
// would need a proxy or CORS, and the tier would then be testing the proxy.

import { defineConfig, mergeConfig } from 'vite'

import viteConfig from './vite.config'
import { fakeApi } from './tests/fake/plugin'

export default mergeConfig(
  viteConfig,
  defineConfig({
    plugins: [fakeApi()],
    server: {
      port: 5199,
      strictPort: true,
      // No HMR. A reload triggered by the dev server mid-test is a source of
      // flakes that looks exactly like an application bug, and nothing here
      // edits a file while the browser is open.
      hmr: false,
    },
  })
)

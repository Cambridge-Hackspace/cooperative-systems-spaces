import { fileURLToPath } from 'node:url'
import { configDefaults, defineConfig, mergeConfig } from 'vitest/config'
import viteConfig from './vite.config.ts'

// Separate from vite.config.ts rather than a `test` key inside it, so the
// alias and plugin list stay defined once and nothing test-only is loaded when
// the production bundle is built.
export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      root: fileURLToPath(new URL('./', import.meta.url)),

      // jsdom, not happy-dom, and the reason is specific to this tier rather
      // than a general preference. happy-dom is roughly three times faster and
      // that would settle it if these were behavioural tests. They are not:
      // tier 2 asserts whole-string class attributes and exact element counts
      // on components dense with inline <svg> (ThemePicker.vue alone has
      // five), and happy-dom's SVG-namespace and innerHTML normalisation have
      // historically differed from browsers in exactly those places. Buying
      // speed with a chance of a false conformance pass is the wrong trade for
      // the tier whose entire value is exactness. If this gets slow the answer
      // is --shard, not a laxer DOM.
      environment: 'jsdom',

      // Every test imports describe/it/expect explicitly. The structural tests
      // are plain Node reading files with fs, and they should not depend on
      // ambient magic to run.
      globals: false,

      setupFiles: ['./tests/setup.ts'],
      include: ['tests/unit/**/*.spec.ts', 'tests/structure/**/*.spec.ts'],

      // Playwright owns tests/e2e. Vitest must not try to collect it: the two
      // runners have incompatible `test` globals and the failure is confusing.
      exclude: [...configDefaults.exclude, 'tests/e2e/**'],

      restoreMocks: true,
      unstubEnvs: true,
      unstubGlobals: true,

      reporters: process.env.CI ? ['default', 'junit'] : ['default'],
      outputFile: { junit: './test-results/vitest-junit.xml' },

      coverage: {
        provider: 'v8',
        include: ['src/**/*.{ts,vue}'],
        reportsDirectory: './coverage',
        reporter: ['text-summary', 'lcov'],
      },
    },
  })
)

import js from '@eslint/js'
import globals from 'globals'
import tseslint from 'typescript-eslint'
import pluginVue from 'eslint-plugin-vue'
import prettier from 'eslint-config-prettier'

// Flat config on ESLint 9, replacing a `lint` script that could never run: the
// package pinned eslint 8 and there was no `.eslintrc.*` anywhere in the repo,
// so `npm run lint` exited with "No ESLint configuration found". Its `--ext`
// and `--ignore-path` flags were also both removed in ESLint 9, so the script
// had to change regardless of which major we landed on.
export default tseslint.config(
  {
    // Flat config replaces .eslintignore.
    ignores: [
      'dist/**',
      'coverage/**',
      'node_modules/**',
      'test-results/**',
      'playwright-report/**',
    ],
  },

  js.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,
  ...pluginVue.configs['flat/recommended'],

  {
    // Type-aware linting, inside .vue single-file components as well.
    //
    // This is the whole reason for the upgrade rather than a cosmetic one:
    // type information is what buys `no-floating-promises`, and an async call
    // whose rejection nothing handles is precisely the shape of the two most
    // recent bug-fix commits on this branch — a door check-in that spun
    // forever and a rule addition that failed silently.
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals: { ...globals.browser },
      parserOptions: {
        parser: tseslint.parser,
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: ['.vue'],
      },
    },
  },

  {
    files: ['**/*.{ts,vue}'],
    rules: {
      // Errors, not warnings. A warning that never fails a build is a decision
      // the project quietly made, and §2 of the testing methodology forbids
      // the softer forms of exactly that.
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/no-misused-promises': 'error',
      '@typescript-eslint/await-thenable': 'error',
      '@typescript-eslint/require-await': 'error',
      'vue/no-unused-components': 'error',
      'vue/no-template-shadow': 'error',
    },
  },

  {
    // ---- The type-strictness ratchet ------------------------------------
    //
    // These six rules fire on `any`, and the base tsconfig has
    // `"strict": false`, so 24,000 lines written under it produce 585 of them
    // at once. Landing that as errors would force either an unreviewable diff
    // or a wave of `eslint-disable`, and the second is the weakening this
    // project's methodology forbids.
    //
    // So they are OFF here and ON for the paths in tsconfig.strict.json (see
    // the block below, which comes later and therefore wins). The two ratchets
    // move together, and GROWING THAT LIST IS THE UNIT OF WORK.
    //
    // This narrowing covers exactly the `any`-propagation family and nothing
    // else. Every other rule — including no-floating-promises, which is the
    // reason type-aware linting is here at all — stays on everywhere.
    files: ['**/*.{ts,vue}'],
    rules: {
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-call': 'off',
      '@typescript-eslint/no-unsafe-argument': 'off',
      '@typescript-eslint/no-unsafe-return': 'off',
      '@typescript-eslint/no-unsafe-enum-comparison': 'off',
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-redundant-type-constituents': 'off',
    },
  },

  {
    // The ratchet's current reach. Keep in step with tsconfig.strict.json's
    // `include`; a path in one and not the other is a gap that looks like
    // coverage.
    files: ['src/types/**/*.ts', 'src/components/schedule_templates.ts', 'tests/**/*.ts'],
    rules: {
      '@typescript-eslint/no-unsafe-member-access': 'error',
      '@typescript-eslint/no-unsafe-assignment': 'error',
      '@typescript-eslint/no-unsafe-call': 'error',
      '@typescript-eslint/no-unsafe-argument': 'error',
      '@typescript-eslint/no-unsafe-return': 'error',
      '@typescript-eslint/no-unsafe-enum-comparison': 'error',
      '@typescript-eslint/no-explicit-any': 'error',
    },
  },

  {
    // `App.vue` is the framework's own convention and cannot be renamed.
    // Scoped to that one file rather than switched off globally, so a genuinely
    // single-word component elsewhere is still reported.
    files: ['src/App.vue'],
    rules: { 'vue/multi-word-component-names': 'off' },
  },

  {
    // Tests read files and run under Node rather than a browser.
    files: ['tests/**/*.ts', '*.config.ts', 'vitest.config.ts'],
    languageOptions: { globals: { ...globals.node } },
  },

  {
    // Plain-JS config files are not part of the TypeScript project, so the
    // type-aware rules cannot analyse them and the project service reports a
    // parsing error instead. Disabled for these four files only; every .ts and
    // .vue file keeps full type-aware linting.
    files: ['*.js'],
    languageOptions: {
      globals: { ...globals.node },
      sourceType: 'commonjs',
      parserOptions: { projectService: false },
    },
    rules: {
      // Spread, not replaced: a bare `rules` key would overwrite the whole set
      // disableTypeChecked turns off, and every type-aware rule would then fire
      // on a file with no type information behind it.
      ...tseslint.configs.disableTypeChecked.rules,
      // These are CommonJS config files -- `module.exports` with `require` --
      // loaded by tailwind's and postcss's own resolvers. Converting them to
      // ESM changes how the build loads them, which is a build change rather
      // than a lint fix. Scoped to the four config files at this directory's
      // root; every other file keeps the rule.
      '@typescript-eslint/no-require-imports': 'off',
    },
  },

  {
    // Fixtures are deliberately untyped: several of them exist to represent
    // shapes the type system says cannot occur, which is the point of the test.
    // Scoped to fixtures rather than to all of tests/.
    files: ['tests/**/fixtures/**/*.ts'],
    rules: { '@typescript-eslint/no-unsafe-assignment': 'off' },
  },

  // Must be last: turns off every rule Prettier owns, so the two never argue
  // about the same character.
  prettier
)

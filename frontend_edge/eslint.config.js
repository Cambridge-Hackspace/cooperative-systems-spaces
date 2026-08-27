import js from '@eslint/js'
import globals from 'globals'
import tseslint from 'typescript-eslint'
import pluginVue from 'eslint-plugin-vue'
import prettier from 'eslint-config-prettier'

// The edge frontend's lint configuration.
//
// It exists because it did not, and CI called `npm run type-check` here for a
// script this package had never defined. `frontend/` was given eslint,
// prettier, a type-check and a strictness ratchet; this directory was given
// none of them while its CI job was written as though it had.
//
// Deliberately NOT a copy of frontend/eslint.config.js. That file carries a
// ratchet -- the `no-unsafe-*` family switched off globally and back on for a
// growing list of paths -- because 24,000 lines were written under
// `"strict": false` and enabling them at once produces 585 errors. None of that
// applies here: this is 565 lines across 8 files and tsconfig.json already sets
// `"strict": true`. So every rule is on, everywhere, and there is no list to
// grow. If this file ever needs a narrowing, that is a regression to argue
// about rather than a step to take quietly.
//
// Pinned to eslint 9 to match frontend/, and that is the only reason: 9.39.5 is
// the last 9.x and is out of support, with 10.x current. Moving both frontends
// to 10 is its own unit of work; moving one is two config dialects in one
// repository, which is worse than being a major behind in step.
export default tseslint.config(
  {
    // Flat config replaces .eslintignore.
    ignores: ['dist/**', 'node_modules/**'],
  },

  js.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,
  ...pluginVue.configs['flat/recommended'],

  {
    // Type-aware linting, inside .vue single-file components as well.
    //
    // The reason is the same one it was for frontend/: type information is what
    // buys `no-floating-promises`, and an async call whose rejection nothing
    // handles is the shape of this project's two most recent bug-fix commits.
    // This directory talks to a device over HTTP from a kiosk that nobody is
    // watching, so a promise that rejects into nothing is a screen that stops
    // updating with no indication why.
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
      // the project quietly made.
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/no-misused-promises': 'error',
      '@typescript-eslint/await-thenable': 'error',
      '@typescript-eslint/require-await': 'error',
      'vue/no-unused-components': 'error',
      'vue/no-template-shadow': 'error',
    },
  },

  {
    // `App.vue` is the framework's own convention and cannot be renamed.
    // Scoped to that one file, so a genuinely single-word component elsewhere
    // is still reported.
    files: ['src/App.vue'],
    rules: { 'vue/multi-word-component-names': 'off' },
  },

  {
    // tailwind.config.js and postcss.config.js are CommonJS, loaded by those
    // tools' own resolvers, and are not part of the TypeScript project -- so
    // the type-aware rules have no type information to work from and the
    // project service reports a parsing error instead.
    //
    // Spread rather than replaced: a bare `rules` key would overwrite the whole
    // set `disableTypeChecked` turns off, and every type-aware rule would then
    // fire on a file with nothing behind it.
    files: ['*.js'],
    languageOptions: {
      globals: { ...globals.node },
      sourceType: 'commonjs',
      parserOptions: { projectService: false },
    },
    rules: {
      ...tseslint.configs.disableTypeChecked.rules,
      '@typescript-eslint/no-require-imports': 'off',
    },
  },

  {
    // vite.config.ts runs under Node, not in a browser.
    files: ['vite.config.ts'],
    languageOptions: { globals: { ...globals.node } },
  },

  // Must be last: turns off every rule Prettier owns, so the two never argue
  // about the same character.
  prettier
)

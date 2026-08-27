declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  // `{}` here would mean "any non-nullish value", which admits `0` and `""`,
  // and `any` for the third parameter disables checking on every component
  // instance reached through a `*.vue` import.
  const component: DefineComponent<Record<string, unknown>, Record<string, unknown>, unknown>
  export default component
}

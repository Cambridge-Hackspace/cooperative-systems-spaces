/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{vue,js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        'css-primary': '#007bff',
        'css-secondary': '#6c757d',
        'css-accent': '#28a745',
      }
    },
  },
  plugins: [require('daisyui')],
  daisyui: {
    themes: [
      {
        'css-light': {
          'primary': '#007bff',
          'secondary': '#6c757d',
          'accent': '#28a745',
          'neutral': '#f8f9fa',
          'base-100': '#ffffff',
          'base-200': '#f8f9fa',
          'base-300': '#e9ecef',
          'info': '#17a2b8',
          'success': '#28a745',
          'warning': '#ffc107',
          'error': '#dc3545',
        },
        'css-dark': {
          'primary': '#0d6efd',
          'secondary': '#6c757d',
          'accent': '#198754',
          'neutral': '#212529',
          'base-100': '#1a1a1a',
          'base-200': '#2d2d2d',
          'base-300': '#404040',
          'info': '#0dcaf0',
          'success': '#198754',
          'warning': '#ffc107',
          'error': '#dc3545',
        },
      },
      'light',
      'dark',
      'cupcake',
      'corporate',
    ],
    darkTheme: 'css-dark',
    base: true,
    styled: true,
    utils: true,
    prefix: '',
    logs: true,
    themeRoot: ':root',
  },
}
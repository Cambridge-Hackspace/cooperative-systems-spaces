/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{vue,js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {},
  },
  plugins: [require("daisyui")],
  daisyui: {
    themes: [
      {
        afterdark: {
          "primary": "#7B79B5",
          "secondary": "#ACABD5",
          "accent": "#fef3c7",
          "neutral": "#38357F",
          "base-100": "#201D65",
          "info": "#7dd3fc",
          "success": "#a7f3d0",
          "warning": "#fef08a",
          "error": "#fca5a5",
        },
        her: {
          "primary": "#b57979",
          "secondary": "#d5abab",
          "accent": "#fef3c7",
          "neutral": "#7f3535",
          "base-100": "#651d1d",
          "info": "#7dd3fc",
          "success": "#a7f3d0",
          "warning": "#fef08a",
          "error": "#fca5a5",
        },
      },
        "light", "dark", "cupcake"],
  },
}

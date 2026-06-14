/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./crates/gitnodes-app/src/**/*.rs"],
  theme: {
    extend: {
      colors: {
        // Brand palette derived from the GitNodes mark: cyan (#00c0f0) primary
        // with a violet (#7020f0) secondary on dark navy neutrals. `brand` and
        // `accent` are the semantic tokens; `teal` is overridden to the brand
        // cyan so existing `teal-*` chrome adopts the brand globally without
        // touching every component.
        brand: {
          50: "#ecfdff",
          100: "#d2f6fd",
          200: "#a8ecfb",
          300: "#6cddf7",
          400: "#22c8f0",
          500: "#00b0e6",
          600: "#008cc2",
          700: "#0a6f9c",
          800: "#125b80",
          900: "#154c6a",
          950: "#093047",
        },
        accent: {
          50: "#f3eefe",
          100: "#e8defe",
          200: "#d3befd",
          300: "#b694fb",
          400: "#9866f8",
          500: "#7e3df2",
          600: "#7020f0",
          700: "#5d18c9",
          800: "#4d18a3",
          900: "#411882",
          950: "#280a59",
        },
        teal: {
          50: "#ecfdff",
          100: "#d2f6fd",
          200: "#a8ecfb",
          300: "#6cddf7",
          400: "#22c8f0",
          500: "#00b0e6",
          600: "#008cc2",
          700: "#0a6f9c",
          800: "#125b80",
          900: "#154c6a",
          950: "#093047",
        },
      },
      typography: ({ theme }) => ({
        invert: {
          css: {
            "--tw-prose-body": theme("colors.slate.300"),
            "--tw-prose-headings": theme("colors.slate.200"),
            "--tw-prose-lead": theme("colors.slate.300"),
            "--tw-prose-links": theme("colors.teal.300"),
            "--tw-prose-bold": theme("colors.slate.100"),
            "--tw-prose-counters": theme("colors.slate.400"),
            "--tw-prose-bullets": theme("colors.slate.600"),
            "--tw-prose-hr": theme("colors.slate.800"),
            "--tw-prose-quotes": theme("colors.slate.200"),
            "--tw-prose-quote-borders": theme("colors.teal.500"),
            "--tw-prose-captions": theme("colors.slate.400"),
            "--tw-prose-code": theme("colors.teal.200"),
            "--tw-prose-pre-code": theme("colors.slate.200"),
            "--tw-prose-pre-bg": theme("colors.slate.900"),
            "--tw-prose-th-borders": theme("colors.slate.700"),
            "--tw-prose-td-borders": theme("colors.slate.800"),
          },
        },
      }),
    },
  },
  plugins: [require("@tailwindcss/typography")],
};

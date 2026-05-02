/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./crates/brain-app/src/**/*.rs"],
  theme: {
    extend: {
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
  plugins: [require("@tailwindcss/typography"), require("daisyui")],
  daisyui: {
    themes: [
      {
        brain: {
          "color-scheme": "dark",
          primary: "#2dd4bf",
          "primary-content": "#042f2e",
          secondary: "#38bdf8",
          "secondary-content": "#082f49",
          accent: "#a78bfa",
          "accent-content": "#1e1b4b",
          neutral: "#1e293b",
          "neutral-content": "#e2e8f0",
          "base-100": "#0f172a",
          "base-200": "#0b1220",
          "base-300": "#020617",
          "base-content": "#e2e8f0",
          info: "#38bdf8",
          success: "#4ade80",
          warning: "#f59e0b",
          error: "#f87171",
          "--rounded-box": "0.75rem",
          "--rounded-btn": "0.5rem",
          "--rounded-badge": "0.375rem",
        },
      },
    ],
    darkTheme: "brain",
    base: false,
    styled: true,
    utils: true,
    logs: false,
  },
};

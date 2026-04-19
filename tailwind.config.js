/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./crates/brain-app/src/**/*.rs"],
  theme: {
    extend: {
      typography: ({ theme }) => ({
        invert: {
          css: {
            "--tw-prose-body": theme("colors.slate.300"),
            "--tw-prose-headings": theme("colors.slate.100"),
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

/** A theme colour that still takes an opacity, as in `border-line/40`. */
const mix = (name) => `color-mix(in srgb, var(--${name}) calc(<alpha-value> * 100%), transparent)`;

/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: ['class', '[data-theme="dark"]'],
  theme: {
    extend: {
      colors: {
        ground: mix('ground'),
        panel: mix('panel'),
        raised: mix('raised'),
        line: mix('line'),
        edge: mix('edge'),
        pick: mix('pick'),
        ink: mix('ink'),
        muted: mix('muted'),
        accent: mix('accent'),
        'on-accent': mix('on-accent'),
        safe: mix('safe'),
        change: mix('change'),
        danger: mix('danger'),
        'on-safe': mix('on-safe'),
        'on-change': mix('on-change'),
        'on-danger': mix('on-danger'),
      },
      fontFamily: {
        sans: ['"Atkinson Hyperlegible"', 'system-ui', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'monospace'],
      },
      boxShadow: { pop: 'var(--shadow)' },
      fontSize: { xs: ['12px', '16px'], sm: ['13px', '18px'], base: ['14px', '20px'], lg: ['16px', '22px'], xl: ['20px', '26px'] },
    },
  },
  plugins: [],
};

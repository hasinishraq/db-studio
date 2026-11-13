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
        hover: mix('hover'),
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
        sans: ['var(--font-ui)'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'monospace'],
      },
      boxShadow: { pop: 'var(--shadow)' },
      // Desktop density: 13px is the working size, as in most native database tools.
      fontSize: { '2xs': ['11px', '14px'], xs: ['12px', '16px'], sm: ['13px', '18px'], base: ['13px', '20px'], lg: ['15px', '22px'], xl: ['18px', '24px'] },
      borderRadius: { DEFAULT: '4px', md: '6px', lg: '8px' },
      keyframes: {
        'pop-in': { from: { opacity: '0', transform: 'scale(0.97)' }, to: { opacity: '1', transform: 'scale(1)' } },
        'fade-in': { from: { opacity: '0' }, to: { opacity: '1' } },
      },
      animation: { 'pop-in': 'pop-in 120ms ease-out', 'fade-in': 'fade-in 120ms ease-out' },
    },
  },
  plugins: [],
};

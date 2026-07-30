import type { Config } from 'tailwindcss';
import tailwindcssAnimate from 'tailwindcss-animate';

export default {
  darkMode: ['class'],
  content: ['./src/**/*.{html,js,svelte,ts}'],
  theme: {
    container: {
      center: true,
      padding: '2rem',
      screens: { '2xl': '1400px' },
    },
    extend: {
      colors: {
        border: 'hsl(var(--border-hsl) / <alpha-value>)',
        input: 'hsl(var(--input-hsl) / <alpha-value>)',
        ring: 'hsl(var(--ring-hsl) / <alpha-value>)',
        background: 'hsl(var(--background-hsl) / <alpha-value>)',
        foreground: 'hsl(var(--foreground-hsl) / <alpha-value>)',
        primary: {
          DEFAULT: 'hsl(var(--primary-hsl) / <alpha-value>)',
          foreground: 'hsl(var(--primary-foreground-hsl) / <alpha-value>)',
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary-hsl) / <alpha-value>)',
          foreground: 'hsl(var(--secondary-foreground-hsl) / <alpha-value>)',
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive-hsl) / <alpha-value>)',
          foreground: 'hsl(var(--destructive-foreground-hsl) / <alpha-value>)',
        },
        muted: {
          DEFAULT: 'hsl(var(--muted-hsl) / <alpha-value>)',
          foreground: 'hsl(var(--muted-foreground-hsl) / <alpha-value>)',
        },
        accent: {
          DEFAULT: 'hsl(var(--accent-hsl) / <alpha-value>)',
          foreground: 'hsl(var(--accent-foreground-hsl) / <alpha-value>)',
        },
        popover: {
          DEFAULT: 'hsl(var(--popover-hsl) / <alpha-value>)',
          foreground: 'hsl(var(--popover-foreground-hsl) / <alpha-value>)',
        },
        card: {
          DEFAULT: 'hsl(var(--card-hsl) / <alpha-value>)',
          foreground: 'hsl(var(--card-foreground-hsl) / <alpha-value>)',
        },
        // 串口工具专用颜色
        rx: 'hsl(var(--rx-hsl) / <alpha-value>)',
        tx: 'hsl(var(--tx-hsl) / <alpha-value>)',
        system: 'hsl(var(--system-hsl) / <alpha-value>)',
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
      },
      fontFamily: {
        mono: ['"SF Mono"', '"JetBrains Mono"', '"Cascadia Code"', 'Consolas', 'monospace'],
        serif: ['"Iowan Old Style"', '"SF Pro Display"', 'Georgia', 'serif'],
      },
    },
  },
  plugins: [tailwindcssAnimate],
} satisfies Config;

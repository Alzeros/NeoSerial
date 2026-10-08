import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { defaultCustomTheme, normalizeCustomTheme } from '../src/lib/customTheme.ts';

const css = readFileSync('src/app.css', 'utf8');
const theme = (selector: string) => {
  const start = css.indexOf(`${selector} {`);
  assert.notEqual(start, -1);
  const block = css.slice(start, css.indexOf('}', start));
  return (name: string) => {
    const value = block.match(new RegExp(`--${name}: (#[\\da-fA-F]{6});`))?.[1];
    assert.ok(value, `${selector}: missing ${name}`);
    return value;
  };
};
function luminance(hex: string) {
  const [r, g, b] = hex.slice(1).match(/../g)!.map(v => Number.parseInt(v, 16) / 255)
    .map(v => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return r * 0.2126 + g * 0.7152 + b * 0.0722;
}
function contrast(a: string, b: string) {
  const values = [luminance(a), luminance(b)].sort((a, b) => a - b);
  return (values[1] + 0.05) / (values[0] + 0.05);
}

for (const selector of [':root', ...[2, 3, 4].map(n => `html[data-theme="preset-${n}"]`)]) {
  test(`${selector}: secondary and muted text remain readable on preset surfaces`, () => {
    const color = theme(selector);
    for (const foreground of ['foreground-secondary', 'muted-foreground']) {
      for (const background of ['background', 'background-elevated', 'background-data', 'background-input', 'background-deep']) {
        assert.ok(contrast(color(foreground), color(background)) >= 4.5, `${foreground} on ${background}`);
      }
    }
  });
}

test('new custom themes use the readable default palette without rewriting saved user colors', () => {
  const color = theme(':root');
  const defaults = defaultCustomTheme();
  for (const key of ['foreground-secondary', 'muted-foreground']) assert.equal(defaults[key], color(key));
  assert.equal(normalizeCustomTheme({ 'muted-foreground': '#8A8676' })['muted-foreground'], '#8A8676');
});

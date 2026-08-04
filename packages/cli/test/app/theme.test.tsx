/**
 * @vitest-environment happy-dom
 */
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ThemeProvider, ThemeToggle } from '../../app/src/theme.js';

const show = () =>
  render(
    <ThemeProvider>
      <ThemeToggle />
    </ThemeProvider>,
  );

beforeEach(() => {
  localStorage.clear();
  document.documentElement.className = '';
});

afterEach(cleanup);

describe('the two modes', () => {
  it('starts on ink', () => {
    // Both modes are real in this brand — the product's output is a printed
    // sheet — but the tool sits open beside an editor, and a page only reads
    // as a page when what surrounds it is not one.
    show();

    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('remembers the mode that was chosen', () => {
    localStorage.setItem('imprenta.theme', 'paper');

    show();

    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });

  it('follows the system when asked to', () => {
    // happy-dom answers `prefers-color-scheme: dark` with false, so system
    // here means paper.
    localStorage.setItem('imprenta.theme', 'system');

    show();

    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });

  it('offers the choice by a name somebody can say out loud', () => {
    show();

    expect(screen.getByRole('button', { name: /theme/i })).toBeTruthy();
  });

  it('ignores a stored value that is not a mode', () => {
    // Somebody's devtools, or a version of this app that had other names.
    localStorage.setItem('imprenta.theme', 'chartreuse');

    show();

    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });
});

/**
 * @vitest-environment happy-dom
 *
 * Declared here rather than for the whole package: the server tests need a
 * real Node, and only these need a document to render into.
 */
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Checks } from '../../app/src/Checks.js';
import type { Finding } from '../../app/src/types.js';

const finding = (over: Partial<Finding> = {}): Finding => ({
  rule: 'faint-text',
  status: 'warning',
  source: 'document',
  detail: 'too pale',
  occurrences: 1,
  ...over,
});

const counts = (checks: Finding[]) => ({
  errors: checks.filter((c) => c.status === 'error').length,
  warnings: checks.filter((c) => c.status === 'warning').length,
});

const show = (
  checks: Finding[],
  over: { open?: boolean; disabled?: boolean; format?: 'pdf' | 'xlsx' } = {},
) =>
  render(
    <Checks
      checks={checks}
      counts={counts(checks)}
      format={over.format ?? 'pdf'}
      open={over.open ?? true}
      disabled={over.disabled ?? false}
      onOpenChange={() => {}}
    />,
  );

// Without globals, testing-library does not register its own teardown, and
// one render's markup would answer the next one's questions.
afterEach(cleanup);

describe('the checks panel', () => {
  it('says the document is ready when nothing is wrong', () => {
    // The state an author should see most of the time, and the one worth
    // being unambiguous about: an empty table says nothing.
    show([]);

    expect(screen.getByText(/ready to print/i)).toBeTruthy();
    expect(screen.getByText(/nothing to fix/i)).toBeTruthy();
  });

  it('counts the errors and the warnings apart, in words', () => {
    // A red 1 beside an amber 2 is legible to somebody looking at it and
    // silent to somebody listening, so the badges carry the noun.
    //
    // Three distinct findings, because the panel is handed a list that has
    // already been collapsed by rule and signature — two identical rows are a
    // state it never sees, and inventing one here only breaks React's keys.
    show([
      finding({ status: 'error', rule: 'tiny-text' }),
      finding({ rule: 'faint-text' }),
      finding({ rule: 'unprintable-margin', detail: 'inside the five millimetres' }),
    ]);

    expect(screen.queryByText(/ready to print/i)).toBeNull();
    expect(screen.getByLabelText('1 error')).toBeTruthy();
    expect(screen.getByLabelText('2 warnings')).toBeTruthy();
  });

  it('shows a count only for the kind that occurred', () => {
    show([finding({ status: 'error' })]);

    expect(screen.getByLabelText('1 error')).toBeTruthy();
    expect(screen.queryByLabelText(/warning/)).toBeNull();
  });

  it('names the rule and explains it', () => {
    show([finding({ rule: 'tiny-text', detail: 'set at 4pt, below the 6pt where print stops' })]);

    expect(screen.getByText('tiny-text')).toBeTruthy();
    expect(screen.getByText(/below the 6pt/)).toBeTruthy();
  });

  it('says how many places a fault was found in, when it was more than one', () => {
    show([finding({ occurrences: 3 })]);

    expect(screen.getByText('×3')).toBeTruthy();
  });

  it('does not clutter a single occurrence with a count of one', () => {
    show([finding({ occurrences: 1 })]);

    expect(screen.queryByText('×1')).toBeNull();
  });

  it('says where a finding came from', () => {
    // The engine noticing something and a rule noticing it are different
    // kinds of fact, and an author fixes them in different places.
    show([finding({ source: 'engine', rule: 'missing-glyph' })]);

    expect(screen.getByText('engine')).toBeTruthy();
  });

  it('says which severity a row is, rather than only colouring it', () => {
    // Two shapes and two colours down the left edge, which is fast to read and
    // useless to anybody who cannot see it. The icon carries the word.
    show([finding({ status: 'error', rule: 'tiny-text' }), finding({ rule: 'faint-text' })]);

    expect(screen.getByLabelText('error')).toBeTruthy();
    expect(screen.getByLabelText('warning')).toBeTruthy();
  });

  it('shows nothing but the bar when it is closed', () => {
    show([finding()], { open: false });

    expect(screen.queryByText('too pale')).toBeNull();
    expect(screen.getByText('Checks')).toBeTruthy();
  });

  it('says the checks did not run when the document did not render', () => {
    // Claiming a broken document is ready to print would be a lie.
    show([], { disabled: true });

    expect(screen.getByText(/not run/i)).toBeTruthy();
    expect(screen.queryByText(/ready to print/i)).toBeNull();
  });

  it('opens and closes when the bar is used', () => {
    const onOpenChange = vi.fn();
    render(
      <Checks
        checks={[]}
        counts={{ errors: 0, warnings: 0 }}
        format="pdf"
        open
        onOpenChange={onOpenChange}
        disabled={false}
      />,
    );

    screen.getByRole('button', { expanded: true }).click();

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('reassures the author about the medium they are actually in', () => {
    // Telling somebody writing a spreadsheet that the margins are fine is how
    // a panel stops being read.
    cleanup();
    show([], { format: 'xlsx' });
    expect(screen.getByText(/Every number is a number/)).toBeTruthy();
    expect(screen.queryByText(/printer/)).toBeNull();

    cleanup();
    show([], { format: 'pdf' });
    expect(screen.getByText(/printer can reach/)).toBeTruthy();
  });
});

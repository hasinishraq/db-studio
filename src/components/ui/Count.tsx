import NumberFlow from '@number-flow/react';

/** A number that rolls to its new value, for row counts and timings in status lines.
 *  NumberFlow honours prefers-reduced-motion on its own. */
export function Count({ value, suffix, className }: { value: number; suffix?: string; className?: string }) {
  return (
    <NumberFlow
      value={value}
      suffix={suffix}
      className={className}
      style={{ fontVariantNumeric: 'tabular-nums' }}
      willChange
    />
  );
}

import * as Dialog from '@radix-ui/react-dialog';
import { cva, type VariantProps } from 'class-variance-authority';
import { CircleAlert, Info, TriangleAlert, X } from 'lucide-react';
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from 'react';
import { cn } from '../../lib/utils';

export { cn };

/** Buttons follow desktop-tool conventions: 26px tall, a quiet default, one filled primary.
 *  `quiet` is for toolbars. `change` and `danger` are kept for actions that write. */
const button = cva(
  'inline-flex h-[26px] shrink-0 select-none items-center justify-center gap-1.5 whitespace-nowrap rounded border px-2.5 text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-40 [&_svg]:shrink-0',
  {
    variants: {
      tone: {
        plain: 'border-edge/70 bg-raised text-ink shadow-[0_1px_0_rgb(0_0_0/0.06)] hover:border-edge hover:bg-hover',
        quiet: 'border-transparent bg-transparent text-muted hover:bg-hover hover:text-ink',
        primary: 'border-transparent bg-accent text-on-accent hover:brightness-110 active:brightness-95',
        change: 'border-transparent bg-change text-on-change hover:brightness-110 active:brightness-95',
        danger: 'border-transparent bg-danger text-on-danger hover:brightness-110 active:brightness-95',
      },
    },
    defaultVariants: { tone: 'plain' },
  },
);

type Tone = NonNullable<VariantProps<typeof button>['tone']>;

export function Button({
  tone = 'plain',
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { tone?: Tone }) {
  return <button type="button" className={cn(button({ tone }), className)} {...props} />;
}

const FIELD =
  'h-[26px] w-full rounded border border-edge/70 bg-ground px-2 text-sm text-ink placeholder:text-muted/80 transition-colors hover:border-edge focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/25 disabled:opacity-40';

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cn(FIELD, className)} {...props} />;
}

export function Select({ className, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select className={cn(FIELD, 'pr-6', className)} {...props} />;
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block text-sm">
      <span className="mb-1 block text-xs font-medium text-muted">{label}</span>
      {children}
    </label>
  );
}

/** A keyboard shortcut, drawn as a key cap. */
export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        'inline-flex h-[18px] min-w-[18px] items-center justify-center rounded border border-line bg-ground px-1 font-sans text-2xs text-muted',
        className,
      )}
    >
      {children}
    </kbd>
  );
}

/** A segmented control built from radio buttons, so arrow keys and screen readers work as usual. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (value: T) => void;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="inline-flex rounded border border-edge/70 bg-ground p-0.5">
      {options.map((option) => (
        <label
          key={option.value}
          className={cn(
            'flex h-[22px] cursor-pointer items-center gap-1.5 rounded-[3px] px-2.5 text-xs has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-accent',
            value === option.value ? 'bg-raised text-ink shadow-[0_0_0_1px_var(--line)]' : 'text-muted hover:text-ink',
          )}
        >
          <input
            type="radio"
            className="sr-only"
            name={label}
            value={option.value}
            checked={value === option.value}
            onChange={() => onChange(option.value)}
          />
          {option.label}
        </label>
      ))}
    </div>
  );
}

/** An on/off switch. It is a button with role="switch", so a <label htmlFor> names it. */
export function Switch({
  checked,
  onChange,
  id,
  disabled,
  ...rest
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  id?: string;
  disabled?: boolean;
  'aria-label'?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border transition-colors disabled:opacity-40',
        checked ? 'border-accent bg-accent' : 'border-edge/70 bg-ground',
      )}
      {...rest}
    >
      <span
        aria-hidden
        className={cn(
          'absolute h-3 w-3 rounded-full shadow-sm transition-transform',
          checked ? 'translate-x-[15px] bg-on-accent' : 'translate-x-[2px] bg-muted',
        )}
      />
    </button>
  );
}

/** The dimmed backdrop behind every popup. */
export const OVERLAY = 'fixed inset-0 z-40 bg-black/50 animate-fade-in';

/** The one look every popup shares, so a review and a form read as the same app. */
export const SURFACE =
  'fixed left-1/2 top-1/2 z-50 max-h-[85vh] w-[92vw] -translate-x-1/2 -translate-y-1/2 overflow-auto rounded-lg border border-line bg-panel p-5 shadow-pop focus:outline-none';

export function Modal({
  title,
  onClose,
  children,
  wide,
  footer,
  className,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
  /** Buttons pinned under the body, so they stay in reach when the body scrolls. */
  footer?: ReactNode;
  /** Overrides the width, for dialogs between the two standard sizes. */
  className?: string;
}) {
  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className={OVERLAY} />
        <Dialog.Content
          aria-describedby={undefined}
          className={cn(SURFACE, 'flex flex-col overflow-hidden p-0', wide ? 'max-w-4xl' : 'max-w-lg', className)}
        >
          <div className="flex h-10 shrink-0 items-center justify-between border-b border-line bg-panel pl-5 pr-2">
            <Dialog.Title className="text-sm font-semibold">{title}</Dialog.Title>
            <Dialog.Close asChild>
              <button
                type="button"
                aria-label="Close"
                className="rounded p-1 text-muted hover:bg-hover hover:text-ink"
              >
                <X size={15} />
              </button>
            </Dialog.Close>
          </div>
          <div className="min-h-0 flex-1 overflow-auto p-5">{children}</div>
          {footer && (
            <div className="flex shrink-0 items-center gap-2 border-t border-line bg-panel px-5 py-3">{footer}</div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** What a callout is about: plain information, something to check, or real danger. */
export type CalloutTone = 'info' | 'change' | 'danger';

const CALLOUT: Record<CalloutTone, { box: string; icon: string; Icon: typeof Info }> = {
  info: { box: 'border-line bg-ground', icon: 'text-accent', Icon: Info },
  change: { box: 'border-change/35 bg-change/10', icon: 'text-change', Icon: TriangleAlert },
  danger: { box: 'border-danger/35 bg-danger/10', icon: 'text-danger', Icon: CircleAlert },
};

/** A boxed note with an icon. The tone shows in the icon and a faint tint, never a side bar. */
export function Callout({
  tone = 'info',
  title,
  children,
  className,
  role,
}: {
  tone?: CalloutTone;
  title?: ReactNode;
  children?: ReactNode;
  className?: string;
  role?: 'alert' | 'status';
}) {
  const { box, icon, Icon } = CALLOUT[tone];
  return (
    <div role={role} className={cn('flex gap-2 rounded-md border px-3 py-2 text-sm', box, className)}>
      <Icon size={15} aria-hidden className={cn('mt-0.5 shrink-0', icon)} />
      <div className="min-w-0 flex-1">
        {title && <p className="font-semibold">{title}</p>}
        {children}
      </div>
    </div>
  );
}

export function ErrorNote({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <Callout tone="danger" role="alert">
      {message}
    </Callout>
  );
}

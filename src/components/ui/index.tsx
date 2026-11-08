import * as Dialog from '@radix-ui/react-dialog';
import { clsx } from 'clsx';
import { CircleAlert, Info, TriangleAlert } from 'lucide-react';
import { twMerge } from 'tailwind-merge';
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from 'react';

type Tone = 'plain' | 'quiet' | 'primary' | 'change' | 'danger';

/** Every button has an edge, so it never reads as a label. `quiet` is for toolbar extras.
 *  `primary` is the accent; `change` and `danger` are kept for actions that write. */
const TONES: Record<Tone, string> = {
  plain: 'border-edge bg-raised text-ink hover:bg-pick',
  quiet: 'border-transparent bg-transparent text-muted hover:border-edge hover:bg-raised hover:text-ink',
  primary: 'border-transparent bg-accent font-bold text-on-accent hover:brightness-110',
  change: 'border-transparent bg-change font-bold text-on-change hover:brightness-110',
  danger: 'border-transparent bg-danger font-bold text-on-danger hover:brightness-110',
};

export function Button({
  tone = 'plain',
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { tone?: Tone }) {
  return (
    <button
      type="button"
      className={twMerge(
        'inline-flex items-center gap-1.5 rounded border px-2.5 py-1 text-sm disabled:cursor-not-allowed disabled:border-line disabled:opacity-40 disabled:hover:bg-raised',
        TONES[tone],
        className,
      )}
      {...props}
    />
  );
}

const FIELD =
  'w-full rounded border border-edge bg-ground px-2 py-1 text-sm text-ink placeholder:text-muted disabled:opacity-40';

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={twMerge(FIELD, className)} {...props} />;
}

export function Select({ className, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select className={twMerge(FIELD, className)} {...props} />;
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block text-sm">
      <span className="mb-1 block text-muted">{label}</span>
      {children}
    </label>
  );
}

/** The dimmed backdrop behind every popup. */
export const OVERLAY = 'fixed inset-0 z-40 bg-black/55';

/** The one look every popup shares, so a review and a form read as the same app. */
export const SURFACE =
  'fixed left-1/2 top-1/2 z-50 max-h-[85vh] w-[92vw] -translate-x-1/2 -translate-y-1/2 overflow-auto rounded-lg border border-line bg-panel p-5 shadow-pop';

export function Modal({
  title,
  onClose,
  children,
  wide,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className={OVERLAY} />
        <Dialog.Content aria-describedby={undefined} className={clsx(SURFACE, wide ? 'max-w-4xl' : 'max-w-lg')}>
          <Dialog.Title className="mb-3 text-lg font-bold">{title}</Dialog.Title>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** What a callout is about: plain information, something to check, or real danger. */
export type CalloutTone = 'info' | 'change' | 'danger';

const CALLOUT: Record<CalloutTone, { box: string; icon: string; Icon: typeof Info }> = {
  info: { box: 'border-line bg-ground', icon: 'text-accent', Icon: Info },
  change: { box: 'border-change/40 bg-change/10', icon: 'text-change', Icon: TriangleAlert },
  danger: { box: 'border-danger/40 bg-danger/10', icon: 'text-danger', Icon: CircleAlert },
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
    <div role={role} className={twMerge('flex gap-2 rounded-md border px-2.5 py-2 text-sm', box, className)}>
      <Icon size={16} aria-hidden className={clsx('mt-0.5 shrink-0', icon)} />
      <div className="min-w-0 flex-1">
        {title && <p className="font-bold">{title}</p>}
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

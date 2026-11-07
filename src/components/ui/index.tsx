import * as Dialog from '@radix-ui/react-dialog';
import { clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from 'react';

type Tone = 'plain' | 'quiet' | 'primary' | 'change' | 'danger';

/** Every button has an edge, so it never reads as a label. `quiet` is for toolbar extras. */
const TONES: Record<Tone, string> = {
  plain: 'border-edge bg-raised text-ink hover:bg-pick',
  quiet: 'border-transparent bg-transparent text-muted hover:border-edge hover:bg-raised hover:text-ink',
  primary: 'border-transparent bg-safe font-bold text-on-safe hover:brightness-110',
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
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content
          aria-describedby={undefined}
          className={clsx(
            'fixed left-1/2 top-1/2 z-50 max-h-[85vh] w-[92vw] -translate-x-1/2 -translate-y-1/2 overflow-auto rounded-md border border-line bg-panel p-4',
            wide ? 'max-w-4xl' : 'max-w-lg',
          )}
        >
          <Dialog.Title className="mb-3 text-lg font-bold">{title}</Dialog.Title>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

export function ErrorNote({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <p role="alert" className="border-l-4 border-danger bg-danger/10 px-2 py-1.5 text-sm">
      {message}
    </p>
  );
}

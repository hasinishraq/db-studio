import { Toaster as Sonner, toast } from 'sonner';
import { CircleAlert, CircleCheck, Info, TriangleAlert } from 'lucide-react';

/** Short-lived notices: lost connections, keychain warnings, finished exports.
 *  Never put SQL parameters, row values or secrets in one. The run log stays the record. */
export const notify = {
  info: (message: string) => toast(message, { icon: <Info size={15} className="text-accent" /> }),
  success: (message: string) => toast.success(message),
  warning: (message: string) => toast.warning(message, { duration: 8000 }),
  /** Errors stay until dismissed, so nobody misses one. */
  error: (message: string) => toast.error(message, { duration: Infinity }),
};

/** The one place toasts appear: bottom right, above the status bar, in the app's colours. */
export function Toaster() {
  return (
    <Sonner
      position="bottom-right"
      offset={{ bottom: 36, right: 16 }}
      gap={8}
      closeButton
      visibleToasts={4}
      icons={{
        success: <CircleCheck size={15} className="text-safe" />,
        warning: <TriangleAlert size={15} className="text-change" />,
        error: <CircleAlert size={15} className="text-danger" />,
      }}
      toastOptions={{
        unstyled: true,
        classNames: {
          toast:
            'group pointer-events-auto flex w-[356px] items-start gap-2.5 rounded-md border border-line bg-raised px-3 py-2.5 text-sm text-ink shadow-pop',
          icon: 'mt-0.5 shrink-0',
          content: 'min-w-0 flex-1',
          title: 'leading-snug',
          description: 'mt-0.5 text-xs text-muted',
          closeButton:
            '!left-auto !right-1.5 !top-1.5 !translate-x-0 !translate-y-0 !border-0 !bg-transparent text-muted hover:!bg-hover hover:text-ink',
        },
      }}
    />
  );
}

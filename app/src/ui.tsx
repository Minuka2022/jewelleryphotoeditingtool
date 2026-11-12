// Small building blocks shared by the whole interface.

import { useEffect } from 'react';
import type { ButtonHTMLAttributes, ReactNode, SelectHTMLAttributes } from 'react';

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(' ');
}

type ButtonVariant = 'default' | 'primary' | 'danger';

const buttonStyles: Record<ButtonVariant, string> = {
  default: 'bg-raised border-edge hover:bg-[#444] text-ink',
  primary: 'bg-accent border-accent hover:brightness-110 text-white font-semibold',
  danger: 'bg-bad/90 border-bad hover:brightness-110 text-white font-semibold',
};

export function Button({
  variant = 'default',
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant }) {
  return (
    <button
      type="button"
      {...props}
      className={cx(
        'h-7 shrink-0 rounded border px-3 text-xs whitespace-nowrap disabled:cursor-not-allowed disabled:opacity-45',
        buttonStyles[variant],
        className,
      )}
    />
  );
}

export function Select({ className, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={cx(
        'h-6 rounded border border-edge bg-field px-1 text-xs text-ink outline-none focus:border-accent disabled:opacity-45',
        className,
      )}
    />
  );
}

export const inputClass =
  'w-full rounded border border-edge bg-field px-2 py-1 text-xs text-ink outline-none placeholder:text-dim/70 focus:border-accent';

/** A label above (or beside) a control. */
export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1 block text-[11px] text-dim">{label}</span>
      {children}
      {hint && <span className="mt-1 block text-[11px] leading-snug text-dim">{hint}</span>}
    </label>
  );
}

export function Modal({
  title,
  onClose,
  children,
  footer,
  width = 'w-[460px]',
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: string;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/55"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-label={title}
        className={cx('flex max-h-[88vh] flex-col rounded-md border border-edge bg-panel shadow-2xl', width)}
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-2.5">
          <h2 className="text-[13px] font-semibold">{title}</h2>
          <button type="button" aria-label="Close" onClick={onClose} className="text-dim hover:text-ink">
            <Icon name="cross" />
          </button>
        </div>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 py-3">{children}</div>
        {footer && <div className="flex justify-end gap-2 border-t border-line px-4 py-2.5">{footer}</div>}
      </div>
    </div>
  );
}

const icons = {
  hand: 'M6 11V5.5a1.5 1.5 0 013 0V10m0-5.5v-1a1.5 1.5 0 013 0V10m0-5a1.5 1.5 0 013 0v6m0-3a1.5 1.5 0 013 0v5.5a6.5 6.5 0 01-6.5 6.5H11a6 6 0 01-5-2.7L3.3 13a1.6 1.6 0 012.6-1.8L6 11',
  zoom: 'M10.5 17a6.5 6.5 0 100-13 6.5 6.5 0 000 13zm4.6-1.9L20 20M8 10.5h5M10.5 8v5',
  single: 'M4 5h16v14H4z',
  split: 'M4 5h16v14H4zM12 5v14',
  slider: 'M12 3v18M9 9l-3 3 3 3M15 9l3 3-3 3',
  fit: 'M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  cross: 'M6 6l12 12M18 6L6 18',
  trash: 'M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13',
  plus: 'M12 5v14M5 12h14',
  folder: 'M3 7a1 1 0 011-1h5l2 2h8a1 1 0 011 1v9a1 1 0 01-1 1H4a1 1 0 01-1-1V7z',
} as const;

export type IconName = keyof typeof icons;

export function Icon({ name, size = 16 }: { name: IconName; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={icons[name]} />
    </svg>
  );
}

export function Spinner({ size = 14 }: { size?: number }) {
  return (
    <span
      className="inline-block animate-spin rounded-full border-2 border-edge border-t-accent"
      style={{ width: size, height: size }}
      aria-hidden="true"
    />
  );
}

// Design system primitives: buttons, inputs, cards, badges, dialogs, drawers.
import { useEffect, useId, useRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode,
  type SelectHTMLAttributes } from 'react';
import { createPortal } from 'react-dom';

import { ApiError } from '../api';
import { IconAlert, IconInfo, IconX } from './icons';

export const cx = (...parts: (string | false | null | undefined)[]) => parts.filter(Boolean).join(' ');

// -- buttons ----------------------------------------------------------------

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'subtle';
type Size = 'sm' | 'md';

const VARIANTS: Record<Variant, string> = {
  primary:
    'bg-gradient-to-b from-brand-500 to-brand-600 text-white shadow-sm shadow-brand-600/30 ring-1 ring-inset ring-white/10 ' +
    'hover:from-brand-400 hover:to-brand-600 active:to-brand-700',
  secondary:
    'bg-white text-zinc-800 shadow-xs ring-1 ring-inset ring-zinc-200 hover:bg-zinc-50 hover:ring-zinc-300 ' +
    'dark:bg-zinc-900 dark:text-zinc-100 dark:ring-zinc-800 dark:hover:bg-zinc-800 dark:hover:ring-zinc-700',
  ghost: 'text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900 dark:text-zinc-400 dark:hover:bg-zinc-800/70 dark:hover:text-zinc-100',
  subtle:
    'bg-brand-50 text-brand-700 hover:bg-brand-100 dark:bg-brand-500/10 dark:text-brand-300 dark:hover:bg-brand-500/20',
  danger:
    'bg-white text-red-600 shadow-xs ring-1 ring-inset ring-red-200 hover:bg-red-50 ' +
    'dark:bg-zinc-900 dark:text-red-400 dark:ring-red-900/60 dark:hover:bg-red-950/40',
};
const SIZES: Record<Size, string> = {
  sm: 'h-8 gap-1.5 rounded-lg px-2.5 text-xs',
  md: 'h-9 gap-2 rounded-lg px-3.5 text-sm',
};

export function Button({ variant = 'secondary', size = 'md', icon, className, children, ...props }:
  ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: Size; icon?: ReactNode }) {
  return (
    <button type="button" {...props}
            className={cx('inline-flex shrink-0 items-center justify-center font-medium whitespace-nowrap transition',
              'disabled:pointer-events-none disabled:opacity-50', VARIANTS[variant], SIZES[size], className)}>
      {icon}
      {children}
    </button>
  );
}

export function IconButton({ label, className, children, ...props }:
  ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button type="button" aria-label={label} title={label} {...props}
            className={cx('inline-flex size-8 shrink-0 items-center justify-center rounded-lg text-zinc-500 transition',
              'hover:bg-zinc-100 hover:text-zinc-900 disabled:opacity-50 dark:hover:bg-zinc-800 dark:hover:text-zinc-100',
              className)}>
      {children}
    </button>
  );
}

// -- form controls ----------------------------------------------------------

export const controlClass =
  'h-9 rounded-lg border-0 bg-white px-3 text-sm text-zinc-900 shadow-xs ring-1 ring-inset ring-zinc-200 ' +
  'placeholder:text-zinc-400 transition focus:ring-2 focus:ring-brand-500 focus:outline-none ' +
  'dark:bg-zinc-900 dark:text-zinc-100 dark:ring-zinc-800 dark:placeholder:text-zinc-500 dark:focus:ring-brand-400';

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={cx(controlClass, !/(^|\s)w-/.test(className ?? '') && 'w-full', className)} />;
}

export function Select({ className, children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select {...props} className={cx(controlClass, 'w-full appearance-none bg-[length:16px] bg-[right_0.6rem_center] bg-no-repeat pr-8',
      "bg-[url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%2371717a' stroke-width='2'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E\")]",
      className)}>
      {children}
    </select>
  );
}

export function Field({ label, help, children, className }: {
  label: string; help?: ReactNode; children: (id: string) => ReactNode; className?: string;
}) {
  const id = useId();
  return (
    <div className={className}>
      <label htmlFor={id} className="mb-1.5 block text-xs font-medium text-zinc-600 dark:text-zinc-400">{label}</label>
      {children(id)}
      {help && <p className="mt-1.5 text-xs text-zinc-500">{help}</p>}
    </div>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={checked} onClick={() => onChange(!checked)}
            className="inline-flex items-center gap-2 text-sm text-zinc-700 dark:text-zinc-300">
      <span className={cx('relative inline-flex h-5 w-9 items-center rounded-full transition',
        checked ? 'bg-brand-500' : 'bg-zinc-300 dark:bg-zinc-700')}>
        <span className={cx('inline-block size-4 rounded-full bg-white shadow transition', checked ? 'translate-x-4.5' : 'translate-x-0.5')} />
      </span>
      {label}
    </button>
  );
}

export function Segmented<T extends string>({ value, options, onChange }: {
  value: T; options: { value: T; label: ReactNode; title?: string }[]; onChange: (v: T) => void;
}) {
  return (
    <div className="inline-flex rounded-lg bg-zinc-100 p-0.5 dark:bg-zinc-800/80">
      {options.map((o) => (
        <button key={o.value} type="button" title={o.title} aria-pressed={o.value === value} onClick={() => onChange(o.value)}
                className={cx('inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium transition',
                  o.value === value
                    ? 'bg-white text-zinc-900 shadow-sm dark:bg-zinc-950 dark:text-white'
                    : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100')}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

// -- layout pieces ----------------------------------------------------------

export function PageHeader({ title, description, actions, eyebrow }: {
  title: string; description?: ReactNode; actions?: ReactNode; eyebrow?: ReactNode;
}) {
  return (
    <div className="mb-8 flex flex-col gap-4 sm:flex-row sm:items-end">
      <div className="min-w-0 flex-1">
        {eyebrow && <div className="mb-2 text-xs font-semibold tracking-wider text-brand-600 uppercase dark:text-brand-400">{eyebrow}</div>}
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-950 sm:text-3xl dark:text-white">{title}</h1>
        {description && <p className="mt-2 max-w-2xl text-sm text-zinc-500 dark:text-zinc-400">{description}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export function Card({ title, description, actions, children, className, bodyClassName, accent }: {
  title?: ReactNode; description?: ReactNode; actions?: ReactNode; children?: ReactNode;
  className?: string; bodyClassName?: string; accent?: string | null;
}) {
  return (
    <section className={cx('relative overflow-hidden rounded-2xl bg-white shadow-soft ring-1 ring-zinc-200/70',
      'dark:bg-zinc-900/70 dark:ring-zinc-800', className)}>
      {accent && <div className="absolute inset-x-0 top-0 h-1" style={{ background: accent }} />}
      {(title || actions) && (
        <header className="flex flex-wrap items-start gap-3 border-b border-zinc-100 px-5 py-4 dark:border-zinc-800/80">
          <div className="min-w-0 flex-1">
            {title && <h2 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{title}</h2>}
            {description && <p className="mt-0.5 text-xs text-zinc-500">{description}</p>}
          </div>
          {actions && <div className="flex flex-wrap items-center gap-1.5">{actions}</div>}
        </header>
      )}
      <div className={bodyClassName ?? 'p-5'}>{children}</div>
    </section>
  );
}

const TONES = {
  neutral: 'bg-zinc-100 text-zinc-700 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700',
  brand: 'bg-brand-50 text-brand-700 ring-brand-200 dark:bg-brand-500/10 dark:text-brand-300 dark:ring-brand-500/30',
  green: 'bg-emerald-50 text-emerald-700 ring-emerald-200 dark:bg-emerald-500/10 dark:text-emerald-300 dark:ring-emerald-500/30',
  amber: 'bg-amber-50 text-amber-800 ring-amber-200 dark:bg-amber-500/10 dark:text-amber-300 dark:ring-amber-500/30',
  red: 'bg-red-50 text-red-700 ring-red-200 dark:bg-red-500/10 dark:text-red-300 dark:ring-red-500/30',
} as const;
export type Tone = keyof typeof TONES;

export function Badge({ tone = 'neutral', children, className }: { tone?: Tone; children: ReactNode; className?: string }) {
  return (
    <span className={cx('inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium ring-1 ring-inset',
      TONES[tone], className)}>
      {children}
    </span>
  );
}

export function StatusDot({ tone, pulse }: { tone: 'green' | 'gray' | 'red' | 'amber'; pulse?: boolean }) {
  const color = { green: 'bg-emerald-500', gray: 'bg-zinc-400', red: 'bg-red-500', amber: 'bg-amber-500' }[tone];
  return (
    <span className="relative inline-flex size-2">
      {pulse && <span className={cx('absolute inset-0 animate-ping rounded-full opacity-60', color)} />}
      <span className={cx('relative inline-flex size-2 rounded-full', color)} />
    </span>
  );
}

export function Stat({ label, value, hint, icon }: { label: string; value: ReactNode; hint?: ReactNode; icon?: ReactNode }) {
  return (
    <div className="rounded-2xl bg-white p-4 shadow-soft ring-1 ring-zinc-200/70 dark:bg-zinc-900/70 dark:ring-zinc-800">
      <div className="flex items-center gap-2 text-xs font-medium text-zinc-500">
        {icon && <span className="flex size-6 items-center justify-center rounded-md bg-brand-50 text-brand-600 dark:bg-brand-500/10 dark:text-brand-300">{icon}</span>}
        {label}
      </div>
      <div className="mt-2 text-2xl font-semibold tracking-tight text-zinc-950 tabular-nums dark:text-white">{value}</div>
      {hint && <div className="mt-0.5 truncate text-xs text-zinc-500">{hint}</div>}
    </div>
  );
}

export function EmptyState({ icon, title, children, action }: {
  icon: ReactNode; title: string; children?: ReactNode; action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center px-6 py-10 text-center">
      <div className="mb-4 flex size-12 items-center justify-center rounded-2xl bg-gradient-to-br from-brand-100 to-fuchsia-100 text-brand-600 ring-1 ring-brand-200/60 dark:from-brand-500/15 dark:to-fuchsia-500/10 dark:text-brand-300 dark:ring-brand-500/20">
        {icon}
      </div>
      <h3 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{title}</h3>
      {children && <div className="mt-1 max-w-sm text-sm text-zinc-500">{children}</div>}
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={cx('animate-pulse rounded-lg bg-zinc-200/70 dark:bg-zinc-800', className)} />;
}

export function Notice({ tone = 'info', children }: { tone?: 'info' | 'warn' | 'error'; children: ReactNode }) {
  const styles = {
    info: 'bg-brand-50/70 text-brand-900 ring-brand-200/70 dark:bg-brand-500/10 dark:text-brand-200 dark:ring-brand-500/20',
    warn: 'bg-amber-50 text-amber-900 ring-amber-200 dark:bg-amber-500/10 dark:text-amber-200 dark:ring-amber-500/20',
    error: 'bg-red-50 text-red-800 ring-red-200 dark:bg-red-500/10 dark:text-red-200 dark:ring-red-500/20',
  }[tone];
  return (
    <div className={cx('flex items-start gap-2.5 rounded-xl px-3.5 py-2.5 text-sm ring-1 ring-inset', styles)}>
      <span className="mt-0.5 shrink-0">{tone === 'info' ? <IconInfo size={16} /> : <IconAlert size={16} />}</span>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

const ERRORS: Record<string, string> = {
  invalid_name: 'Enter a name.',
  invalid_pattern: 'This pattern cannot match anything.',
  invalid_currency: 'Currency is a 3-letter code (EUR, USD).',
  invalid_color: 'Pick a color.',
  invalid_window: 'Start and end must be different times.',
  invalid_days: 'Pick valid days.',
  invalid_body: 'Check the values.',
  duplicate_rule: 'This project already has that rule.',
  not_found: 'It no longer exists. Refresh the page.',
};

export function ErrorText({ error }: { error: unknown }) {
  if (!error) return null;
  const code = error instanceof ApiError ? error.code : 'error';
  return <p className="text-sm text-red-600 dark:text-red-400">{ERRORS[code] ?? `Something went wrong (${code}).`}</p>;
}

// -- overlays ---------------------------------------------------------------

function useEscape(onClose: () => void) {
  const ref = useRef(onClose);
  ref.current = onClose;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && ref.current();
    window.addEventListener('keydown', onKey);
    const overflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      window.removeEventListener('keydown', onKey);
      document.body.style.overflow = overflow;
    };
  }, []);
}

export function Dialog({ title, description, onClose, children, footer }: {
  title: string; description?: ReactNode; onClose: () => void; children: ReactNode; footer?: ReactNode;
}) {
  useEscape(onClose);
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-end justify-center p-4 sm:items-center" role="dialog" aria-modal="true" aria-label={title}>
      <div className="absolute inset-0 animate-fade-in bg-zinc-950/40 backdrop-blur-sm" onClick={onClose} />
      <div className="relative w-full max-w-lg animate-pop-in rounded-2xl bg-white shadow-lift ring-1 ring-zinc-200 dark:bg-zinc-900 dark:ring-zinc-800">
        <div className="flex items-start gap-3 px-6 pt-5">
          <div className="min-w-0 flex-1">
            <h2 className="text-base font-semibold text-zinc-950 dark:text-white">{title}</h2>
            {description && <p className="mt-1 text-sm text-zinc-500">{description}</p>}
          </div>
          <IconButton label="Close" onClick={onClose}><IconX size={16} /></IconButton>
        </div>
        <div className="px-6 py-5">{children}</div>
        {footer && (
          <div className="flex justify-end gap-2 rounded-b-2xl border-t border-zinc-100 bg-zinc-50/70 px-6 py-3.5 dark:border-zinc-800 dark:bg-zinc-950/40">
            {footer}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}

export function Drawer({ title, onClose, children }: { title: ReactNode; onClose: () => void; children: ReactNode }) {
  useEscape(onClose);
  return createPortal(
    <div className="fixed inset-0 z-50" role="dialog" aria-modal="true">
      <div className="absolute inset-0 animate-fade-in bg-zinc-950/30 backdrop-blur-[2px]" onClick={onClose} />
      <aside className="absolute inset-y-0 right-0 flex w-full max-w-md animate-slide-in flex-col bg-white shadow-lift ring-1 ring-zinc-200 dark:bg-zinc-900 dark:ring-zinc-800">
        <div className="flex items-center gap-3 border-b border-zinc-100 px-5 py-4 dark:border-zinc-800">
          <div className="min-w-0 flex-1 text-sm font-semibold">{title}</div>
          <IconButton label="Close" onClick={onClose}><IconX size={16} /></IconButton>
        </div>
        <div className="scroll-thin flex-1 overflow-y-auto p-5">{children}</div>
      </aside>
    </div>,
    document.body,
  );
}

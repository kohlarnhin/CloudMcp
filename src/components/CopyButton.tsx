import { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { toast } from 'sonner';
import { copyText } from '../lib/desktop';
import { cn } from '../lib/utils';

export function CopyButton({ text, label = '复制', resolve, className, button, disabled, onCopied }: {
  text?: string;
  label?: string;
  resolve?: () => Promise<string>;
  className?: string;
  /** Render as a toolbar pill instead of a compact tag. */
  button?: boolean;
  disabled?: boolean;
  onCopied?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await copyText(resolve ? await resolve() : text ?? '');
      onCopied?.();
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch (error) {
      toast.error(String(error));
    }
  };
  return <button type="button" className={cn(button ? 'h1-button' : 'h1-tag', className)} disabled={disabled} onClick={() => { void copy(); }}>
    {copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}{copied ? '已复制' : label}
  </button>;
}

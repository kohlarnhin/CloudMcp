import { cn } from '../../lib/utils';

export function Switch({ id, checked, onCheckedChange, disabled, className, ...aria }: {
  id?: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
  'aria-describedby'?: string;
  'aria-label'?: string;
}) {
  return <button id={id} type="button" role="switch" aria-checked={checked} disabled={disabled} data-state={checked ? 'checked' : 'unchecked'}
    className={cn('h1-switch', className)} onClick={() => onCheckedChange(!checked)} {...aria}>
    <span aria-hidden="true" />
  </button>;
}

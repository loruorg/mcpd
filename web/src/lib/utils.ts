/**
 * The class merger every shadcn primitive is built on.
 *
 * `clsx` resolves the conditionals; `tailwind-merge` then drops any earlier
 * class that a later one overrides. Without the second step a caller's
 * `className` loses to the component's own default, which is exactly backwards:
 * the point of accepting a `className` is to let the caller win.
 */
import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

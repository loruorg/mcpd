/**
 * Tooltip, on Radix.
 *
 * The shadcn primitive with its default styling replaced by this project's
 * panel material: a raised surface, a hairline in its own colour, a light lip
 * along the top edge, and no shadow. The point of taking the primitive is the
 * behaviour nobody should hand-roll, and it is a long list: focus and hover
 * both open it, Escape closes it, it never traps the pointer, it flips and
 * shifts to stay on screen, it is announced as a description of the thing it
 * belongs to rather than as separate content, and it does not open on a touch
 * tap where there is no hover to speak of.
 */
import * as React from 'react'
import * as TooltipPrimitive from '@radix-ui/react-tooltip'
import { cn } from '@/lib/utils'

export const TooltipProvider = TooltipPrimitive.Provider
export const Tooltip = TooltipPrimitive.Root
export const TooltipTrigger = TooltipPrimitive.Trigger

export const TooltipContent = React.forwardRef<
  React.ElementRef<typeof TooltipPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TooltipPrimitive.Content>
>(({ className, sideOffset = 6, ...props }, ref) => (
  <TooltipPrimitive.Portal>
    <TooltipPrimitive.Content
      ref={ref}
      sideOffset={sideOffset}
      className={cn(
        'panel-raised z-50 max-w-[min(28rem,90vw)] px-2.5 py-1.5',
        'text-[0.75rem] leading-snug text-ink',
        // The content is a span name or a value, which is data. It gets the
        // data face and is allowed to wrap rather than being truncated a
        // second time inside the thing that exists to show it in full.
        'data break-words',
        className,
      )}
      {...props}
    />
  </TooltipPrimitive.Portal>
))
TooltipContent.displayName = TooltipPrimitive.Content.displayName

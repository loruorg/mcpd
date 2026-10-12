/**
 * Text that is clipped by its column, with the rest available on hover.
 *
 * The tooltip appears only when there is something to reveal. A tooltip that
 * repeats a line you can already read in full is noise: it covers the row under
 * the pointer and teaches the reader that hovering here is not worth it, so by
 * the time a name IS clipped they have stopped looking.
 *
 * Whether it is clipped is decided when the pointer arrives, not when the row
 * mounts. Measuring on mount looks obvious and does not survive contact: the
 * first measurement runs against the fallback font, so the string is narrower
 * than it will be; a `ResizeObserver` does not rescue that, because it watches
 * this element's own box, which the grid column sets and which does not move
 * when the TEXT reflows; and the live tail re-renders the list often enough
 * that anything cached on mount goes stale. Reading `scrollWidth` once, on
 * open, has none of those failure modes and costs one reflow on a deliberate
 * hover.
 */
import * as React from 'react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

export function Truncated({ children, className }: { children: string; className?: string }) {
  const ref = React.useRef<HTMLSpanElement>(null)
  const [open, setOpen] = React.useState(false)

  return (
    <Tooltip
      open={open}
      onOpenChange={(next) => {
        const el = ref.current
        // One pixel of tolerance: sub-pixel layout makes scrollWidth exceed
        // clientWidth by a fraction on text that is not actually clipped.
        const clipped = !!el && el.scrollWidth > el.clientWidth + 1
        setOpen(next && clipped)
      }}
    >
      <TooltipTrigger asChild>
        <span ref={ref} className={className}>
          {children}
        </span>
      </TooltipTrigger>
      <TooltipContent side="top" align="start">
        {children}
      </TooltipContent>
    </Tooltip>
  )
}

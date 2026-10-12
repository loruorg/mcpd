/**
 * The one frame loop every figure runs on.
 *
 * Time-based, with `dt` clamped so a tab that was in the background does not
 * jump a part across the bench when it comes back. The loop runs only while
 * the figure is on screen and the page is visible, and stops when the figure
 * says nothing is moving. Under reduced motion `calm` is true: the figure jumps
 * to its targets, runs no tour, and still answers the pointer and the keys.
 *
 * The idle tour drives the same targets the reader would, starts after a short
 * wait, pauses on any input and resumes a few seconds after the last one.
 */
export interface LiveOptions {
  /** Advance by `dt` seconds. Return true while anything still moves. */
  tick: (dt: number, calm: boolean) => boolean
  /** Called with a step count when the tour moves on. */
  tour?: (step: number) => void
  /** Seconds between tour steps. */
  tourEvery?: number
}

const REDUCED = '(prefers-reduced-motion: reduce)'

export function live(stage: HTMLElement, { tick, tour, tourEvery = 4 }: LiveOptions) {
  const still = matchMedia(REDUCED)
  let frame = 0
  let last = 0
  let visible = false
  let idleSince = -Infinity
  let tourClock = 0
  let step = 0
  let wantMore = true

  const touring = () => !!tour && !still.matches && performance.now() - idleSince > 3600

  const loop = (now: number) => {
    const dt = last ? Math.min((now - last) / 1000, 1 / 30) : 1 / 60
    last = now
    if (touring()) {
      tourClock += dt
      if (tourClock >= tourEvery) {
        tourClock = 0
        step += 1
        tour!(step)
      }
    }
    wantMore = tick(dt, still.matches)
    frame = visible && !document.hidden && (wantMore || touring()) ? requestAnimationFrame(loop) : 0
  }
  const run = () => {
    if (!frame && visible && !document.hidden) {
      last = 0
      frame = requestAnimationFrame(loop)
    }
  }

  new IntersectionObserver(
    (entries) => {
      // The newest entry: one callback can carry a leave and a return.
      visible = entries[entries.length - 1]!.isIntersecting
      if (visible) run()
    },
    { rootMargin: '120px 0px' },
  ).observe(stage)
  document.addEventListener('visibilitychange', run)
  still.addEventListener?.('change', run)

  return {
    /** The reader acted: hold the tour, and run until things settle. */
    touch() {
      idleSince = performance.now()
      tourClock = 0
      run()
    },
    run,
  }
}

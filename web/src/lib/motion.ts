/**
 * The one scheduler every figure on the site runs on.
 *
 * A figure is an element with `data-motion`. It plays only while it is on
 * screen, the tab is visible and the reader has not asked for reduced motion.
 * While it plays it carries `data-playing`, which `panel.css` reads to pause
 * CSS animations inside it, and the callback below receives the time since the
 * figure started, in seconds, once per frame.
 *
 * The markup of every figure is its finished state. A figure that never plays
 * (no JavaScript, reduced motion, a print) is complete and readable; playing
 * only replays how it got there.
 */

export type Frame = (t: number) => void

const reduced = () =>
  typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches

/**
 * Run `frame(t)` on every animation frame while `el` is visible.
 *
 * `t` keeps counting across pauses rather than jumping, so a figure scrolled
 * away mid-sequence picks up where it was. `rest()` is called when motion is
 * reduced, and should draw the finished state (the markup usually already is).
 */
export function animate(el: HTMLElement, frame: Frame, rest?: () => void): void {
  if (reduced()) {
    rest?.()
    return
  }
  let visible = false
  let raf = 0
  let last = 0
  let t = 0

  const tick = (now: number) => {
    const dt = last ? Math.min(0.1, (now - last) / 1000) : 0
    last = now
    t += dt
    frame(t)
    raf = requestAnimationFrame(tick)
  }
  const sync = () => {
    const on = visible && document.visibilityState === 'visible'
    if (on && !raf) {
      el.setAttribute('data-playing', '')
      last = 0
      raf = requestAnimationFrame(tick)
    } else if (!on && raf) {
      el.removeAttribute('data-playing')
      cancelAnimationFrame(raf)
      raf = 0
    }
  }
  new IntersectionObserver(
    (entries) => {
      visible = entries.some((e) => e.isIntersecting)
      sync()
    },
    { rootMargin: '80px 0px' },
  ).observe(el)
  document.addEventListener('visibilitychange', sync)
}

/** Every element on the page matching `selector`, each with its own clock. */
export function each(selector: string, setup: (el: HTMLElement) => void): void {
  document.querySelectorAll<HTMLElement>(selector).forEach((el) => {
    if (el.dataset.motionReady) return
    el.dataset.motionReady = '1'
    setup(el)
  })
}

/** 0 before `a`, 1 after `b`, linear between. */
export const span = (t: number, a: number, b: number) =>
  t <= a ? 0 : t >= b ? 1 : (t - a) / (b - a)

/** An ease that settles: fast out, slow in. */
export const easeOut = (x: number) => 1 - Math.pow(1 - x, 3)

/** Format milliseconds the way the interface does. */
export const ms = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(2)} s` : `${Math.round(n)} ms`)

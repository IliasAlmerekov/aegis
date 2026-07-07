import { useEffect, useRef, useState } from 'react'

// Counts a stat's leading digits up to their target value once the stat
// scrolls into view. Non-numeric values (e.g. "MIT") render statically.
export function NumberTicker({ value, inView }) {
  const match = value.match(/^(\d+)(.*)$/)
  const target = match ? parseInt(match[1], 10) : null
  const suffix = match ? match[2] : ''

  const [display, setDisplay] = useState(target === null ? value : `0${suffix}`)
  // Only latches once the count-up actually finishes — if a flickering
  // IntersectionObserver cancels playback mid-flight, the next inView
  // still gets a chance to finish the count instead of getting stuck.
  const done = useRef(false)

  useEffect(() => {
    if (target === null || !inView || done.current) return

    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      setDisplay(value)
      done.current = true
      return
    }

    const duration = 900
    let raf
    // Anchor elapsed time to the first rAF callback's own timestamp rather
    // than a performance.now() taken beforehand — the two clocks can
    // disagree by a frame, which briefly yields a negative t (and a
    // negative eased/target) on the very first tick.
    let start = null

    const tick = (now) => {
      if (start === null) start = now
      const t = Math.min(Math.max((now - start) / duration, 0), 1)
      const eased = 1 - Math.pow(1 - t, 3)
      setDisplay(`${Math.round(target * eased)}${suffix}`)
      if (t < 1) {
        raf = requestAnimationFrame(tick)
      } else {
        done.current = true
      }
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [inView, target, suffix, value])

  return <span className="tabular-nums">{display}</span>
}

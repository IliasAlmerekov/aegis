import { useEffect, useRef, useState } from 'react'

// Shared live-session player for TerminalWindow content.
// Scenario step kinds:
//  cmd    — typed character by character behind a prompt
//  out    — output line appearing after `delay`
//  danger — the Aegis warning box
//  actions— the y/N/? row; highlights the N key after a beat
//  pause  — dramatic beat
// Any item may carry `step: <n>` — the player reports it via onStep,
// letting a stepper alongside track playback. `seek` ({step, token})
// restarts playback with everything before that step rendered instantly.

function DangerBox({ id, title, alt }) {
  return (
    <div className="danger-box-enter mt-4 rounded border border-[#7fee64]/30 p-3" style={{ backgroundColor: '#0f1e0f' }}>
      <p className="text-xs font-semibold" style={{ color: '#c8f9b6' }}>
        ⚠ DANGER — {id}
      </p>
      <p className="mt-1 text-xs text-[#677d64]">{title}</p>
      <p className="mt-1 text-xs text-[#485346]">{alt}</p>
    </div>
  )
}

function ActionRow({ pressed }) {
  const base = 'rounded px-3 py-1 text-xs'
  return (
    <div className="mt-4 flex items-center gap-4" aria-hidden="true">
      <span className={`${base} font-semibold text-[#000000]`} style={{ backgroundColor: '#7fee64', opacity: pressed ? 0.35 : 1 }}>
        y Allow
      </span>
      <span
        className={`${base} border transition-colors duration-150 ${
          pressed
            ? 'key-pressed border-[#7fee64] text-[#000000] font-semibold'
            : 'border-[#3e4a3c] text-[#677d64]'
        }`}
        style={pressed ? { backgroundColor: '#7fee64' } : undefined}
      >
        N Deny
      </span>
      <span className={`${base} border border-[#3e4a3c] text-[#677d64]`} style={{ opacity: pressed ? 0.35 : 1 }}>
        ? Details
      </span>
    </div>
  )
}

export function LiveTerminal({ scenario, playing, minHeightClass = 'min-h-[280px]', onStep, seek }) {
  const [items, setItems] = useState([])
  const runRef = useRef(0)
  const onStepRef = useRef(onStep)
  onStepRef.current = onStep
  const seekRef = useRef(null)

  useEffect(() => {
    if (seek) seekRef.current = seek.step
  }, [seek])

  useEffect(() => {
    const run = ++runRef.current
    const alive = () => runRef.current === run

    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      setItems(
        scenario
          .filter((s) => s.k !== 'pause')
          .map((s) => ({ ...s, shown: s.text?.length ?? 0, pressed: true }))
      )
      return
    }
    if (!playing) return

    const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
    ;(async () => {
      let startStep = seekRef.current ?? 0
      seekRef.current = null
      while (alive()) {
        setItems([])
        await sleep(400)
        let cur = 0
        for (const step of scenario) {
          if (!alive()) return
          if (step.step !== undefined && step.step !== cur) {
            cur = step.step
          }
          const instant = cur < startStep
          if (step.step !== undefined && (instant || cur >= startStep)) {
            onStepRef.current?.(cur)
          }
          if (step.k === 'pause') {
            if (!instant) await sleep(step.ms)
          } else if (instant) {
            // Fast-forward: render fully resolved, no delays.
            setItems((prev) => [
              ...prev,
              { ...step, shown: step.text?.length ?? 0, pressed: true },
            ])
          } else if (step.k === 'cmd') {
            setItems((prev) => [...prev, { ...step, shown: 0 }])
            for (let c = 1; c <= step.text.length; c++) {
              await sleep(30)
              if (!alive()) return
              setItems((prev) =>
                prev.map((it, i) => (i === prev.length - 1 ? { ...it, shown: c } : it))
              )
            }
            await sleep(220)
          } else if (step.k === 'actions') {
            await sleep(step.delay ?? 300)
            if (!alive()) return
            setItems((prev) => [...prev, { ...step, pressed: false }])
            await sleep(1000)
            if (!alive()) return
            setItems((prev) =>
              prev.map((it, i) => (i === prev.length - 1 ? { ...it, pressed: true } : it))
            )
            await sleep(400)
          } else {
            await sleep(step.delay ?? 300)
            if (!alive()) return
            setItems((prev) => [...prev, step])
          }
        }
        await sleep(3400)
        startStep = 0 // after a seeked pass, loop plays in full
      }
    })()

    return () => {
      runRef.current++
    }
  }, [scenario, playing, seek])

  const last = items[items.length - 1]
  const typingNow = last?.k === 'cmd' && last.shown < last.text.length

  return (
    <div className={`${minHeightClass} space-y-1 font-mono text-sm`}>
      {items.map((it, i) => {
        if (it.k === 'cmd') {
          const text = it.text.slice(0, it.shown)
          return (
            <p key={i} className={i > 0 ? 'mt-3 text-xs' : undefined} style={{ color: it.color }}>
              <span className="mr-2 text-[#7fee64]">{it.prompt}</span>
              {text}
              {i === items.length - 1 && typingNow && <span className="term-cursor" />}
            </p>
          )
        }
        if (it.k === 'danger') return <DangerBox key={i} {...it} />
        if (it.k === 'actions') return <ActionRow key={i} pressed={it.pressed} />
        return (
          <p key={i} className="mt-2 break-all text-xs" style={{ color: it.color }}>
            {it.text}
          </p>
        )
      })}
      {!typingNow && <span className="term-cursor" aria-hidden="true" />}
    </div>
  )
}

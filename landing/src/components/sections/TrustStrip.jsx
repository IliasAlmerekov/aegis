import { Reveal, useInView } from '../ui/Reveal'
import { NumberTicker } from '../ui/NumberTicker'

const STATS = [
  {
    value: '1-pass',
    label: 'Safe-path scan',
    detail: 'Safe commands clear the Aho-Corasick fast scan in a single pass — no regex on the hot path, so overhead stays minimal.',
  },
  {
    value: '0',
    label: 'Bytes to any server',
    detail: 'Aegis is entirely local. No telemetry, no callbacks, no analytics. Your commands stay on your machine.',
  },
  {
    value: '100%',
    label: 'Decisions on record',
    detail: 'Every approval, denial, and auto-pass appends an entry to your local JSONL audit log — tamper-evident when hash-chain integrity is enabled.',
  },
  {
    value: 'MIT',
    label: 'License',
    detail: 'Read the source, audit the patterns, fork it, or build on top. No restrictions.',
  },
]

export function TrustStrip() {
  const [gridRef, gridInView] = useInView(0.3)

  return (
    <section
      aria-label="Why trust Aegis"
      style={{ backgroundColor: '#def0dd' }}
    >
      <div className="mx-auto max-w-[1200px] px-6 py-16 lg:py-20">
        {/* Header */}
        <Reveal className="mb-12 flex flex-col gap-2 lg:flex-row lg:items-end lg:justify-between">
          <h2 className="font-display text-3xl font-medium leading-tight tracking-tight text-[#000000] lg:text-4xl">
            Built to be trusted.
          </h2>
          <p className="max-w-[400px] font-body text-sm leading-relaxed text-[#485346] lg:text-right">
            Open source means you can verify every claim below. The code is on GitHub.
          </p>
        </Reveal>

        {/* Stats grid */}
        <div
          ref={gridRef}
          className="grid grid-cols-2 gap-px lg:grid-cols-4"
          style={{ backgroundColor: '#aed2a4' }}
        >
          {STATS.map(({ value, label, detail }, i) => (
            <div
              key={label}
              className="trust-card px-6 py-8 lg:px-8"
              style={{ backgroundColor: '#def0dd' }}
            >
              <Reveal delay={80 + i * 70} className="flex flex-col gap-3">
                <span
                  className="font-display text-[44px] font-medium leading-none tracking-tight text-[#000000]"
                  aria-label={value}
                >
                  <NumberTicker value={value} inView={gridInView} />
                </span>
                <div className="flex flex-col gap-1.5">
                  <span className="font-mono text-xs font-medium uppercase tracking-widest text-[#677d64]">
                    {label}
                  </span>
                  <p className="font-body text-sm leading-relaxed text-[#485346]">
                    {detail}
                  </p>
                </div>
              </Reveal>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}

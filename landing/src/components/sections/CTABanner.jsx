import { Reveal } from '../ui/Reveal'

export function CTABanner() {
  return (
    <section
      className="mx-auto w-full max-w-[1200px] px-6 py-16"
      aria-label="Call to action"
    >
      <Reveal>
      <div
        className="relative overflow-hidden rounded-lg px-8 py-14 text-center"
        style={{
          background: '#0d1e0d',
          border: '1px solid #3e4a3c',
        }}
      >
        {/* Subtle radial glow */}
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0"
          style={{
            background:
              'radial-gradient(ellipse 60% 80% at 50% 50%, rgba(127,238,100,0.07) 0%, transparent 70%)',
          }}
        />

        <p className="relative mb-4 font-mono text-xs font-medium tracking-widest text-[#677d64] uppercase">
          Get started
        </p>
        <h2 className="relative font-display text-4xl font-medium leading-tight tracking-tight text-[#ddffdc] sm:text-5xl">
          Guard your stack in minutes.
        </h2>
        <p className="relative mx-auto mt-5 max-w-[480px] font-body text-[15px] leading-relaxed text-[#677d64]">
          Open source, zero telemetry, minimal overhead on the safe path. Install
          with the installer, Homebrew, npm, or Cargo and your AI agents work
          under supervision.
        </p>

        <div className="relative mt-8 flex flex-wrap items-center justify-center gap-3">
          <a
            href="https://github.com/IliasAlmerekov/aegis"
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex h-11 items-center gap-2 rounded px-6 text-sm font-medium text-[#000000] transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7fee64]"
            style={{ backgroundColor: '#7fee64' }}
            onMouseEnter={e => e.currentTarget.style.backgroundColor = '#c8f9b6'}
            onMouseLeave={e => e.currentTarget.style.backgroundColor = '#7fee64'}
          >
            View on GitHub
          </a>
          <a
            href="#how-it-works"
            className="inline-flex h-11 items-center gap-2 rounded border border-[#3e4a3c] px-6 text-sm font-medium text-[#ddffdc] transition-colors duration-150 hover:border-[#677d64] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7fee64]"
          >
            Read the docs
          </a>
        </div>
      </div>
      </Reveal>
    </section>
  )
}

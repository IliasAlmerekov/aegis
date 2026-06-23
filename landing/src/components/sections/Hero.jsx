import { lazy, Suspense } from 'react'

const ShieldScene = lazy(() =>
  import('../3d/ShieldScene').then((m) => ({ default: m.ShieldScene }))
)

export function Hero() {
  return (
    <section
      id="hero"
      className="relative flex min-h-dvh flex-col items-center justify-center overflow-hidden pt-16"
      aria-label="Hero"
    >
      {/* Background grid */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0"
        style={{
          backgroundImage:
            'linear-gradient(rgba(127,238,100,0.04) 1px, transparent 1px), linear-gradient(90deg, rgba(127,238,100,0.04) 1px, transparent 1px)',
          backgroundSize: '48px 48px',
        }}
      />
      {/* Radial glow */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0"
        style={{
          background:
            'radial-gradient(ellipse 60% 55% at 60% 45%, rgba(127,238,100,0.08) 0%, transparent 70%)',
        }}
      />

      <div className="relative mx-auto flex w-full max-w-[1200px] flex-col items-center gap-16 px-6 py-20 lg:flex-row lg:gap-8">
        {/* Left column — text */}
        <div className="flex w-full min-w-0 flex-1 flex-col items-center text-center lg:items-start lg:text-left">
          {/* Eyebrow */}
          <div className="mb-5 flex items-center gap-2 rounded-full border border-[#3e4a3c] px-3 py-1.5" style={{ backgroundColor: '#1a2018' }}>
            <span className="h-1.5 w-1.5 rounded-full bg-[#7fee64]" aria-hidden="true" />
            <span className="font-mono text-xs font-medium tracking-widest text-[#677d64] uppercase">
              Open Source · Rust · v0.5.8
            </span>
          </div>

          {/* Headline */}
          <h1 className="font-display text-5xl font-medium leading-[1.05] tracking-tight text-[#ddffdc] sm:text-6xl lg:text-[64px]">
            Your AI agent{' '}
            <span className="text-[#7fee64]">asked first.</span>
          </h1>

          {/* Subheadline */}
          <p className="mt-6 max-w-[480px] font-body text-[17px] leading-relaxed text-[#aed2a4]">
            Aegis sits between your AI assistant and the shell. Dangerous commands —
            deletes, drops, overwrites — pause for your approval. Safe ones pass
            through with minimal overhead.
          </p>

          {/* CTAs */}
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <a
              href="https://github.com/IliasAlmerekov/aegis"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex h-11 items-center gap-2 rounded px-5 text-sm font-semibold text-[#000000] transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7fee64]"
              style={{ backgroundColor: '#7fee64' }}
              onMouseEnter={e => e.currentTarget.style.backgroundColor = '#c8f9b6'}
              onMouseLeave={e => e.currentTarget.style.backgroundColor = '#7fee64'}
            >
              Get started — free
            </a>
            <a
              href="#how-it-works"
              className="inline-flex h-11 items-center gap-2 rounded border border-[#3e4a3c] px-5 text-sm font-medium text-[#ddffdc] transition-colors duration-150 hover:border-[#677d64] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7fee64]"
            >
              See how it works
            </a>
          </div>

          {/* Install snippet */}
          <div className="mt-8 flex w-full min-w-0 max-w-full items-start gap-3 rounded border border-[#3e4a3c] px-4 py-2.5 text-left" style={{ backgroundColor: '#0d1210' }}>
            <span className="font-mono text-xs leading-relaxed text-[#677d64]">$</span>
            <code className="min-w-0 flex-1 break-all font-mono text-xs leading-relaxed text-[#7fee64]">
              curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
            </code>
            <button
              className="ml-1 shrink-0 rounded p-1 text-[#677d64] transition-colors duration-150 hover:text-[#ddffdc] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64] cursor-pointer"
              aria-label="Copy install command"
              onClick={() => navigator.clipboard?.writeText('curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh')}
            >
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
              </svg>
            </button>
          </div>
        </div>

        {/* Right column — 3D Shield */}
        <div className="relative flex h-[420px] w-full max-w-[420px] items-center justify-center lg:h-[560px] lg:max-w-[500px]">
          <Suspense
            fallback={
              <div className="flex h-full w-full items-center justify-center">
                <div className="h-32 w-24 rounded-lg border border-[#3e4a3c]/40 bg-[#7fee64]/5 animate-pulse" />
              </div>
            }
          >
            <ShieldScene />
          </Suspense>
        </div>
      </div>

      {/* Scroll indicator */}
      <div
        className="absolute bottom-8 left-1/2"
        aria-hidden="true"
        style={{ animation: 'scroll-hint 2.4s cubic-bezier(0.4, 0, 0.6, 1) infinite' }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#677d64" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M6 9l6 6 6-6" />
        </svg>
      </div>
    </section>
  )
}

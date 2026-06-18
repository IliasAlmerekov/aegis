import { useState } from 'react'
import { TerminalWindow } from '../ui/TerminalWindow'

const TABS = [
  {
    id: 'intercept',
    label: 'INTERCEPT',
    heading: 'Every command scanned before it runs.',
    body: 'Aegis sits as your $SHELL, intercepting every command your AI agent attempts. The Aho-Corasick fast scan classifies risk in under 2 ms — safe commands pass through invisibly.',
  },
  {
    id: 'approve',
    label: 'APPROVE',
    heading: 'One keystroke between danger and safety.',
    body: 'Dangerous commands surface a TUI dialog with full context — the command, the risk pattern, and a safe alternative. You press y or N. Aegis respects your answer.',
  },
  {
    id: 'audit',
    label: 'AUDIT',
    heading: 'Append-only log of every decision.',
    body: 'Every approval and denial writes a signed JSONL entry to ~/.aegis/audit.jsonl. Immutable, timestamped, ready to pipe into your SIEM or grep.',
  },
]

export function FeatureSection() {
  const [active, setActive] = useState('intercept')
  const tab = TABS.find((t) => t.id === active)

  return (
    <section
      id="why-aegis"
      className="mx-auto w-full max-w-[1200px] px-6 py-24"
      aria-labelledby="why-aegis-heading"
    >
      {/* Eyebrow */}
      <p className="mb-4 font-mono text-xs font-medium tracking-widest text-[#677d64] uppercase">
        Why Aegis?
      </p>

      <div className="flex flex-col gap-12 lg:flex-row lg:gap-16">
        {/* Left — description */}
        <div className="flex flex-1 flex-col">
          <h2
            id="why-aegis-heading"
            className="font-display text-4xl font-medium leading-tight tracking-tight text-[#ddffdc] lg:text-5xl"
          >
            AI agents are powerful.<br />
            <span className="text-[#7fee64]">They don't ask permission.</span>
          </h2>

          {/* Tabs */}
          <div className="mt-8 flex gap-0 border-b border-[#3e4a3c]" role="tablist" aria-label="Features">
            {TABS.map((t) => (
              <button
                key={t.id}
                role="tab"
                aria-selected={active === t.id}
                aria-controls={`panel-${t.id}`}
                id={`tab-${t.id}`}
                onClick={() => setActive(t.id)}
                className={`px-4 py-2.5 font-mono text-xs tracking-widest transition-colors duration-150 cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64] ${
                  active === t.id
                    ? 'border-b-2 border-[#7fee64] text-[#7fee64]'
                    : 'text-[#677d64] hover:text-[#ddffdc]'
                }`}
              >
                {t.label}
              </button>
            ))}
          </div>

          <div
            id={`panel-${tab.id}`}
            role="tabpanel"
            aria-labelledby={`tab-${tab.id}`}
            className="mt-6"
          >
            <h3 className="font-display text-xl font-medium text-[#ddffdc]">
              {tab.heading}
            </h3>
            <p className="mt-3 font-body text-[15px] leading-relaxed text-[#677d64]">
              {tab.body}
            </p>
          </div>
        </div>

        {/* Right — Terminal mock */}
        <div className="flex-1 lg:max-w-[520px]">
          <TerminalWindow title="aegis — zsh">
            <div className="space-y-1 font-mono text-sm">
              <p className="text-[#677d64]">
                <span className="text-[#7fee64]">$</span> aegis run -- zsh
              </p>
              <p className="text-[#677d64] text-xs mt-3">
                agent: rm -rf /var/log/nginx &amp;&amp; systemctl restart nginx
              </p>

              {/* Warning box */}
              <div className="mt-4 rounded border border-[#7fee64]/30 p-3" style={{ backgroundColor: '#0f1e0f' }}>
                <p className="text-xs font-semibold" style={{ color: '#c8f9b6' }}>
                  ⚠ DANGER — FS-003
                </p>
                <p className="mt-1 text-xs text-[#677d64]">
                  Recursive delete of system log directory
                </p>
                <p className="mt-1 text-xs text-[#485346]">
                  safe alt: truncate -s 0 /var/log/nginx/*.log
                </p>
              </div>

              {/* Action row */}
              <div className="mt-4 flex items-center gap-4">
                <button className="rounded px-3 py-1 text-xs font-semibold text-[#000000] cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64]" style={{ backgroundColor: '#7fee64' }}>
                  y Allow
                </button>
                <button className="rounded border border-[#3e4a3c] px-3 py-1 text-xs text-[#677d64] cursor-pointer hover:border-[#677d64] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64]">
                  N Deny
                </button>
                <button className="rounded border border-[#3e4a3c] px-3 py-1 text-xs text-[#677d64] cursor-pointer hover:border-[#677d64] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64]">
                  ? Details
                </button>
              </div>
            </div>
          </TerminalWindow>
        </div>
      </div>
    </section>
  )
}

import { useLayoutEffect, useRef, useState } from 'react'
import { TerminalWindow } from '../ui/TerminalWindow'
import { LiveTerminal } from '../ui/LiveTerminal'
import { Reveal, useInView } from '../ui/Reveal'

const TABS = [
  {
    id: 'intercept',
    label: 'INTERCEPT',
    heading: 'Every command scanned before it runs.',
    body: 'Aegis sits as your $SHELL, intercepting every command your AI agent attempts. The Aho-Corasick fast scan classifies risk with minimal overhead — safe commands pass through invisibly.',
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
    body: 'Every approval and denial appends a JSONL entry to ~/.aegis/audit.jsonl. Append-only, timestamped, tamper-evident when hash-chain integrity is enabled.',
  },
]

const SCENARIOS = {
  intercept: [
    { k: 'cmd', prompt: 'agent:', text: 'cargo build --release', color: '#ddffdc' },
    { k: 'out', text: '✓ safe — passed through in 0.4ms', color: '#485346', delay: 420 },
    { k: 'pause', ms: 900 },
    { k: 'cmd', prompt: 'agent:', text: 'rm -rf /var/log/nginx', color: '#ddffdc' },
    {
      k: 'danger',
      id: 'FS-001',
      title: 'Recursive force delete — no recovery path',
      alt: 'safe alt: mv /var/log/nginx /tmp/backup-$(date +%s)',
      delay: 500,
    },
    { k: 'out', text: 'waiting for your decision…', color: '#677d64', delay: 700 },
  ],
  approve: [
    { k: 'cmd', prompt: 'agent:', text: 'DROP TABLE users;', color: '#ddffdc' },
    {
      k: 'danger',
      id: 'DB-001',
      title: 'Destructive SQL — table is gone for good',
      alt: 'safe alt: ALTER TABLE users RENAME TO users_retired',
      delay: 500,
    },
    { k: 'actions', press: 'N', delay: 400 },
    { k: 'out', text: '✖ denied — logged to audit.jsonl', color: '#677d64', delay: 420 },
  ],
  audit: [
    { k: 'cmd', prompt: '$', text: 'tail -f ~/.aegis/audit.jsonl', color: '#ddffdc' },
    { k: 'out', text: '{"ts":"09:12:43Z","cmd":"rm -rf /var/log/nginx","decision":"denied","pattern":"FS-001"}', color: '#859085', delay: 600 },
    { k: 'out', text: '{"ts":"09:13:01Z","cmd":"git reset --hard HEAD~3","decision":"approved","pattern":"GIT-001"}', color: '#7fee64', delay: 900 },
    { k: 'out', text: '{"ts":"09:15:22Z","cmd":"cargo build --release","decision":"pass"}', color: '#485346', delay: 900 },
    { k: 'out', text: '{"ts":"09:17:08Z","cmd":"DROP TABLE sessions;","decision":"denied","pattern":"DB-001"}', color: '#859085', delay: 900 },
  ],
}

export function FeatureSection() {
  const [active, setActive] = useState('intercept')
  const tab = TABS.find((t) => t.id === active)
  const [inViewRef, inView] = useInView(0.2)
  const tabRefs = useRef({})
  const [indicator, setIndicator] = useState({ left: 0, width: 0 })

  useLayoutEffect(() => {
    const el = tabRefs.current[active]
    if (el) setIndicator({ left: el.offsetLeft, width: el.offsetWidth })
  }, [active])

  return (
    <section
      id="why-aegis"
      className="mx-auto w-full max-w-[1200px] px-6 py-24"
      aria-labelledby="why-aegis-heading"
    >
      {/* Eyebrow */}
      <Reveal>
        <p className="mb-4 font-mono text-xs font-medium tracking-widest text-[#677d64] uppercase">
          Why Aegis?
        </p>
      </Reveal>

      <div className="flex flex-col gap-12 lg:flex-row lg:gap-16">
        {/* Left — description */}
        <div className="flex flex-1 flex-col">
          <Reveal delay={60}>
            <h2
              id="why-aegis-heading"
              className="font-display text-4xl font-medium leading-tight tracking-tight text-[#ddffdc] lg:text-5xl"
            >
              AI agents are powerful.<br />
              <span className="text-[#7fee64]">They don't ask permission.</span>
            </h2>
          </Reveal>

          {/* Tabs */}
          <Reveal delay={140}>
            <div className="relative mt-8 flex gap-0 border-b border-[#3e4a3c]" role="tablist" aria-label="Features">
              {TABS.map((t) => (
                <button
                  key={t.id}
                  ref={(el) => { tabRefs.current[t.id] = el }}
                  role="tab"
                  aria-selected={active === t.id}
                  aria-controls={`panel-${t.id}`}
                  id={`tab-${t.id}`}
                  onClick={() => setActive(t.id)}
                  className={`px-4 py-2.5 font-mono text-xs tracking-widest transition-colors duration-150 cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64] ${
                    active === t.id ? 'text-[#7fee64]' : 'text-[#677d64] hover:text-[#ddffdc]'
                  }`}
                >
                  {t.label}
                </button>
              ))}
              <span
                className="tab-indicator absolute bottom-0 h-[2px] bg-[#7fee64]"
                style={{ left: `${indicator.left}px`, width: `${indicator.width}px` }}
                aria-hidden="true"
              />
            </div>

            <div
              key={tab.id}
              id={`panel-${tab.id}`}
              role="tabpanel"
              aria-labelledby={`tab-${tab.id}`}
              className="tab-panel-fade mt-6"
            >
              <h3 className="font-display text-xl font-medium text-[#ddffdc]">
                {tab.heading}
              </h3>
              <p className="mt-3 font-body text-[15px] leading-relaxed text-[#677d64]">
                {tab.body}
              </p>
            </div>
          </Reveal>
        </div>

        {/* Right — live terminal, scenario follows the active tab */}
        <div className="flex-1 lg:max-w-[520px]" ref={inViewRef}>
          <Reveal delay={200}>
            <TerminalWindow title="aegis — zsh">
              <LiveTerminal scenario={SCENARIOS[active]} playing={inView} />
            </TerminalWindow>
          </Reveal>
        </div>
      </div>
    </section>
  )
}

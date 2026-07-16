import { useState } from 'react'
import { TerminalWindow } from '../ui/TerminalWindow'
import { LiveTerminal } from '../ui/LiveTerminal'
import { Reveal, useInView } from '../ui/Reveal'

const STEPS = [
  {
    num: '01',
    heading: 'Install Aegis',
    body: 'Use the installer, Homebrew, npm, or Cargo source install. Package-manager installs are binary-only.',
  },
  {
    num: '02',
    heading: 'Opt in to shell-proxy mode',
    body: 'Run setup-shell when you want tools that use $SHELL -c to route through Aegis.',
  },
  {
    num: '03',
    heading: 'Approve or deny in context',
    body: 'When a dangerous pattern fires, Aegis pauses and shows you the command, the risk ID, and a safer alternative. One keystroke decides.',
  },
]

// One continuous session covering all three steps — installing Aegis
// looks like this in a real terminal, start to finish.
const SESSION = [
  { k: 'cmd', prompt: '$', text: 'npm i -g @iliasalmerekov/aegis', color: '#ddffdc', step: 0 },
  { k: 'out', text: '+ @iliasalmerekov/aegis@0.6.2', color: '#485346', delay: 500 },
  { k: 'cmd', prompt: '$', text: 'aegis --version', color: '#ddffdc' },
  { k: 'out', text: 'aegis 0.6.2', color: '#7fee64', delay: 350 },
  { k: 'pause', ms: 900 },
  { k: 'cmd', prompt: '$', text: 'aegis setup-shell', color: '#ddffdc', step: 1 },
  { k: 'out', text: '✓ managed shell block installed', color: '#7fee64', delay: 450 },
  { k: 'out', text: 'commands via $SHELL -c now route through aegis', color: '#485346', delay: 300 },
  { k: 'pause', ms: 1100 },
  { k: 'cmd', prompt: 'agent:', text: 'DROP TABLE users;', color: '#ddffdc', step: 2 },
  {
    k: 'danger',
    id: 'DB-001',
    title: 'Destructive SQL — table is gone for good',
    alt: 'safe alt: ALTER TABLE users RENAME TO users_retired',
    delay: 500,
  },
  { k: 'actions', press: 'N', delay: 400 },
  { k: 'out', text: '✖ denied — logged to audit.jsonl', color: '#677d64', delay: 420 },
]

export function HowItWorks() {
  const [inViewRef, inView] = useInView(0.25)
  const [activeStep, setActiveStep] = useState(0)
  const [seek, setSeek] = useState(null)

  const jumpTo = (i) => {
    setActiveStep(i)
    setSeek({ step: i, token: (seek?.token ?? 0) + 1 })
  }

  return (
    <section
      id="how-it-works"
      className="mx-auto w-full max-w-[1200px] px-6 py-24"
      aria-labelledby="how-it-works-heading"
    >
      <Reveal>
        <p className="mb-4 font-mono text-xs font-medium tracking-widest text-[#677d64] uppercase">
          How It Works
        </p>
        <h2
          id="how-it-works-heading"
          className="font-display text-4xl font-medium leading-tight tracking-tight text-[#ddffdc] lg:text-5xl"
        >
          One session, three steps.
        </h2>
      </Reveal>

      <div
        className="mt-14 flex flex-col gap-12 lg:flex-row lg:items-start lg:gap-16"
        ref={inViewRef}
      >
        {/* Left — stepper, highlights follow the session */}
        <div className="flex w-full max-w-[400px] flex-col gap-10">
          {STEPS.map((step, i) => {
            const isActive = i === activeStep
            return (
              <Reveal key={step.num} delay={80 + i * 70}>
                <button
                  type="button"
                  onClick={() => jumpTo(i)}
                  aria-current={isActive ? 'step' : undefined}
                  className="group flex w-full cursor-pointer gap-5 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64] rounded"
                >
                  <span
                    className="font-display text-[40px] font-medium leading-none transition-colors duration-300"
                    style={{ color: isActive ? '#7fee64' : '#3e4a3c' }}
                  >
                    {step.num}
                  </span>
                  <span className="flex flex-col gap-2 pt-1">
                    <span
                      className="font-display text-lg font-medium leading-snug transition-colors duration-300 group-hover:text-[#ddffdc]"
                      style={{ color: isActive ? '#ddffdc' : '#677d64' }}
                    >
                      {step.heading}
                    </span>
                    <span
                      className="font-body text-sm leading-relaxed transition-opacity duration-300"
                      style={{ color: '#677d64', opacity: isActive ? 1 : 0.55 }}
                    >
                      {step.body}
                    </span>
                  </span>
                </button>
              </Reveal>
            )
          })}
        </div>

        {/* Right — the single neon terminal playing the whole session */}
        <div className="min-w-0 flex-1">
          <Reveal delay={160}>
            <TerminalWindow title="aegis — zsh">
              <LiveTerminal
                scenario={SESSION}
                playing={inView}
                minHeightClass="min-h-[400px]"
                onStep={setActiveStep}
                seek={seek}
              />
            </TerminalWindow>
          </Reveal>
        </div>
      </div>
    </section>
  )
}

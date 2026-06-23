import { TerminalWindow } from '../ui/TerminalWindow'

const STEPS = [
  {
    num: '01',
    heading: 'Install Aegis',
    body: 'Use the installer, Homebrew, npm, or Cargo source install. Package-manager installs are binary-only.',
    lines: [
      { prompt: '$', text: 'npm i -g @iliasalmerekov/aegis', color: '#ddffdc' },
      { prompt: '$', text: 'aegis --version', color: '#ddffdc' },
      { text: 'aegis 0.5.8', color: '#7fee64' },
    ],
  },
  {
    num: '02',
    heading: 'Opt in to shell-proxy mode',
    body: 'Run setup-shell when you want tools that use $SHELL -c to route through Aegis.',
    lines: [
      { prompt: '$', text: 'aegis setup-shell', color: '#ddffdc' },
      { text: 'managed shell block installed', color: '#7fee64' },
    ],
  },
  {
    num: '03',
    heading: 'Approve or deny in context',
    body: 'When a dangerous pattern fires, Aegis pauses and shows you the command, the risk ID, and a safer alternative. One keystroke decides.',
    lines: [
      { prompt: 'agent:', text: 'DROP TABLE users;', color: '#ddffdc' },
      { text: '⚠ DANGER — DB-001', color: '#c8f9b6' },
      { text: '[y] Allow  [N] Deny  [?] Details', color: '#677d64' },
      { prompt: '>', text: 'N', color: '#7fee64' },
      { text: '✖ denied — logged to audit.jsonl', color: '#677d64' },
    ],
  },
]

export function HowItWorks() {
  return (
    <section
      id="how-it-works"
      className="mx-auto w-full max-w-[1200px] px-6 py-24"
      aria-labelledby="how-it-works-heading"
    >
      <p className="mb-4 font-mono text-xs font-medium tracking-widest text-[#677d64] uppercase">
        How It Works
      </p>
      <h2
        id="how-it-works-heading"
        className="font-display text-4xl font-medium leading-tight tracking-tight text-[#ddffdc] lg:text-5xl"
      >
        Up in three steps.
      </h2>

      <div
        className="mt-14 grid items-start gap-8"
        style={{ gridTemplateColumns: 'repeat(1, 1fr)' }}
      >
        {/* Steps — desktop: first two equal, third full-width highlight */}
        <div className="grid gap-8 md:grid-cols-2 lg:grid-cols-[1fr_1fr_1.5fr]"
          style={{ alignItems: 'start' }}>
          {STEPS.map((step, i) => (
            <div
              key={step.num}
              className="flex flex-col gap-4"
            >
              <span
                className="font-display font-medium leading-none text-[#7fee64]"
                style={{ fontSize: i === 2 ? '52px' : '40px' }}
              >
                {step.num}
              </span>
              <h3
                className="font-display font-medium text-[#ddffdc]"
                style={{ fontSize: i === 2 ? '22px' : '20px' }}
              >
                {step.heading}
              </h3>
              <p className="font-body text-sm leading-relaxed text-[#677d64]">
                {step.body}
              </p>
              <TerminalWindow title="aegis" className="mt-2">
                <div className="space-y-1 font-mono text-xs">
                  {step.lines.map((line, j) => (
                    <p key={j} style={{ color: line.color }}>
                      {line.prompt && (
                        <span className="mr-2 text-[#677d64]">{line.prompt}</span>
                      )}
                      {line.text}
                    </p>
                  ))}
                </div>
              </TerminalWindow>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}

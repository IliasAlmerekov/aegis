import { TerminalWindow } from '../ui/TerminalWindow'

const ENTRIES = [
  {
    ts: '2026-06-17T09:12:43Z',
    cmd: 'rm -rf /var/log/nginx',
    decision: 'denied',
    pattern: 'FS-001',
  },
  {
    ts: '2026-06-17T09:13:01Z',
    cmd: 'git reset --hard HEAD~3',
    decision: 'approved',
    pattern: 'GIT-001',
  },
  {
    ts: '2026-06-17T09:15:22Z',
    cmd: 'DROP TABLE sessions;',
    decision: 'denied',
    pattern: 'DB-001',
  },
  {
    ts: '2026-06-17T09:17:08Z',
    cmd: 'cargo build --release',
    decision: 'pass',
    pattern: null,
  },
  {
    ts: '2026-06-17T09:18:55Z',
    cmd: 'kubectl delete pod api-7f9d --force',
    decision: 'denied',
    pattern: 'CL-003',
  },
]

const DECISION_COLOR = {
  denied: '#859085',
  approved: '#7fee64',
  pass: '#485346',
}

export function AuditSection() {
  return (
    <section
      id="audit-trail"
      className="mx-auto w-full max-w-[1200px] px-6 py-24"
      aria-labelledby="audit-heading"
    >
      <div className="flex flex-col gap-12 lg:flex-row lg:items-start lg:gap-16">
        {/* Left */}
        <div className="flex max-w-[380px] flex-col">
          <p className="mb-4 font-mono text-xs font-medium tracking-widest text-[#677d64] uppercase">
            Audit Trail
          </p>
          <h2
            id="audit-heading"
            className="font-display text-4xl font-medium leading-tight tracking-tight text-[#ddffdc] lg:text-5xl"
          >
            Every decision,{' '}
            <span className="text-[#7fee64]">on the record.</span>
          </h2>
          <p className="mt-5 font-body text-[15px] leading-relaxed text-[#677d64]">
            Aegis appends a JSONL entry to{' '}
            <code className="font-mono text-xs text-[#ddffdc]">~/.aegis/audit.jsonl</code>{' '}
            for every command — approved, denied, or auto-passed. Append-only,
            structured, and tamper-evident when hash-chain integrity is enabled.
          </p>
          <a
            href="https://github.com/IliasAlmerekov/aegis#audit-log"
            target="_blank"
            rel="noopener noreferrer"
            className="mt-6 inline-flex items-center gap-1.5 font-mono text-sm text-[#7fee64] hover:text-[#c8f9b6] transition-colors duration-150 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#7fee64] rounded"
          >
            View log format
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M5 12h14M12 5l7 7-7 7"/>
            </svg>
          </a>
        </div>

        {/* Right — JSONL terminal */}
        <div className="flex-1">
          <TerminalWindow title="~/.aegis/audit.jsonl">
            <div className="space-y-3 font-mono text-xs">
              {ENTRIES.map((e, i) => (
                <div key={i} className="flex flex-col gap-0.5 border-b border-[#3e4a3c]/40 pb-3 last:border-0 last:pb-0">
                  <div className="flex items-start gap-2 flex-wrap">
                    <span className="text-[#3e4a3c]">{'{'}</span>
                    <span className="text-[#677d64]">&quot;ts&quot;:</span>
                    <span className="text-[#aed2a4]">&quot;{e.ts}&quot;</span>
                    <span className="text-[#677d64]">,</span>
                  </div>
                  <div className="pl-4 flex gap-2 flex-wrap">
                    <span className="text-[#677d64]">&quot;cmd&quot;:</span>
                    <span className="text-[#ddffdc]">&quot;{e.cmd}&quot;</span>
                    <span className="text-[#677d64]">,</span>
                  </div>
                  <div className="pl-4 flex gap-2">
                    <span className="text-[#677d64]">&quot;decision&quot;:</span>
                    <span style={{ color: DECISION_COLOR[e.decision] }}>
                      &quot;{e.decision}&quot;
                    </span>
                    {e.pattern && (
                      <>
                        <span className="text-[#677d64]">, &quot;pattern&quot;:</span>
                        <span className="text-[#677d64]">&quot;{e.pattern}&quot;</span>
                      </>
                    )}
                  </div>
                  <span className="text-[#3e4a3c]">{'}'}</span>
                </div>
              ))}
            </div>
          </TerminalWindow>
        </div>
      </div>
    </section>
  )
}

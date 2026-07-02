export function TerminalWindow({ title = 'aegis — zsh', children, className = '' }) {
  // Desync the perimeter scan between instances so terminals visible
  // together don't pulse in lockstep. Derived from the title —
  // deterministic across renders, no state needed.
  const neonDelay = -((title.length * 1.3) % 7)

  return (
    <div
      className={`terminal-neon rounded-lg ${className}`}
      style={{ '--neon-delay': `${neonDelay.toFixed(2)}s` }}
    >
      <div
        className="rounded-lg overflow-hidden border border-[#3e4a3c]"
        style={{ backgroundColor: '#0d1210' }}
      >
        {/* Titlebar */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[#3e4a3c]/60" style={{ backgroundColor: '#1a2018' }}>
          <span className="h-3 w-3 rounded-full" style={{ backgroundColor: '#ff5f56' }} aria-hidden="true" />
          <span className="h-3 w-3 rounded-full" style={{ backgroundColor: '#ffbd2e' }} aria-hidden="true" />
          <span className="h-3 w-3 rounded-full" style={{ backgroundColor: '#27c93f' }} aria-hidden="true" />
          <span className="ml-2 font-mono text-xs text-[#677d64]">{title}</span>
        </div>
        <div className="p-4">{children}</div>
      </div>
    </div>
  )
}
